use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::inspect::{
    GLOBAL_GATEWAY_LABEL, ServiceLaunchSpec, current_uid, global_plist_path, managed_plist_path,
    managed_service_label, resolve_service_launch, service_status,
};
use super::platform::{ServiceManagerKind, service_manager_kind};
use crate::env::{EnvMeta, EnvironmentService};
use crate::infra::shell::build_openclaw_env;
use crate::store::{
    derive_env_paths, display_path, list_environments, now_utc, resolve_ocm_home, save_environment,
};

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

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLogSummary {
    pub env_name: String,
    pub service_kind: String,
    pub stream: String,
    pub path: String,
    pub tail_lines: Option<usize>,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAdoptionSummary {
    pub env_name: String,
    pub service_kind: String,
    pub global_label: String,
    pub global_plist_path: String,
    pub backup_plist_path: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub gateway_port: u32,
    pub dry_run: bool,
    pub adopted: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRestoreSummary {
    pub env_name: String,
    pub service_kind: String,
    pub global_label: String,
    pub global_plist_path: String,
    pub backup_plist_path: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub gateway_port: u32,
    pub dry_run: bool,
    pub restored: bool,
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

struct GlobalServiceBinding {
    plist_path: PathBuf,
    config_path: Option<String>,
    gateway_port: Option<u32>,
}

struct PreparedGlobalAdoption {
    prepared: PreparedService,
    global: GlobalServiceBinding,
    backup_plist_path: PathBuf,
}

struct PreparedGlobalRestore {
    env_meta: EnvMeta,
    global_plist_path: PathBuf,
    backup_plist_path: PathBuf,
    managed_label: String,
    managed_plist_path: PathBuf,
    gateway_port: u32,
    warnings: Vec<String>,
}

pub fn install_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceInstallSummary, String> {
    let prepared = prepare_service(name, env, cwd)?;
    write_service_definition(&prepared, env)?;
    activate_managed_service(&prepared.managed_label, &prepared.managed_plist_path, env)?;

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

pub fn adopt_global_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    dry_run: bool,
) -> Result<ServiceAdoptionSummary, String> {
    ensure_launchd_only("service adopt-global", env)?;
    let adoption = prepare_global_adoption(name, env, cwd, !dry_run)?;
    if !dry_run {
        write_service_definition(&adoption.prepared, env)?;
        backup_global_plist(&adoption.global.plist_path, &adoption.backup_plist_path)?;
        bootout_global_service()?;
        activate_managed_service(
            &adoption.prepared.managed_label,
            &adoption.prepared.managed_plist_path,
            env,
        )?;
        fs::remove_file(&adoption.global.plist_path).map_err(|error| error.to_string())?;
    }

    Ok(ServiceAdoptionSummary {
        env_name: adoption.prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_plist_path: display_path(&adoption.global.plist_path),
        backup_plist_path: display_path(&adoption.backup_plist_path),
        managed_label: adoption.prepared.managed_label,
        managed_plist_path: display_path(&adoption.prepared.managed_plist_path),
        gateway_port: adoption.prepared.env_meta.gateway_port.unwrap_or_default(),
        dry_run,
        adopted: !dry_run,
        warnings: adoption.prepared.warnings,
    })
}

pub fn restore_global_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    dry_run: bool,
) -> Result<ServiceRestoreSummary, String> {
    ensure_launchd_only("service restore-global", env)?;
    let restore = prepare_global_restore(name, env, cwd)?;
    if !dry_run {
        restore_global_plist(&restore.backup_plist_path, &restore.global_plist_path)?;
        bootout_managed_service(&restore.managed_label)?;
        activate_launch_agent(GLOBAL_GATEWAY_LABEL, &restore.global_plist_path)?;
        if restore.managed_plist_path.exists() {
            fs::remove_file(&restore.managed_plist_path).map_err(|error| error.to_string())?;
        }
    }

    Ok(ServiceRestoreSummary {
        env_name: restore.env_meta.name,
        service_kind: "gateway".to_string(),
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_plist_path: display_path(&restore.global_plist_path),
        backup_plist_path: display_path(&restore.backup_plist_path),
        managed_label: restore.managed_label,
        managed_plist_path: display_path(&restore.managed_plist_path),
        gateway_port: restore.gateway_port,
        dry_run,
        restored: !dry_run,
        warnings: restore.warnings,
    })
}

pub fn start_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let prepared = prepare_existing_service(name, env, cwd)?;
    start_managed_service(&prepared, env)?;

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
    stop_managed_service(&prepared, env)?;

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
    restart_managed_service(&prepared, env)?;

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
    let service = EnvironmentService::new(env, cwd);
    let env_meta = service.apply_effective_gateway_port(service.get(name)?)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;

    uninstall_managed_service_by_label(&managed_label, &managed_plist_path, env)?;
    let plist_path = display_path(&managed_plist_path);
    if managed_plist_path.exists() {
        fs::remove_file(&managed_plist_path).map_err(|error| error.to_string())?;
    }

    Ok(ServiceActionSummary {
        env_name: env_meta.name,
        service_kind: "gateway".to_string(),
        action: "uninstall".to_string(),
        managed_label,
        managed_plist_path: plist_path,
        installed: false,
        gateway_port: env_meta.gateway_port,
        warnings: Vec::new(),
    })
}

pub fn service_logs(
    name: &str,
    stream: &str,
    tail_lines: Option<usize>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceLogSummary, String> {
    let env_meta = EnvironmentService::new(env, cwd).get(name)?;
    let stream = normalize_log_stream(stream)?;
    if service_manager_kind(env) == ServiceManagerKind::SystemdUser {
        let label = managed_service_label(&env_meta.name, env, cwd)?;
        let path = format!("journalctl --user --unit {label}");
        let content = read_systemd_service_logs(&label, tail_lines)?;
        return Ok(ServiceLogSummary {
            env_name: env_meta.name,
            service_kind: "gateway".to_string(),
            stream: stream.to_string(),
            path,
            tail_lines,
            content,
        });
    }

    let log_path = match stream {
        "stdout" => service_stdout_log_path(&env_meta),
        "stderr" => service_stderr_log_path(&env_meta),
        _ => unreachable!("normalize_log_stream validates service log stream"),
    };

    if !log_path.exists() {
        return Err(format!(
            "{} log does not exist for env \"{}\": {}",
            stream,
            env_meta.name,
            display_path(&log_path)
        ));
    }

    let raw = fs::read_to_string(&log_path).map_err(|error| error.to_string())?;
    let content = if let Some(tail_lines) = tail_lines {
        tail_text(&raw, tail_lines)
    } else {
        raw
    };

    Ok(ServiceLogSummary {
        env_name: env_meta.name,
        service_kind: "gateway".to_string(),
        stream: stream.to_string(),
        path: display_path(&log_path),
        tail_lines,
        content,
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
    prepare_service_with_allowed_busy_port(name, env, cwd, None, true)
}

fn prepare_service_with_allowed_busy_port(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    allowed_busy_port: Option<u32>,
    persist_gateway_port: bool,
) -> Result<PreparedService, String> {
    let port_assignment =
        persist_service_gateway_port(name, env, cwd, allowed_busy_port, persist_gateway_port)?;
    let env_meta = port_assignment.env_meta;
    let launch = resolve_service_launch(&env_meta, env, cwd)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;
    let log_dir = service_log_dir(&env_meta);
    let stdout_path = service_stdout_log_path(&env_meta);
    let stderr_path = service_stderr_log_path(&env_meta);
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
    allowed_busy_port: Option<u32>,
    persist_gateway_port: bool,
) -> Result<PersistedGatewayPort, String> {
    let service = EnvironmentService::new(env, cwd);
    let original = service.get(name)?;
    let effective = service.apply_effective_gateway_port(original.clone())?;
    let current_summary = service_status(name, env, cwd)?;
    let reserved_ports = collect_reserved_ports(name, env, cwd)?;
    let preferred_port = effective.gateway_port.unwrap_or_default();
    let chosen_port = choose_gateway_port(
        preferred_port,
        &reserved_ports,
        &current_summary,
        allowed_busy_port,
    );
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
    let env_meta = if persist_gateway_port {
        save_environment(updated, env, cwd)?
    } else {
        updated
    };
    Ok(PersistedGatewayPort {
        env_meta,
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
    allowed_busy_port: Option<u32>,
) -> u32 {
    let mut port = preferred_port.max(18_789);
    loop {
        let managed_self_running =
            current_summary.installed && (current_summary.loaded || current_summary.running);
        let allowed_busy = allowed_busy_port == Some(port);
        let available = !reserved_ports.contains(&port)
            && (port_is_available(port)
                || allowed_busy
                || managed_self_running && current_summary.gateway_port == port);
        if available {
            return port;
        }
        port = port.saturating_add(1);
    }
}

fn port_is_available(port: u32) -> bool {
    TcpListener::bind(("127.0.0.1", port as u16)).is_ok()
}

fn normalize_log_stream(stream: &str) -> Result<&str, String> {
    match stream.trim().to_ascii_lowercase().as_str() {
        "stdout" => Ok("stdout"),
        "stderr" => Ok("stderr"),
        _ => Err(format!("unsupported service log stream: {stream}")),
    }
}

fn tail_text(raw: &str, tail_lines: usize) -> String {
    if tail_lines == 0 {
        return String::new();
    }

    let trailing_newline = raw.ends_with('\n');
    let lines = raw.lines().collect::<Vec<_>>();
    if lines.len() <= tail_lines {
        return raw.to_string();
    }

    let mut content = lines[lines.len() - tail_lines..].join("\n");
    if trailing_newline {
        content.push('\n');
    }
    content
}

fn service_log_dir(env_meta: &EnvMeta) -> PathBuf {
    derive_env_paths(Path::new(&env_meta.root))
        .state_dir
        .join("logs")
}

fn service_stdout_log_path(env_meta: &EnvMeta) -> PathBuf {
    service_log_dir(env_meta).join("gateway.log")
}

fn service_stderr_log_path(env_meta: &EnvMeta) -> PathBuf {
    service_log_dir(env_meta).join("gateway.err.log")
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
        &build_service_environment(&prepared.env_meta, &prepared.managed_label, env),
    );
    fs::write(&prepared.managed_plist_path, plist).map_err(|error| error.to_string())?;
    set_mode(&prepared.managed_plist_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn write_service_definition(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => write_plist_file(prepared, env),
        ServiceManagerKind::SystemdUser => write_systemd_unit_file(prepared, env),
    }
}

fn write_systemd_unit_file(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    ensure_secure_dir(&prepared.log_dir)?;
    if let Some(parent) = prepared.managed_plist_path.parent() {
        ensure_secure_dir(parent)?;
    }

    let unit = build_systemd_unit(
        &prepared.managed_label,
        &format!(
            "OCM-managed OpenClaw gateway service for env {}",
            prepared.env_meta.name
        ),
        &prepared.program_arguments,
        &prepared.run_dir,
        &build_service_environment(&prepared.env_meta, &prepared.managed_label, env),
    );
    fs::write(&prepared.managed_plist_path, unit).map_err(|error| error.to_string())?;
    set_mode(&prepared.managed_plist_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn prepare_global_adoption(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    persist_gateway_port: bool,
) -> Result<PreparedGlobalAdoption, String> {
    let target_env = EnvironmentService::new(env, cwd).get(name)?;
    let global = read_global_service_binding(env)?;
    let target_config_path =
        display_path(&derive_env_paths(Path::new(&target_env.root)).config_path);
    let global_config_path = global.config_path.clone().ok_or_else(|| {
        "global OpenClaw service does not expose OPENCLAW_CONFIG_PATH for adoption".to_string()
    })?;
    if global_config_path != target_config_path {
        return Err(format!(
            "global OpenClaw service points at a different env; expected {} but found {}",
            target_config_path, global_config_path
        ));
    }

    Ok(PreparedGlobalAdoption {
        prepared: prepare_service_with_allowed_busy_port(
            name,
            env,
            cwd,
            global.gateway_port,
            persist_gateway_port,
        )?,
        backup_plist_path: global_plist_backup_path(env, cwd)?,
        global,
    })
}

fn prepare_global_restore(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PreparedGlobalRestore, String> {
    let service = EnvironmentService::new(env, cwd);
    let env_meta = service.apply_effective_gateway_port(service.get(name)?)?;
    let global_plist_path = global_plist_path(env);
    if global_plist_path.exists() {
        return Err(
            "global OpenClaw service is already installed; remove it before restoring from backup"
                .to_string(),
        );
    }

    let backup = latest_matching_global_plist_backup(&env_meta, env, cwd)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;
    let mut warnings = Vec::new();
    if !managed_plist_path.exists() {
        warnings.push(format!(
            "managed service plist is absent for env \"{}\"; restoring the global plist anyway",
            env_meta.name
        ));
    }

    Ok(PreparedGlobalRestore {
        gateway_port: backup
            .gateway_port
            .or(env_meta.gateway_port)
            .unwrap_or_default(),
        env_meta,
        global_plist_path,
        backup_plist_path: backup.plist_path,
        managed_label,
        managed_plist_path,
        warnings,
    })
}

fn build_service_environment(
    env_meta: &EnvMeta,
    launchd_label: &str,
    process_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut base_env = BTreeMap::new();
    if let Some(home) = process_env
        .get("HOME")
        .filter(|value| !value.trim().is_empty())
    {
        base_env.insert("HOME".to_string(), home.trim().to_string());
    }
    if let Some(path) = process_env
        .get("PATH")
        .filter(|value| !value.trim().is_empty())
    {
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
        if let Some(value) = process_env
            .get(key)
            .filter(|value| !value.trim().is_empty())
        {
            base_env.insert(key.to_string(), value.trim().to_string());
        }
    }
    for key in SERVICE_EXTRA_ENV_KEYS {
        if let Some(value) = process_env
            .get(key)
            .filter(|value| !value.trim().is_empty())
        {
            base_env.insert(key.to_string(), value.trim().to_string());
        }
    }

    let mut service_env = build_openclaw_env(env_meta, &base_env);
    service_env.insert("OPENCLAW_PROFILE".to_string(), String::new());
    service_env.insert(
        "OPENCLAW_SERVICE_LABEL".to_string(),
        launchd_label.to_string(),
    );
    match service_manager_kind(process_env) {
        ServiceManagerKind::Launchd => {
            service_env.insert(
                "OPENCLAW_LAUNCHD_LABEL".to_string(),
                launchd_label.to_string(),
            );
        }
        ServiceManagerKind::SystemdUser => {
            service_env.insert(
                "OPENCLAW_SYSTEMD_UNIT".to_string(),
                launchd_label.to_string(),
            );
        }
    }
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

fn build_systemd_unit(
    _label: &str,
    description: &str,
    program_arguments: &[String],
    working_directory: &Path,
    environment: &BTreeMap<String, String>,
) -> String {
    let exec_start = program_arguments
        .iter()
        .map(|arg| systemd_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let environment_lines = environment
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| {
            format!(
                "Environment=\"{}={}\"",
                systemd_escape(key),
                systemd_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let environment_block = if environment_lines.is_empty() {
        String::new()
    } else {
        format!("{environment_lines}\n")
    };

    format!(
        "[Unit]\nDescription={}\nAfter=network.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={}\n{}Restart=always\nRestartSec=1\nUMask=0077\n\n[Install]\nWantedBy=default.target\n",
        systemd_escape(description),
        systemd_quote(&display_path(working_directory)),
        exec_start,
        environment_block,
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

fn activate_managed_service(
    label: &str,
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => activate_launch_agent(label, service_path),
        ServiceManagerKind::SystemdUser => {
            run_systemctl(["--user", "daemon-reload"])?;
            let enable = run_systemctl(["--user", "enable", "--now", label])?;
            if !enable.status.success() {
                return Err(format!(
                    "systemctl --user enable --now failed: {}",
                    systemctl_detail(&enable)
                ));
            }
            Ok(())
        }
    }
}

fn backup_global_plist(source_path: &Path, backup_path: &Path) -> Result<(), String> {
    let Some(parent) = backup_path.parent() else {
        return Err("failed to resolve backup directory for global service plist".to_string());
    };
    ensure_secure_dir(parent)?;
    fs::copy(source_path, backup_path).map_err(|error| error.to_string())?;
    set_mode(backup_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn restore_global_plist(backup_path: &Path, global_path: &Path) -> Result<(), String> {
    let Some(parent) = global_path.parent() else {
        return Err(
            "failed to resolve LaunchAgents directory for global service restore".to_string(),
        );
    };
    ensure_secure_dir(parent)?;
    fs::copy(backup_path, global_path).map_err(|error| error.to_string())?;
    set_mode(global_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn read_global_service_binding(
    env: &BTreeMap<String, String>,
) -> Result<GlobalServiceBinding, String> {
    let plist_path = global_plist_path(env);
    if !plist_path.exists() {
        return Err("global OpenClaw service is not installed".to_string());
    }

    read_service_binding(&plist_path)
}

fn global_plist_backup_path(env: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf, String> {
    let backup_root = global_plist_backup_dir(env, cwd)?;
    let timestamp = now_utc().unix_timestamp_nanos();
    Ok(backup_root.join(format!("{GLOBAL_GATEWAY_LABEL}.{timestamp}.plist")))
}

fn global_plist_backup_dir(env: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf, String> {
    Ok(resolve_ocm_home(env, cwd)?.join("services").join("backups"))
}

fn latest_matching_global_plist_backup(
    env_meta: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<GlobalServiceBinding, String> {
    let backup_dir = global_plist_backup_dir(env, cwd)?;
    if !backup_dir.exists() {
        return Err(format!(
            "no global service backup exists for env \"{}\"",
            env_meta.name
        ));
    }

    let target_config_path = display_path(&derive_env_paths(Path::new(&env_meta.root)).config_path);
    let mut bindings = Vec::new();
    for entry in fs::read_dir(&backup_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(&format!("{GLOBAL_GATEWAY_LABEL}."))
            || !file_name.ends_with(".plist")
        {
            continue;
        }
        let binding = read_service_binding(&path)?;
        if binding.config_path.as_deref() == Some(target_config_path.as_str()) {
            bindings.push(binding);
        }
    }

    bindings.sort_by(|left, right| {
        left.plist_path
            .file_name()
            .cmp(&right.plist_path.file_name())
    });
    bindings
        .pop()
        .ok_or_else(|| format!("no global service backup matches env \"{}\"", env_meta.name))
}

fn read_service_binding(plist_path: &Path) -> Result<GlobalServiceBinding, String> {
    Ok(GlobalServiceBinding {
        config_path: read_launch_agent_environment_value(plist_path, "OPENCLAW_CONFIG_PATH")?,
        gateway_port: read_launch_agent_environment_value(plist_path, "OPENCLAW_GATEWAY_PORT")?
            .and_then(|value| value.parse::<u32>().ok()),
        plist_path: plist_path.to_path_buf(),
    })
}

fn bootout_global_service() -> Result<(), String> {
    let target = format!("{}/{}", gui_domain(), GLOBAL_GATEWAY_LABEL);
    let bootout = run_launchctl(["bootout", target.as_str()])?;
    if !bootout.status.success() && !launchctl_not_loaded(&bootout) {
        return Err(format!(
            "launchctl bootout failed: {}",
            launchctl_detail(&bootout)
        ));
    }
    Ok(())
}

fn bootout_managed_service(label: &str) -> Result<(), String> {
    let target = format!("{}/{}", gui_domain(), label);
    let bootout = run_launchctl(["bootout", target.as_str()])?;
    if !bootout.status.success() && !launchctl_not_loaded(&bootout) {
        return Err(format!(
            "launchctl bootout failed: {}",
            launchctl_detail(&bootout)
        ));
    }
    Ok(())
}

fn start_managed_service(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
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
            Ok(())
        }
        ServiceManagerKind::SystemdUser => {
            if !prepared.managed_plist_path.exists() {
                return Err(format!(
                    "service for env \"{}\" is not installed; run service install first",
                    prepared.env_meta.name
                ));
            }
            let start = run_systemctl(["--user", "start", prepared.managed_label.as_str()])?;
            if !start.status.success() {
                return Err(format!(
                    "systemctl --user start failed: {}",
                    systemctl_detail(&start)
                ));
            }
            Ok(())
        }
    }
}

fn stop_managed_service(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
            let target = format!("{}/{}", gui_domain(), prepared.managed_label);
            let bootout = run_launchctl(["bootout", target.as_str()])?;
            if !bootout.status.success() && !launchctl_not_loaded(&bootout) {
                return Err(format!(
                    "launchctl bootout failed: {}",
                    launchctl_detail(&bootout)
                ));
            }
            Ok(())
        }
        ServiceManagerKind::SystemdUser => {
            let stop = run_systemctl(["--user", "stop", prepared.managed_label.as_str()])?;
            if !stop.status.success() && !systemctl_not_loaded(&stop) {
                return Err(format!(
                    "systemctl --user stop failed: {}",
                    systemctl_detail(&stop)
                ));
            }
            Ok(())
        }
    }
}

fn restart_managed_service(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
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
            Ok(())
        }
        ServiceManagerKind::SystemdUser => {
            if !prepared.managed_plist_path.exists() {
                return Err(format!(
                    "service for env \"{}\" is not installed; run service install first",
                    prepared.env_meta.name
                ));
            }
            let restart = run_systemctl(["--user", "restart", prepared.managed_label.as_str()])?;
            if !restart.status.success() {
                return Err(format!(
                    "systemctl --user restart failed: {}",
                    systemctl_detail(&restart)
                ));
            }
            Ok(())
        }
    }
}

fn uninstall_managed_service_by_label(
    label: &str,
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
            let target = format!("{}/{}", gui_domain(), label);
            let _ = run_launchctl(["bootout", target.as_str()]);
            Ok(())
        }
        ServiceManagerKind::SystemdUser => {
            if service_path.exists() {
                let _ = run_systemctl(["--user", "disable", "--now", label]);
                let reload = run_systemctl(["--user", "daemon-reload"])?;
                if !reload.status.success() {
                    return Err(format!(
                        "systemctl --user daemon-reload failed: {}",
                        systemctl_detail(&reload)
                    ));
                }
            }
            Ok(())
        }
    }
}

fn read_launch_agent_environment_value(
    plist_path: &Path,
    key: &str,
) -> Result<Option<String>, String> {
    let raw = fs::read_to_string(plist_path).map_err(|error| error.to_string())?;
    let Some(env_section_start) = raw.find("<key>EnvironmentVariables</key>") else {
        return Ok(None);
    };
    let env_section = &raw[env_section_start..];
    let Some(dict_start_offset) = env_section.find("<dict>") else {
        return Ok(None);
    };
    let env_section = &env_section[dict_start_offset + "<dict>".len()..];
    let Some(dict_end_offset) = env_section.find("</dict>") else {
        return Ok(None);
    };
    let env_section = &env_section[..dict_end_offset];
    let key_marker = format!("<key>{key}</key>");
    let Some(key_offset) = env_section.find(&key_marker) else {
        return Ok(None);
    };
    let entry = &env_section[key_offset + key_marker.len()..];
    let Some(string_start_offset) = entry.find("<string>") else {
        return Ok(None);
    };
    let entry = &entry[string_start_offset + "<string>".len()..];
    let Some(string_end_offset) = entry.find("</string>") else {
        return Ok(None);
    };
    Ok(Some(plist_unescape(&entry[..string_end_offset])))
}

fn plist_unescape(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
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

fn run_systemctl<const N: usize>(args: [&str; N]) -> Result<std::process::Output, String> {
    Command::new("systemctl")
        .args(args)
        .output()
        .map_err(|error| format!("failed to run \"systemctl\": {error}"))
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

fn systemctl_detail(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn systemctl_not_loaded(output: &std::process::Output) -> bool {
    let detail = systemctl_detail(output).to_ascii_lowercase();
    detail.contains("not loaded") || detail.contains("not found") || detail.contains("no such file")
}

fn read_systemd_service_logs(label: &str, tail_lines: Option<usize>) -> Result<String, String> {
    let mut command = Command::new("journalctl");
    command.args(["--user", "--unit", label, "--no-pager", "-o", "cat"]);
    if let Some(tail_lines) = tail_lines {
        command.args(["-n", &tail_lines.to_string()]);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to run \"journalctl\": {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "journalctl --user failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn ensure_launchd_only(action: &str, env: &BTreeMap<String, String>) -> Result<(), String> {
    if service_manager_kind(env) == ServiceManagerKind::Launchd {
        return Ok(());
    }
    Err(format!(
        "{action} is currently only supported for launchd-managed machine-wide services"
    ))
}

fn systemd_quote(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\\'))
    {
        format!("\"{}\"", systemd_escape(value))
    } else {
        systemd_escape(value)
    }
}

fn systemd_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{
        build_launch_agent_plist, build_systemd_unit, plist_escape, plist_unescape,
        read_launch_agent_environment_value, service_install_warnings, tail_text,
    };
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

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
            &[
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "openclaw gateway run".to_string(),
            ],
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

    #[test]
    fn systemd_unit_includes_exec_start_and_environment() {
        let mut environment = BTreeMap::new();
        environment.insert("PATH".to_string(), "/usr/bin".to_string());
        environment.insert("OPENCLAW_HOME".to_string(), "/tmp/demo".to_string());
        let unit = build_systemd_unit(
            "ai.openclaw.gateway.ocm.test",
            "test",
            &[
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "openclaw gateway run --port 18789".to_string(),
            ],
            Path::new("/tmp/work"),
            &environment,
        );
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("ExecStart=/bin/sh -lc"));
        assert!(unit.contains("WorkingDirectory=/tmp/work"));
        assert!(unit.contains("Environment=\"OPENCLAW_HOME=/tmp/demo\""));
    }

    #[test]
    fn tail_text_keeps_the_requested_number_of_lines() {
        assert_eq!(tail_text("a\nb\nc\n", 2), "b\nc\n");
        assert_eq!(tail_text("a\nb\nc", 1), "c");
        assert_eq!(tail_text("a\nb\n", 5), "a\nb\n");
        assert_eq!(tail_text("a\nb\n", 0), "");
    }

    #[test]
    fn plist_environment_values_round_trip() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ocm-service-global-plist-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let plist_path = root.join("ai.openclaw.gateway.plist");
        fs::write(
            &plist_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>/tmp/demo/openclaw.json</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>18790</string>
    </dict>
  </dict>
</plist>
"#,
        )
        .unwrap();
        assert_eq!(
            read_launch_agent_environment_value(&plist_path, "OPENCLAW_CONFIG_PATH").unwrap(),
            Some("/tmp/demo/openclaw.json".to_string())
        );
        assert_eq!(
            read_launch_agent_environment_value(&plist_path, "OPENCLAW_GATEWAY_PORT").unwrap(),
            Some("18790".to_string())
        );
        assert_eq!(plist_unescape("&lt;a&amp;b&gt;"), "<a&b>");
        fs::remove_dir_all(&root).unwrap();
    }
}
