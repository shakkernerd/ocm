use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::env::{EnvMeta, EnvironmentService, resolve_execution_binding};
use crate::launcher::{build_launcher_command, resolve_launcher_run_dir};
use crate::store::{
    derive_env_paths, display_path, get_environment, get_launcher, get_runtime_verified,
    list_environments, resolve_ocm_home, resolve_user_home,
};

pub(crate) const GLOBAL_GATEWAY_LABEL: &str = "ai.openclaw.gateway";
pub(crate) const OCM_GATEWAY_LABEL_PREFIX: &str = "ai.openclaw.gateway.ocm.";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub env_name: String,
    pub service_kind: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub global_label: String,
    pub global_env_name: Option<String>,
    pub binding_kind: Option<String>,
    pub binding_name: Option<String>,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub args: Vec<String>,
    pub run_dir: String,
    pub gateway_port: u32,
    pub installed: bool,
    pub loaded: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub state: Option<String>,
    pub global_installed: bool,
    pub global_loaded: bool,
    pub global_running: bool,
    pub global_pid: Option<u32>,
    pub global_matches_env: bool,
    pub global_config_path: Option<String>,
    pub latest_backup_plist_path: Option<String>,
    pub backup_available: bool,
    pub can_adopt_global: bool,
    pub can_restore_global: bool,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummaryList {
    pub global_label: String,
    pub global_env_name: Option<String>,
    pub global_installed: bool,
    pub global_loaded: bool,
    pub global_running: bool,
    pub global_pid: Option<u32>,
    pub global_config_path: Option<String>,
    pub services: Vec<ServiceSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServiceSummary {
    pub label: String,
    pub plist_path: String,
    pub source_kind: String,
    pub installed: bool,
    pub loaded: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub state: Option<String>,
    pub config_path: Option<String>,
    pub state_dir: Option<String>,
    pub openclaw_home: Option<String>,
    pub gateway_port: Option<u32>,
    pub program: Option<String>,
    pub program_arguments: Vec<String>,
    pub working_directory: Option<String>,
    pub matched_env_name: Option<String>,
    pub adoptable: bool,
    pub adopt_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredServiceList {
    pub services: Vec<DiscoveredServiceSummary>,
}

#[derive(Clone, Debug)]
pub(crate) enum ServiceLaunchSpec {
    Launcher {
        binding_name: String,
        command: String,
        run_dir: PathBuf,
    },
    Runtime {
        binding_name: String,
        binary_path: String,
        args: Vec<String>,
        run_dir: PathBuf,
    },
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchdJobStatus {
    pub(crate) installed: bool,
    pub(crate) loaded: bool,
    pub(crate) running: bool,
    pub(crate) pid: Option<u32>,
    pub(crate) state: Option<String>,
    pub(crate) config_path: Option<String>,
    pub(crate) gateway_port: Option<u32>,
}

pub fn list_services(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummaryList, String> {
    let envs = list_environments(env, cwd)?;
    let global = inspect_job(GLOBAL_GATEWAY_LABEL, &global_plist_path(env));
    let global_env_name = matched_env_name_in(&envs, global.config_path.as_deref());
    let mut services = Vec::with_capacity(envs.len());
    for meta in envs {
        services.push(build_service_summary(
            meta,
            &global,
            global_env_name.as_deref(),
            env,
            cwd,
        )?);
    }
    services.sort_by(|left, right| left.env_name.cmp(&right.env_name));

    Ok(ServiceSummaryList {
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_env_name,
        global_installed: global.installed,
        global_loaded: global.loaded,
        global_running: global.running,
        global_pid: global.pid,
        global_config_path: global.config_path.clone(),
        services,
    })
}

pub fn service_status(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummary, String> {
    let meta = get_environment(name, env, cwd)?;
    let envs = list_environments(env, cwd)?;
    let global = inspect_job(GLOBAL_GATEWAY_LABEL, &global_plist_path(env));
    let global_env_name = matched_env_name_in(&envs, global.config_path.as_deref());
    build_service_summary(meta, &global, global_env_name.as_deref(), env, cwd)
}

pub fn discover_services(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<DiscoveredServiceList, String> {
    let envs = list_environments(env, cwd)?;
    let mut env_config_paths = BTreeMap::new();
    for meta in envs {
        let config_path = display_path(&derive_env_paths(Path::new(&meta.root)).config_path);
        env_config_paths.insert(config_path, meta.name);
    }

    let launch_agents_dir = launch_agents_dir(env);
    if !launch_agents_dir.exists() {
        return Ok(DiscoveredServiceList {
            services: Vec::new(),
        });
    }

    let mut services = Vec::new();
    for entry in fs::read_dir(&launch_agents_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let plist_path = entry.path();
        if plist_path.extension().and_then(|value| value.to_str()) != Some("plist") {
            continue;
        }

        let label = read_launch_agent_label(&plist_path)?
            .or_else(|| {
                plist_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| display_path(&plist_path));
        let config_path = read_launch_agent_environment_value(&plist_path, "OPENCLAW_CONFIG_PATH")?;
        let state_dir = read_launch_agent_environment_value(&plist_path, "OPENCLAW_STATE_DIR")?;
        let openclaw_home = read_launch_agent_environment_value(&plist_path, "OPENCLAW_HOME")?;
        let program_arguments = read_plist_array_values(&plist_path, "ProgramArguments")?;
        let program = read_plist_string_value(&plist_path, "Program")?
            .or_else(|| program_arguments.first().cloned());
        let working_directory = read_plist_string_value(&plist_path, "WorkingDirectory")?;
        let gateway_port =
            read_launch_agent_environment_value(&plist_path, "OPENCLAW_GATEWAY_PORT")?
                .and_then(|value| value.parse::<u32>().ok());

        if !looks_like_openclaw_service(
            &label,
            program.as_deref(),
            &program_arguments,
            config_path.as_deref(),
            state_dir.as_deref(),
            openclaw_home.as_deref(),
            gateway_port,
        ) {
            continue;
        }

        let status = inspect_job(&label, &plist_path);
        let config_path = config_path.or(status.config_path.clone());
        let gateway_port = gateway_port.or(status.gateway_port);
        let matched_env_name = config_path
            .as_deref()
            .and_then(|value| env_config_paths.get(value))
            .cloned();
        let source_kind = discovered_source_kind(&label).to_string();
        let (adoptable, adopt_reason) = discover_adoption_state(
            &source_kind,
            matched_env_name.as_deref(),
            config_path.as_deref(),
        );

        services.push(DiscoveredServiceSummary {
            label,
            plist_path: display_path(&plist_path),
            source_kind,
            installed: status.installed,
            loaded: status.loaded,
            running: status.running,
            pid: status.pid,
            state: status.state,
            config_path,
            state_dir,
            openclaw_home,
            gateway_port,
            program,
            program_arguments,
            working_directory,
            matched_env_name,
            adoptable,
            adopt_reason,
        });
    }

    services.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then(left.plist_path.cmp(&right.plist_path))
    });

    Ok(DiscoveredServiceList { services })
}

fn build_service_summary(
    meta: EnvMeta,
    global: &LaunchdJobStatus,
    global_env_name: Option<&str>,
    process_env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummary, String> {
    let service = EnvironmentService::new(process_env, cwd);
    let env_meta = service.apply_effective_gateway_port(meta)?;
    let managed_label = managed_service_label(&env_meta.name);
    let managed_plist_path = managed_plist_path(&env_meta.name, process_env);
    let managed = inspect_job(&managed_label, &managed_plist_path);
    let launch = resolve_service_launch(&env_meta, process_env, cwd);
    let env_config_path = display_path(&derive_env_paths(Path::new(&env_meta.root)).config_path);
    let global_matches_env = global
        .config_path
        .as_deref()
        .map(|value| value == env_config_path)
        .unwrap_or(false);
    let latest_backup_plist_path =
        latest_matching_global_backup_path(&env_config_path, process_env, cwd)?;
    let backup_available = latest_backup_plist_path.is_some();
    let can_adopt_global = global.installed && global_matches_env;
    let can_restore_global = !global.installed && backup_available;

    let (binding_kind, binding_name, command, binary_path, args, run_dir, issue) = match launch {
        Ok(ServiceLaunchSpec::Launcher {
            binding_name,
            command,
            run_dir,
        }) => (
            Some("launcher".to_string()),
            Some(binding_name),
            Some(command),
            None,
            Vec::new(),
            display_path(&run_dir),
            None,
        ),
        Ok(ServiceLaunchSpec::Runtime {
            binding_name,
            binary_path,
            args,
            run_dir,
        }) => (
            Some("runtime".to_string()),
            Some(binding_name),
            None,
            Some(binary_path),
            args,
            display_path(&run_dir),
            None,
        ),
        Err(error) => (
            None,
            None,
            None,
            None,
            Vec::new(),
            display_path(Path::new(&env_meta.root)),
            Some(error),
        ),
    };

    Ok(ServiceSummary {
        env_name: env_meta.name,
        service_kind: "gateway".to_string(),
        managed_label,
        managed_plist_path: display_path(&managed_plist_path),
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_env_name: global_env_name.map(|value| value.to_string()),
        binding_kind,
        binding_name,
        command,
        binary_path,
        args,
        run_dir,
        gateway_port: env_meta.gateway_port.unwrap_or_default(),
        installed: managed.installed,
        loaded: managed.loaded,
        running: managed.running,
        pid: managed.pid,
        state: managed.state,
        global_installed: global.installed,
        global_loaded: global.loaded,
        global_running: global.running,
        global_pid: global.pid,
        global_matches_env,
        global_config_path: global.config_path.clone(),
        latest_backup_plist_path: latest_backup_plist_path
            .as_ref()
            .map(|path| display_path(path)),
        backup_available,
        can_adopt_global,
        can_restore_global,
        issue,
    })
}

fn matched_env_name_in(envs: &[EnvMeta], config_path: Option<&str>) -> Option<String> {
    let config_path = config_path?;
    envs.iter().find_map(|meta| {
        let derived = display_path(&derive_env_paths(Path::new(&meta.root)).config_path);
        (derived == config_path).then(|| meta.name.clone())
    })
}

pub(crate) fn resolve_service_launch(
    env: &EnvMeta,
    process_env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceLaunchSpec, String> {
    let port = env
        .gateway_port
        .ok_or_else(|| format!("failed to resolve gateway port for env \"{}\"", env.name))?;
    let gateway_args = vec![
        "gateway".to_string(),
        "run".to_string(),
        "--port".to_string(),
        port.to_string(),
    ];

    match resolve_execution_binding(env, None, None)? {
        crate::env::ExecutionBinding::Launcher(name) => {
            let launcher = get_launcher(&name, process_env, cwd)?;
            Ok(ServiceLaunchSpec::Launcher {
                binding_name: name,
                command: build_launcher_command(&launcher, &gateway_args),
                run_dir: resolve_launcher_run_dir(&launcher, Path::new(&env.root)),
            })
        }
        crate::env::ExecutionBinding::Runtime(name) => {
            let runtime = get_runtime_verified(&name, process_env, cwd)?;
            Ok(ServiceLaunchSpec::Runtime {
                binding_name: name,
                binary_path: runtime.binary_path,
                args: gateway_args,
                run_dir: Path::new(&env.root).to_path_buf(),
            })
        }
    }
}

pub(crate) fn managed_service_label(name: &str) -> String {
    format!("{OCM_GATEWAY_LABEL_PREFIX}{name}")
}

pub(crate) fn managed_plist_path(name: &str, env: &BTreeMap<String, String>) -> PathBuf {
    launch_agents_dir(env).join(format!("{}.plist", managed_service_label(name)))
}

pub(crate) fn global_plist_path(env: &BTreeMap<String, String>) -> PathBuf {
    launch_agents_dir(env).join(format!("{GLOBAL_GATEWAY_LABEL}.plist"))
}

pub(crate) fn launch_agents_dir(env: &BTreeMap<String, String>) -> PathBuf {
    resolve_user_home(env).join("Library").join("LaunchAgents")
}

pub(crate) fn inspect_job(label: &str, plist_path: &Path) -> LaunchdJobStatus {
    let mut status = LaunchdJobStatus {
        installed: plist_path.exists(),
        ..LaunchdJobStatus::default()
    };

    // Keep inspection scoped to the resolved HOME/plist layout so tests and alternate homes do
    // not accidentally report unrelated host-global launchd state.
    if !status.installed {
        return status;
    }

    status.config_path = read_launch_agent_environment_value(plist_path, "OPENCLAW_CONFIG_PATH")
        .ok()
        .flatten();
    status.gateway_port = read_launch_agent_environment_value(plist_path, "OPENCLAW_GATEWAY_PORT")
        .ok()
        .flatten()
        .and_then(|value| value.parse::<u32>().ok());

    #[cfg(target_os = "macos")]
    {
        let Some(uid) = current_uid() else {
            return status;
        };
        let target = format!("gui/{uid}/{label}");
        let output = Command::new("launchctl").args(["print", &target]).output();
        let Ok(output) = output else {
            return status;
        };
        if !output.status.success() {
            return status;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        status.loaded = true;
        parse_launchctl_print(&text, &mut status);
    }

    status
}

pub(crate) fn current_uid() -> Option<u32> {
    let output = Command::new("id").arg("-u").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .ok()
}

fn parse_launchctl_print(raw: &str, status: &mut LaunchdJobStatus) {
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(value) = trimmed.strip_prefix("state = ") {
            let value = value.trim().to_string();
            status.running = value == "running";
            status.state = Some(value);
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("pid = ") {
            status.pid = value.trim().parse::<u32>().ok();
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("OPENCLAW_CONFIG_PATH => ") {
            status.config_path = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("OPENCLAW_GATEWAY_PORT => ") {
            status.gateway_port = value.trim().parse::<u32>().ok();
        }
    }
}

fn latest_matching_global_backup_path(
    env_config_path: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Option<PathBuf>, String> {
    let backup_dir = resolve_ocm_home(env, cwd)?.join("services").join("backups");
    if !backup_dir.exists() {
        return Ok(None);
    }

    let mut matches = Vec::new();
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
        if read_launch_agent_environment_value(&path, "OPENCLAW_CONFIG_PATH")?.as_deref()
            == Some(env_config_path)
        {
            matches.push(path);
        }
    }

    matches.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    Ok(matches.pop())
}

fn looks_like_openclaw_service(
    label: &str,
    program: Option<&str>,
    program_arguments: &[String],
    config_path: Option<&str>,
    state_dir: Option<&str>,
    openclaw_home: Option<&str>,
    gateway_port: Option<u32>,
) -> bool {
    string_mentions_openclaw(label)
        || program.is_some_and(string_mentions_openclaw)
        || program_arguments
            .iter()
            .any(|value| string_mentions_openclaw(value))
        || config_path.is_some()
        || state_dir.is_some()
        || openclaw_home.is_some()
        || gateway_port.is_some()
}

fn string_mentions_openclaw(value: &str) -> bool {
    value.to_ascii_lowercase().contains("openclaw")
}

fn discovered_source_kind(label: &str) -> &'static str {
    if label.starts_with(OCM_GATEWAY_LABEL_PREFIX) {
        "ocm-managed"
    } else if label == GLOBAL_GATEWAY_LABEL {
        "openclaw-global"
    } else {
        "foreign"
    }
}

fn discover_adoption_state(
    source_kind: &str,
    matched_env_name: Option<&str>,
    config_path: Option<&str>,
) -> (bool, Option<String>) {
    match source_kind {
        "ocm-managed" => (false, Some("already managed by ocm".to_string())),
        "openclaw-global" => {
            if let Some(env_name) = matched_env_name {
                (
                    true,
                    Some(format!(
                        "ready to adopt into env \"{env_name}\" with service adopt-global"
                    )),
                )
            } else if config_path.is_some() {
                (
                    false,
                    Some(
                        "create or import a matching env before adopting this global service"
                            .to_string(),
                    ),
                )
            } else {
                (
                    false,
                    Some(
                        "cannot map this global service to an env because it has no OPENCLAW_CONFIG_PATH"
                            .to_string(),
                    ),
                )
            }
        }
        _ => (
            false,
            Some("foreign OpenClaw services are discoverable but not adoptable yet".to_string()),
        ),
    }
}

fn read_launch_agent_label(plist_path: &Path) -> Result<Option<String>, String> {
    read_plist_string_value(plist_path, "Label")
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
    read_plist_string_value_from_section(env_section, &key_marker)
}

fn read_plist_string_value(plist_path: &Path, key: &str) -> Result<Option<String>, String> {
    let raw = fs::read_to_string(plist_path).map_err(|error| error.to_string())?;
    let key_marker = format!("<key>{key}</key>");
    read_plist_string_value_from_section(&raw, &key_marker)
}

fn read_plist_array_values(plist_path: &Path, key: &str) -> Result<Vec<String>, String> {
    let raw = fs::read_to_string(plist_path).map_err(|error| error.to_string())?;
    let key_marker = format!("<key>{key}</key>");
    read_plist_array_values_from_section(&raw, &key_marker)
}

fn read_plist_string_value_from_section(
    section: &str,
    key_marker: &str,
) -> Result<Option<String>, String> {
    let Some(key_offset) = section.find(key_marker) else {
        return Ok(None);
    };
    let entry = &section[key_offset + key_marker.len()..];
    let Some(string_start_offset) = entry.find("<string>") else {
        return Ok(None);
    };
    let entry = &entry[string_start_offset + "<string>".len()..];
    let Some(string_end_offset) = entry.find("</string>") else {
        return Ok(None);
    };
    Ok(Some(plist_unescape(&entry[..string_end_offset])))
}

fn read_plist_array_values_from_section(
    section: &str,
    key_marker: &str,
) -> Result<Vec<String>, String> {
    let Some(key_offset) = section.find(key_marker) else {
        return Ok(Vec::new());
    };
    let entry = &section[key_offset + key_marker.len()..];
    let Some(array_start_offset) = entry.find("<array>") else {
        return Ok(Vec::new());
    };
    let entry = &entry[array_start_offset + "<array>".len()..];
    let Some(array_end_offset) = entry.find("</array>") else {
        return Ok(Vec::new());
    };
    let mut array_section = &entry[..array_end_offset];
    let mut values = Vec::new();
    while let Some(string_start_offset) = array_section.find("<string>") {
        let string_section = &array_section[string_start_offset + "<string>".len()..];
        let Some(string_end_offset) = string_section.find("</string>") else {
            break;
        };
        values.push(plist_unescape(&string_section[..string_end_offset]));
        array_section = &string_section[string_end_offset + "</string>".len()..];
    }
    Ok(values)
}

fn plist_unescape(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::{
        LaunchdJobStatus, discover_adoption_state, discovered_source_kind,
        looks_like_openclaw_service, managed_service_label, parse_launchctl_print,
        read_plist_array_values_from_section, string_mentions_openclaw,
    };

    #[test]
    fn managed_service_labels_are_env_scoped() {
        assert_eq!(
            managed_service_label("demo"),
            "ai.openclaw.gateway.ocm.demo"
        );
    }

    #[test]
    fn parse_launchctl_print_extracts_core_fields() {
        let mut status = LaunchdJobStatus::default();
        parse_launchctl_print(
            r#"
state = running
pid = 23613
environment = {
  OPENCLAW_GATEWAY_PORT => 18790
  OPENCLAW_CONFIG_PATH => /Users/example/.ocm/envs/test/.openclaw/openclaw.json
}
"#,
            &mut status,
        );

        assert!(status.running);
        assert_eq!(status.state.as_deref(), Some("running"));
        assert_eq!(status.pid, Some(23613));
        assert_eq!(status.gateway_port, Some(18790));
        assert_eq!(
            status.config_path.as_deref(),
            Some("/Users/example/.ocm/envs/test/.openclaw/openclaw.json")
        );
    }

    #[test]
    fn discover_classification_is_stable() {
        assert_eq!(
            discovered_source_kind("ai.openclaw.gateway.ocm.demo"),
            "ocm-managed"
        );
        assert_eq!(
            discovered_source_kind("ai.openclaw.gateway"),
            "openclaw-global"
        );
        assert_eq!(
            discovered_source_kind("com.example.openclaw.staging"),
            "foreign"
        );
    }

    #[test]
    fn discover_identifies_openclaw_services_from_label_or_env_vars() {
        assert!(looks_like_openclaw_service(
            "com.example.openclaw",
            None,
            &[],
            None,
            None,
            None,
            None
        ));
        assert!(looks_like_openclaw_service(
            "com.example.something",
            Some("/usr/local/bin/openclaw"),
            &[],
            Some("/tmp/openclaw.json"),
            None,
            None,
            None,
        ));
        assert!(looks_like_openclaw_service(
            "com.example.something",
            Some("/bin/sh"),
            &["openclaw gateway run".to_string()],
            None,
            None,
            None,
            None,
        ));
        assert!(!looks_like_openclaw_service(
            "com.example.something",
            Some("/bin/sh"),
            &["echo hello".to_string()],
            None,
            None,
            None,
            None,
        ));
    }

    #[test]
    fn discover_adoption_state_is_explicit() {
        let (adoptable, reason) =
            discover_adoption_state("openclaw-global", Some("demo"), Some("/tmp/openclaw.json"));
        assert!(adoptable);
        assert_eq!(
            reason.as_deref(),
            Some("ready to adopt into env \"demo\" with service adopt-global")
        );

        let (adoptable, reason) =
            discover_adoption_state("foreign", Some("demo"), Some("/tmp/openclaw.json"));
        assert!(!adoptable);
        assert_eq!(
            reason.as_deref(),
            Some("foreign OpenClaw services are discoverable but not adoptable yet")
        );
    }

    #[test]
    fn string_matching_for_openclaw_is_case_insensitive() {
        assert!(string_mentions_openclaw("/Users/example/OpenClaw"));
    }

    #[test]
    fn plist_array_values_round_trip() {
        let values = read_plist_array_values_from_section(
            r#"
<key>ProgramArguments</key>
<array>
  <string>/bin/sh</string>
  <string>-lc</string>
  <string>openclaw gateway run</string>
</array>
"#,
            "<key>ProgramArguments</key>",
        )
        .unwrap();
        assert_eq!(
            values,
            vec![
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "openclaw gateway run".to_string(),
            ]
        );
    }
}
