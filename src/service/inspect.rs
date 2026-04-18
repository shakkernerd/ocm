use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use super::platform::{ServiceManagerKind, service_manager_kind};
use crate::env::{EnvMeta, EnvironmentService};
use crate::store::{display_path, list_environments, supervisor_logs_dir};
use crate::supervisor::{
    SupervisorChildSpec, SupervisorDaemonSummary, SupervisorRuntimeChild, SupervisorService,
};

fn launchctl_binary(env: &BTreeMap<String, String>) -> String {
    env.get("OCM_INTERNAL_LAUNCHCTL_BIN")
        .cloned()
        .unwrap_or_else(|| "launchctl".to_string())
}

fn systemctl_binary(env: &BTreeMap<String, String>) -> String {
    env.get("OCM_INTERNAL_SYSTEMCTL_BIN")
        .cloned()
        .unwrap_or_else(|| "systemctl".to_string())
}

#[derive(Clone, Debug, Default)]
pub(crate) struct LaunchdJobStatus {
    pub(crate) installed: bool,
    pub(crate) loaded: bool,
    pub(crate) running: bool,
    pub(crate) pid: Option<u32>,
    pub(crate) state: Option<String>,
    pub(crate) config_path: Option<String>,
    pub(crate) state_dir: Option<String>,
    pub(crate) openclaw_home: Option<String>,
    pub(crate) gateway_port: Option<u32>,
    pub(crate) program_arguments: Vec<String>,
    pub(crate) working_directory: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummary {
    pub env_name: String,
    pub service_kind: String,
    pub binding_kind: Option<String>,
    pub binding_name: Option<String>,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub runtime_source_kind: Option<String>,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub args: Vec<String>,
    pub run_dir: String,
    pub gateway_port: u32,
    pub installed: bool,
    pub loaded: bool,
    pub running: bool,
    pub desired_running: bool,
    pub daemon_installed: bool,
    pub daemon_loaded: bool,
    pub daemon_running: bool,
    pub daemon_pid: Option<u32>,
    pub daemon_state: Option<String>,
    pub child_pid: Option<u32>,
    pub child_restart_count: Option<usize>,
    pub child_port: Option<u32>,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummaryList {
    pub daemon_label: String,
    pub daemon_installed: bool,
    pub daemon_loaded: bool,
    pub daemon_running: bool,
    pub daemon_pid: Option<u32>,
    pub daemon_state: Option<String>,
    pub services: Vec<ServiceSummary>,
}

pub fn list_services(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummaryList, String> {
    let env_service = EnvironmentService::new(env, cwd);
    let mut envs = list_environments(env, cwd)?;
    envs.sort_by(|left, right| left.name.cmp(&right.name));

    let supervisor = SupervisorService::new(env, cwd);
    let plan = supervisor.plan()?;
    let runtime = supervisor.runtime()?;
    let daemon = supervisor.daemon_status()?;
    let planned_children = plan
        .children
        .into_iter()
        .map(|child| (child.env_name.clone(), child))
        .collect::<BTreeMap<_, _>>();
    let skipped_envs = plan
        .skipped_envs
        .into_iter()
        .map(|skipped| (skipped.env_name, skipped.reason))
        .collect::<BTreeMap<_, _>>();
    let runtime_children = runtime
        .children
        .into_iter()
        .map(|child| (child.env_name.clone(), child))
        .collect::<BTreeMap<_, _>>();

    let mut services = Vec::with_capacity(envs.len());
    for meta in envs {
        services.push(build_service_summary(
            &env_service,
            &meta,
            env,
            cwd,
            planned_children.get(&meta.name),
            skipped_envs.get(&meta.name),
            runtime_children.get(&meta.name),
            &daemon,
        )?);
    }

    Ok(ServiceSummaryList {
        daemon_label: daemon.managed_label,
        daemon_installed: daemon.installed,
        daemon_loaded: daemon.loaded,
        daemon_running: daemon.running,
        daemon_pid: daemon.pid,
        daemon_state: daemon.state,
        services,
    })
}

pub fn service_status(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummary, String> {
    service_status_fast(name, env, cwd)
}

pub fn service_status_fast(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummary, String> {
    let env_service = EnvironmentService::new(env, cwd);
    let meta = env_service.get(name)?;
    let supervisor = SupervisorService::new(env, cwd);
    let plan = supervisor.plan()?;
    let runtime = supervisor.runtime()?;
    let daemon = supervisor.daemon_status()?;
    let planned_child = plan.children.iter().find(|child| child.env_name == name);
    let skipped_reason = plan
        .skipped_envs
        .iter()
        .find(|skipped| skipped.env_name == name)
        .map(|skipped| &skipped.reason);
    let runtime_child = runtime.children.iter().find(|child| child.env_name == name);

    build_service_summary(
        &env_service,
        &meta,
        env,
        cwd,
        planned_child,
        skipped_reason,
        runtime_child,
        &daemon,
    )
}

fn build_service_summary(
    env_service: &EnvironmentService<'_>,
    meta: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    planned_child: Option<&SupervisorChildSpec>,
    skipped_reason: Option<&String>,
    runtime_child: Option<&SupervisorRuntimeChild>,
    daemon: &SupervisorDaemonSummary,
) -> Result<ServiceSummary, String> {
    let (gateway_port, _) = env_service.resolve_effective_gateway_port(meta)?;
    let resolved_process = env_service.resolve_gateway_process(&meta.name, true);
    let resolved_issue = resolved_process.as_ref().err().cloned();
    let logs_dir = supervisor_logs_dir(env, cwd)?;
    let fallback_stdout = display_path(&logs_dir.join(format!("{}.stdout.log", meta.name)));
    let fallback_stderr = display_path(&logs_dir.join(format!("{}.stderr.log", meta.name)));
    let binding = binding_from_meta(meta);
    let installed = meta.service_enabled;
    let desired_running = meta.service_running;
    let loaded = installed && (daemon.loaded || daemon.running);
    let running = runtime_child.is_some();
    let issue = service_issue(
        installed,
        desired_running,
        daemon,
        running,
        skipped_reason,
        resolved_issue,
    );

    Ok(ServiceSummary {
        env_name: meta.name.clone(),
        service_kind: "gateway".to_string(),
        binding_kind: planned_child
            .map(|child| child.binding_kind.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .map(|process| process.binding_kind.clone())
            })
            .or_else(|| binding.as_ref().map(|(kind, _)| kind.clone())),
        binding_name: planned_child
            .map(|child| child.binding_name.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .map(|process| process.binding_name.clone())
            })
            .or_else(|| binding.as_ref().map(|(_, name)| name.clone())),
        command: planned_child
            .and_then(|child| child.command.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .and_then(|process| process.command.clone())
            }),
        binary_path: planned_child
            .and_then(|child| child.binary_path.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .and_then(|process| process.binary_path.clone())
            }),
        runtime_source_kind: planned_child
            .and_then(|child| child.runtime_source_kind.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .and_then(|process| process.runtime_source_kind.clone())
            }),
        runtime_release_version: planned_child
            .and_then(|child| child.runtime_release_version.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .and_then(|process| process.runtime_release_version.clone())
            }),
        runtime_release_channel: planned_child
            .and_then(|child| child.runtime_release_channel.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .and_then(|process| process.runtime_release_channel.clone())
            }),
        args: planned_child
            .map(|child| child.args.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .map(|process| process.args.clone())
            })
            .unwrap_or_default(),
        run_dir: planned_child
            .map(|child| child.run_dir.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .ok()
                    .map(|process| display_path(&process.run_dir))
            })
            .unwrap_or_else(|| meta.root.clone()),
        gateway_port,
        installed,
        loaded,
        running,
        desired_running,
        daemon_installed: daemon.installed,
        daemon_loaded: daemon.loaded,
        daemon_running: daemon.running,
        daemon_pid: daemon.pid,
        daemon_state: daemon.state.clone(),
        child_pid: runtime_child.map(|child| child.pid),
        child_restart_count: runtime_child.map(|child| child.restart_count),
        child_port: runtime_child.map(|child| child.child_port),
        stdout_path: runtime_child
            .map(|child| child.stdout_path.clone())
            .or_else(|| planned_child.map(|child| child.stdout_path.clone()))
            .or(Some(fallback_stdout)),
        stderr_path: runtime_child
            .map(|child| child.stderr_path.clone())
            .or_else(|| planned_child.map(|child| child.stderr_path.clone()))
            .or(Some(fallback_stderr)),
        issue,
    })
}

fn binding_from_meta(meta: &EnvMeta) -> Option<(String, String)> {
    meta.default_runtime
        .as_ref()
        .map(|name| ("runtime".to_string(), name.clone()))
        .or_else(|| {
            meta.default_launcher
                .as_ref()
                .map(|name| ("launcher".to_string(), name.clone()))
        })
}

fn service_issue(
    installed: bool,
    desired_running: bool,
    daemon: &SupervisorDaemonSummary,
    running: bool,
    skipped_reason: Option<&String>,
    resolved_issue: Option<String>,
) -> Option<String> {
    if let Some(issue) = resolved_issue {
        return Some(issue);
    }
    if !installed {
        return None;
    }
    if !daemon.installed {
        return Some("supervisor daemon is not installed".to_string());
    }
    if desired_running {
        if let Some(reason) = skipped_reason {
            return Some(reason.clone());
        }
        if !daemon.running {
            return Some("supervisor daemon is not running".to_string());
        }
        if !running {
            return Some("env child is not running under the supervisor".to_string());
        }
    }
    None
}

pub(crate) fn inspect_job(
    label: &str,
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> LaunchdJobStatus {
    let mut status = LaunchdJobStatus {
        installed: service_path.exists(),
        ..LaunchdJobStatus::default()
    };

    if status.installed {
        status.config_path =
            read_service_environment_value(service_path, "OPENCLAW_CONFIG_PATH", env)
                .ok()
                .flatten();
        status.state_dir = read_service_environment_value(service_path, "OPENCLAW_STATE_DIR", env)
            .ok()
            .flatten();
        status.openclaw_home = read_service_environment_value(service_path, "OPENCLAW_HOME", env)
            .ok()
            .flatten();
        status.gateway_port =
            read_service_environment_value(service_path, "OPENCLAW_GATEWAY_PORT", env)
                .ok()
                .flatten()
                .and_then(|value| value.parse::<u32>().ok());
    }

    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
            let Some(uid) = current_uid() else {
                return status;
            };
            let target = format!("gui/{uid}/{label}");
            let output = Command::new(launchctl_binary(env))
                .args(["print", &target])
                .output();
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
        ServiceManagerKind::SystemdUser => {
            let output = Command::new(systemctl_binary(env))
                .args([
                    "--user",
                    "show",
                    label,
                    "--property=LoadState,UnitFileState,ActiveState,SubState,MainPID,FragmentPath,ExecStart,WorkingDirectory,Environment",
                ])
                .output();
            let Ok(output) = output else {
                return status;
            };
            if !output.status.success() {
                return status;
            }

            parse_systemctl_show(&String::from_utf8_lossy(&output.stdout), &mut status);
        }
        ServiceManagerKind::Unsupported => {}
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

fn parse_systemctl_show(raw: &str, status: &mut LaunchdJobStatus) {
    let mut load_state = None;
    let mut unit_file_state = None;
    let mut active_state = None;
    let mut sub_state = None;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("LoadState=") {
            load_state = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("UnitFileState=") {
            unit_file_state = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("ActiveState=") {
            active_state = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("SubState=") {
            sub_state = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("MainPID=") {
            status.pid = value.trim().parse::<u32>().ok().filter(|pid| *pid > 0);
            continue;
        }
        if let Some(value) = line.strip_prefix("ExecStart=") {
            status.program_arguments = parse_systemctl_exec_start(value.trim());
            continue;
        }
        if let Some(value) = line.strip_prefix("WorkingDirectory=") {
            let value = value.trim();
            if !value.is_empty() {
                status.working_directory = Some(value.to_string());
            }
            continue;
        }
        if let Some(value) = line.strip_prefix("Environment=") {
            parse_systemctl_environment(value.trim(), status);
        }
    }

    status.loaded = load_state.as_deref() == Some("loaded")
        || unit_file_state
            .as_deref()
            .is_some_and(|value| !matches!(value, "not-found" | "masked"));
    status.running = active_state.as_deref() == Some("active");
    status.state = sub_state.or(active_state);
}

fn parse_systemctl_exec_start(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    if !raw.starts_with('{') {
        return parse_systemd_words(raw).unwrap_or_default();
    }
    let Some(argv_index) = raw.find("argv[]=") else {
        return Vec::new();
    };
    let argv = &raw[argv_index + "argv[]=".len()..];
    let end = argv.find(" ;").unwrap_or(argv.len());
    parse_systemd_words(argv[..end].trim()).unwrap_or_default()
}

fn parse_systemctl_environment(raw: &str, status: &mut LaunchdJobStatus) {
    for entry in parse_systemd_words(raw).unwrap_or_default() {
        let unquoted = systemd_unquote(&entry);
        let Some((key, value)) = unquoted.split_once('=') else {
            continue;
        };
        match key {
            "OPENCLAW_CONFIG_PATH" => status.config_path = Some(value.to_string()),
            "OPENCLAW_STATE_DIR" => status.state_dir = Some(value.to_string()),
            "OPENCLAW_HOME" => status.openclaw_home = Some(value.to_string()),
            "OPENCLAW_GATEWAY_PORT" => {
                status.gateway_port = value.parse::<u32>().ok();
            }
            _ => {}
        }
    }
}

pub(crate) fn read_service_environment_value(
    service_path: &Path,
    key: &str,
    env: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => read_launch_agent_environment_value(service_path, key),
        ServiceManagerKind::SystemdUser => read_systemd_environment_value(service_path, key),
        ServiceManagerKind::Unsupported => Ok(None),
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
    read_plist_string_value_from_section(env_section, &key_marker)
}

fn read_systemd_environment_value(
    service_path: &Path,
    key: &str,
) -> Result<Option<String>, String> {
    for entry in read_systemd_directive_values(service_path, "Environment")? {
        let unquoted = systemd_unquote(&entry);
        if let Some((entry_key, value)) = unquoted.split_once('=') {
            if entry_key == key {
                return Ok(Some(value.to_string()));
            }
        }
    }
    Ok(None)
}

fn read_systemd_directive_values(service_path: &Path, key: &str) -> Result<Vec<String>, String> {
    let raw = fs::read_to_string(service_path).map_err(|error| error.to_string())?;
    let mut values = Vec::new();
    let mut in_service = false;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_service = trimmed.eq_ignore_ascii_case("[Service]");
            continue;
        }
        if !in_service || trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';')
        {
            continue;
        }
        if let Some(value) = trimmed.strip_prefix(&format!("{key}=")) {
            values.push(value.trim().to_string());
        }
    }

    Ok(values)
}

fn parse_systemd_words(raw: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut chars = raw.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '\\' => {
                let Some(next) = chars.next() else {
                    return Err("invalid systemd escape sequence".to_string());
                };
                current.push(next);
            }
            ch if ch.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if in_quotes {
        return Err("unterminated quoted systemd value".to_string());
    }
    if !current.is_empty() {
        words.push(current);
    }
    Ok(words)
}

fn systemd_unquote(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        trimmed[1..trimmed.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\")
    } else {
        trimmed.to_string()
    }
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

fn plist_unescape(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}
