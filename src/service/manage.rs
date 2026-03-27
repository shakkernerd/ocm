use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::inspect::{
    ServiceLaunchSpec, current_uid, managed_plist_path, managed_service_label,
    resolve_service_launch, service_status,
};
use crate::env::{EnvMeta, EnvironmentService};
use crate::infra::shell::build_openclaw_env;
use crate::store::{derive_env_paths, display_path, list_environments, save_environment};

const DEFAULT_SERVICE_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
const LAUNCH_AGENT_DIR_MODE: u32 = 0o755;
const LAUNCH_AGENT_PLIST_MODE: u32 = 0o644;
const LAUNCH_AGENT_THROTTLE_INTERVAL_SECONDS: u32 = 1;
const LAUNCH_AGENT_UMASK_DECIMAL: u32 = 0o077;
const SERVICE_PROXY_ENV_KEYS: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
];
const SERVICE_EXTRA_ENV_KEYS: [&str; 2] = ["NODE_EXTRA_CA_CERTS", "NODE_USE_SYSTEM_CA"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstallSummary {
    pub env_name: String,
    pub service_kind: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub gateway_port: u32,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub args: Vec<String>,
    pub run_dir: String,
    pub log_dir: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub persisted_gateway_port: bool,
    pub previous_gateway_port: Option<u32>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceActionSummary {
    pub env_name: String,
    pub service_kind: String,
    pub action: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub installed: bool,
    pub gateway_port: Option<u32>,
    pub warnings: Vec<String>,
}

struct PreparedService {
    env_meta: EnvMeta,
    binding_kind: String,
    binding_name: String,
    command: Option<String>,
    binary_path: Option<String>,
    args: Vec<String>,
    program_arguments: Vec<String>,
    run_dir: PathBuf,
    managed_label: String,
    managed_plist_path: PathBuf,
    log_dir: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    warnings: Vec<String>,
    persisted_gateway_port: bool,
    previous_gateway_port: Option<u32>,
}

struct PersistedGatewayPort {
    env_meta: EnvMeta,
    persisted_gateway_port: bool,
    previous_gateway_port: Option<u32>,
}

pub fn install_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceInstallSummary, String> {
    let prepared = prepare_service(name, env, cwd)?;
    write_plist_file(&prepared, env)?;
    activate_launch_agent(&prepared.managed_label, &prepared.managed_plist_path)?;

    Ok(ServiceInstallSummary {
        env_name: prepared.env_meta.name.clone(),
        service_kind: "gateway".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        gateway_port: prepared.env_meta.gateway_port.unwrap_or_default(),
        binding_kind: prepared.binding_kind,
        binding_name: prepared.binding_name,
        command: prepared.command,
        binary_path: prepared.binary_path,
        args: prepared.args,
        run_dir: display_path(&prepared.run_dir),
        log_dir: display_path(&prepared.log_dir),
        stdout_path: display_path(&prepared.stdout_path),
        stderr_path: display_path(&prepared.stderr_path),
        persisted_gateway_port: prepared.persisted_gateway_port,
        previous_gateway_port: prepared.previous_gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn start_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let prepared = prepare_existing_service(name, env, cwd)?;
    let domain = gui_domain();
    let target = format!("{domain}/{}", prepared.managed_label);

    let start = run_launchctl(["kickstart", "-k", target.as_str()])?;
    if !start.status.success() {
        if !launchctl_not_loaded(&start) {
            return Err(format!(
                "launchctl kickstart failed: {}",
                launchctl_detail(&start)
            ));
        }
        if !prepared.managed_plist_path.exists() {
            return Err(format!(
                "service for env \"{}\" is not installed; run service install first",
                prepared.env_meta.name
            ));
        }
        activate_launch_agent(&prepared.managed_label, &prepared.managed_plist_path)?;
    }

    Ok(ServiceActionSummary {
        env_name: prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "start".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        installed: prepared.managed_plist_path.exists(),
        gateway_port: prepared.env_meta.gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn stop_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let prepared = prepare_existing_service(name, env, cwd)?;
    let target = format!("{}/{}", gui_domain(), prepared.managed_label);
    let bootout = run_launchctl(["bootout", target.as_str()])?;
    if !bootout.status.success() && !launchctl_not_loaded(&bootout) {
        return Err(format!(
            "launchctl bootout failed: {}",
            launchctl_detail(&bootout)
        ));
    }

    Ok(ServiceActionSummary {
        env_name: prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "stop".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        installed: prepared.managed_plist_path.exists(),
        gateway_port: prepared.env_meta.gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn restart_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let prepared = prepare_existing_service(name, env, cwd)?;
    let target = format!("{}/{}", gui_domain(), prepared.managed_label);

    let restart = run_launchctl(["kickstart", "-k", target.as_str()])?;
    if !restart.status.success() {
        if !launchctl_not_loaded(&restart) {
            return Err(format!(
                "launchctl kickstart failed: {}",
                launchctl_detail(&restart)
            ));
        }
        if !prepared.managed_plist_path.exists() {
            return Err(format!(
                "service for env \"{}\" is not installed; run service install first",
                prepared.env_meta.name
            ));
        }
        activate_launch_agent(&prepared.managed_label, &prepared.managed_plist_path)?;
        let retry = run_launchctl(["kickstart", "-k", target.as_str()])?;
        if !retry.status.success() {
            return Err(format!(
                "launchctl kickstart failed: {}",
                launchctl_detail(&retry)
            ));
        }
    }

    Ok(ServiceActionSummary {
        env_name: prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "restart".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        installed: prepared.managed_plist_path.exists(),
        gateway_port: prepared.env_meta.gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn uninstall_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let prepared = prepare_existing_service(name, env, cwd)?;
    let target = format!("{}/{}", gui_domain(), prepared.managed_label);
    let _ = run_launchctl(["bootout", target.as_str()]);
    let plist_path = display_path(&prepared.managed_plist_path);
    if prepared.managed_plist_path.exists() {
        fs::remove_file(&prepared.managed_plist_path).map_err(|error| error.to_string())?;
    }

    Ok(ServiceActionSummary {
        env_name: prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "uninstall".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: plist_path,
        installed: false,
        gateway_port: prepared.env_meta.gateway_port,
        warnings: prepared.warnings,
    })
}

fn prepare_existing_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PreparedService, String> {
    prepare_service(name, env, cwd)
}

fn prepare_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PreparedService, String> {
    let port_assignment = persist_service_gateway_port(name, env, cwd)?;
    let env_meta = port_assignment.env_meta;
    let launch = resolve_service_launch(&env_meta, env, cwd)?;
    let managed_label = managed_service_label(&env_meta.name);
    let managed_plist_path = managed_plist_path(&env_meta.name, env);
    let log_dir = derive_env_paths(Path::new(&env_meta.root)).state_dir.join("logs");
    let stdout_path = log_dir.join("gateway.log");
    let stderr_path = log_dir.join("gateway.err.log");
    let (binding_kind, binding_name, command, binary_path, args, program_arguments, run_dir) =
        match launch {
            ServiceLaunchSpec::Launcher {
                binding_name,
                command,
                run_dir,
            } => (
                "launcher".to_string(),
                binding_name,
                Some(command.clone()),
                None,
                Vec::new(),
                vec!["/bin/sh".to_string(), "-lc".to_string(), command],
                run_dir,
            ),
            ServiceLaunchSpec::Runtime {
                binding_name,
                binary_path,
                args,
                run_dir,
            } => {
                let mut program_arguments = vec![binary_path.clone()];
                program_arguments.extend(args.iter().cloned());
                (
                    "runtime".to_string(),
                    binding_name,
                    None,
                    Some(binary_path),
                    args,
                    program_arguments,
                    run_dir,
                )
            }
        };

    let warnings = service_install_warnings(
        name,
        port_assignment.previous_gateway_port,
        env_meta.gateway_port.unwrap_or_default(),
        port_assignment.persisted_gateway_port,
    );

    Ok(PreparedService {
        env_meta,
        binding_kind,
        binding_name,
        command,
        binary_path,
        args,
        program_arguments,
        run_dir,
        managed_label,
        managed_plist_path,
        log_dir,
        stdout_path,
        stderr_path,
        warnings,
        persisted_gateway_port: port_assignment.persisted_gateway_port,
        previous_gateway_port: port_assignment.previous_gateway_port,
    })
}

fn service_install_warnings(
    name: &str,
    previous_gateway_port: Option<u32>,
    gateway_port: u32,
    persisted_gateway_port: bool,
) -> Vec<String> {
    if !persisted_gateway_port {
        return Vec::new();
    }

    match previous_gateway_port {
        Some(previous) => vec![format!(
            "gateway port {previous} was unavailable; assigned {gateway_port} to env \"{name}\" and saved it to env metadata"
        )],
        None => vec![format!(
            "assigned gateway port {gateway_port} to env \"{name}\" and saved it to env metadata for service stability"
        )],
    }
}

fn persist_service_gateway_port(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PersistedGatewayPort, String> {
    let service = EnvironmentService::new(env, cwd);
    let original = service.get(name)?;
    let effective = service.apply_effective_gateway_port(original.clone())?;
    let current_summary = service_status(name, env, cwd)?;
    let reserved_ports = collect_reserved_ports(name, env, cwd)?;
    let preferred_port = effective.gateway_port.unwrap_or_default();
    let chosen_port = choose_gateway_port(preferred_port, &reserved_ports, &current_summary);
    let persisted_gateway_port = original.gateway_port != Some(chosen_port);
    let previous_gateway_port = if chosen_port != preferred_port {
        Some(preferred_port)
    } else {
        None
    };

    if original.gateway_port == Some(chosen_port) {
        let mut env_meta = original;
        env_meta.gateway_port = Some(chosen_port);
        return Ok(PersistedGatewayPort {
            env_meta,
            persisted_gateway_port,
            previous_gateway_port,
        });
    }

    let mut updated = original;
    updated.gateway_port = Some(chosen_port);
    Ok(PersistedGatewayPort {
        env_meta: save_environment(updated, env, cwd)?,
        persisted_gateway_port,
        previous_gateway_port,
    })
}

fn collect_reserved_ports(
    target_name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<BTreeSet<u32>, String> {
    let service = EnvironmentService::new(env, cwd);
    let mut ports = BTreeSet::new();
    for meta in list_environments(env, cwd)? {
        if meta.name == target_name {
            continue;
        }
        let port = service.apply_effective_gateway_port(meta)?.gateway_port;
        if let Some(port) = port {
            ports.insert(port);
        }
    }
    Ok(ports)
}

fn choose_gateway_port(
    preferred_port: u32,
    reserved_ports: &BTreeSet<u32>,
    current_summary: &super::ServiceSummary,
) -> u32 {
    let mut port = preferred_port.max(18_789);
    loop {
        let managed_self_running =
            current_summary.installed && (current_summary.loaded || current_summary.running);
        let available = !reserved_ports.contains(&port)
            && (port_is_available(port) || managed_self_running && current_summary.gateway_port == port);
        if available {
            return port;
        }
        port = port.saturating_add(1);
    }
}

fn port_is_available(port: u32) -> bool {
    TcpListener::bind(("127.0.0.1", port as u16)).is_ok()
}

fn write_plist_file(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    ensure_secure_dir(&prepared.log_dir)?;
    if let Some(parent) = prepared.managed_plist_path.parent() {
        ensure_secure_dir(parent)?;
    }

    let plist = build_launch_agent_plist(
        &prepared.managed_label,
        &format!(
            "OCM-managed OpenClaw gateway service for env {}",
            prepared.env_meta.name
        ),
        &prepared.program_arguments,
        &prepared.run_dir,
        &prepared.stdout_path,
        &prepared.stderr_path,
        &build_service_environment(
            &prepared.env_meta,
            &prepared.managed_label,
            env,
        ),
    );
    fs::write(&prepared.managed_plist_path, plist).map_err(|error| error.to_string())?;
    set_mode(&prepared.managed_plist_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn build_service_environment(
    env_meta: &EnvMeta,
    launchd_label: &str,
    process_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut base_env = BTreeMap::new();
    if let Some(home) = process_env.get("HOME").filter(|value| !value.trim().is_empty()) {
        base_env.insert("HOME".to_string(), home.trim().to_string());
    }
    if let Some(path) = process_env.get("PATH").filter(|value| !value.trim().is_empty()) {
        base_env.insert("PATH".to_string(), path.trim().to_string());
    } else {
        base_env.insert("PATH".to_string(), DEFAULT_SERVICE_PATH.to_string());
    }
    if let Some(tmpdir) = process_env
        .get("TMPDIR")
        .filter(|value| !value.trim().is_empty())
    {
        base_env.insert("TMPDIR".to_string(), tmpdir.trim().to_string());
    }
    for key in SERVICE_PROXY_ENV_KEYS {
        if let Some(value) = process_env.get(key).filter(|value| !value.trim().is_empty()) {
            base_env.insert(key.to_string(), value.trim().to_string());
        }
    }
    for key in SERVICE_EXTRA_ENV_KEYS {
        if let Some(value) = process_env.get(key).filter(|value| !value.trim().is_empty()) {
            base_env.insert(key.to_string(), value.trim().to_string());
        }
    }

    let mut service_env = build_openclaw_env(env_meta, &base_env);
    service_env.insert("OPENCLAW_PROFILE".to_string(), String::new());
    service_env.insert(
        "OPENCLAW_LAUNCHD_LABEL".to_string(),
        launchd_label.to_string(),
    );
    service_env
}

fn build_launch_agent_plist(
    label: &str,
    comment: &str,
    program_arguments: &[String],
    working_directory: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    environment: &BTreeMap<String, String>,
) -> String {
    let args_xml = program_arguments
        .iter()
        .map(|arg| format!("\n      <string>{}</string>", plist_escape(arg)))
        .collect::<String>();
    let env_xml = if environment.is_empty() {
        String::new()
    } else {
        let items = environment
            .iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(key, value)| {
                format!(
                    "\n      <key>{}</key>\n      <string>{}</string>",
                    plist_escape(key),
                    plist_escape(value)
                )
            })
            .collect::<String>();
        format!("\n    <key>EnvironmentVariables</key>\n    <dict>{items}\n    </dict>")
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n  <dict>\n    <key>Label</key>\n    <string>{}</string>\n    <key>Comment</key>\n    <string>{}</string>\n    <key>RunAtLoad</key>\n    <true/>\n    <key>KeepAlive</key>\n    <true/>\n    <key>ThrottleInterval</key>\n    <integer>{}</integer>\n    <key>Umask</key>\n    <integer>{}</integer>\n    <key>ProgramArguments</key>\n    <array>{}\n    </array>\n    <key>WorkingDirectory</key>\n    <string>{}</string>\n    <key>StandardOutPath</key>\n    <string>{}</string>\n    <key>StandardErrorPath</key>\n    <string>{}</string>{}\n  </dict>\n</plist>\n",
        plist_escape(label),
        plist_escape(comment),
        LAUNCH_AGENT_THROTTLE_INTERVAL_SECONDS,
        LAUNCH_AGENT_UMASK_DECIMAL,
        args_xml,
        plist_escape(&display_path(working_directory)),
        plist_escape(&display_path(stdout_path)),
        plist_escape(&display_path(stderr_path)),
        env_xml,
    )
}

fn plist_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ensure_secure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())?;
    set_mode(path, LAUNCH_AGENT_DIR_MODE)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn activate_launch_agent(label: &str, plist_path: &Path) -> Result<(), String> {
    let domain = gui_domain();
    let plist_path = display_path(plist_path);
    let _ = run_launchctl(["bootout", domain.as_str(), plist_path.as_str()]);
    let _ = run_launchctl(["unload", plist_path.as_str()]);
    let target = format!("{domain}/{label}");
    let _ = run_launchctl(["enable", target.as_str()]);
    let bootstrap = run_launchctl(["bootstrap", domain.as_str(), plist_path.as_str()])?;
    if !bootstrap.status.success() {
        return Err(format!(
            "launchctl bootstrap failed: {}",
            launchctl_detail(&bootstrap)
        ));
    }
    Ok(())
}

fn gui_domain() -> String {
    current_uid()
        .map(|uid| format!("gui/{uid}"))
        .unwrap_or_else(|| "gui/501".to_string())
}

fn run_launchctl<const N: usize>(args: [&str; N]) -> Result<std::process::Output, String> {
    Command::new("launchctl")
        .args(args)
        .output()
        .map_err(|error| format!("failed to run \"launchctl\": {error}"))
}

fn launchctl_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn launchctl_not_loaded(output: &std::process::Output) -> bool {
    let detail = launchctl_detail(output).to_ascii_lowercase();
    detail.contains("no such process")
        || detail.contains("could not find service")
        || detail.contains("not found")
}

#[cfg(test)]
mod tests {
    use super::{build_launch_agent_plist, plist_escape, service_install_warnings};
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn plist_escape_handles_xml_special_characters() {
        assert_eq!(
            plist_escape("a&b<c>d\"e'f"),
            "a&amp;b&lt;c&gt;d&quot;e&apos;f"
        );
    }

    #[test]
    fn install_warnings_explain_port_persistence_and_reassignment() {
        assert_eq!(
            service_install_warnings("test", None, 18790, true),
            vec![
                "assigned gateway port 18790 to env \"test\" and saved it to env metadata for service stability"
            ]
        );
        assert_eq!(
            service_install_warnings("test", Some(18789), 18790, true),
            vec![
                "gateway port 18789 was unavailable; assigned 18790 to env \"test\" and saved it to env metadata"
            ]
        );
    }

    #[test]
    fn launch_agent_plist_includes_program_arguments_and_environment() {
        let mut environment = BTreeMap::new();
        environment.insert("PATH".to_string(), "/usr/bin".to_string());
        let plist = build_launch_agent_plist(
            "ai.openclaw.gateway.ocm.test",
            "test",
            &["/bin/sh".to_string(), "-lc".to_string(), "openclaw gateway run".to_string()],
            Path::new("/tmp/work"),
            Path::new("/tmp/stdout.log"),
            Path::new("/tmp/stderr.log"),
            &environment,
        );
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("ai.openclaw.gateway.ocm.test"));
        assert!(plist.contains("openclaw gateway run"));
        assert!(plist.contains("<key>EnvironmentVariables</key>"));
        assert!(plist.contains("/tmp/stdout.log"));
    }
}
