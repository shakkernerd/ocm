use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde::Serialize;

use super::platform::{ServiceManagerKind, service_manager_kind};
use crate::env::GatewayProcessSpec;
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
    pub ocm_service_installed: bool,
    pub ocm_service_loaded: bool,
    pub ocm_service_running: bool,
    pub ocm_service_pid: Option<u32>,
    pub ocm_service_state: Option<String>,
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
    pub ocm_service_label: String,
    pub ocm_service_installed: bool,
    pub ocm_service_loaded: bool,
    pub ocm_service_running: bool,
    pub ocm_service_pid: Option<u32>,
    pub ocm_service_state: Option<String>,
    pub services: Vec<ServiceSummary>,
}

struct ServiceSnapshot {
    daemon: SupervisorDaemonSummary,
    planned_children: BTreeMap<String, SupervisorChildSpec>,
    skipped_envs: BTreeMap<String, String>,
    runtime_children: BTreeMap<String, SupervisorRuntimeChild>,
}

pub fn list_services(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummaryList, String> {
    let env_service = EnvironmentService::new(env, cwd);
    let mut envs = list_environments(env, cwd)?;
    envs.sort_by(|left, right| left.name.cmp(&right.name));

    let snapshot = load_service_snapshot(env, cwd)?;

    let mut services = Vec::with_capacity(envs.len());
    for meta in envs {
        services.push(build_service_summary(
            &env_service,
            &meta,
            env,
            cwd,
            snapshot.planned_children.get(&meta.name),
            snapshot.skipped_envs.get(&meta.name),
            snapshot.runtime_children.get(&meta.name),
            &snapshot.daemon,
        )?);
    }

    Ok(ServiceSummaryList {
        ocm_service_label: snapshot.daemon.managed_label,
        ocm_service_installed: snapshot.daemon.installed,
        ocm_service_loaded: snapshot.daemon.loaded,
        ocm_service_running: snapshot.daemon.running,
        ocm_service_pid: snapshot.daemon.pid,
        ocm_service_state: snapshot.daemon.state,
        services,
    })
}

pub fn service_status_fast(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummary, String> {
    let env_service = EnvironmentService::new(env, cwd);
    let meta = env_service.get(name)?;
    let snapshot = load_service_snapshot(env, cwd)?;

    build_service_summary(
        &env_service,
        &meta,
        env,
        cwd,
        snapshot.planned_children.get(name),
        snapshot.skipped_envs.get(name),
        snapshot.runtime_children.get(name),
        &snapshot.daemon,
    )
}

fn load_service_snapshot(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSnapshot, String> {
    let supervisor = SupervisorService::new(env, cwd);
    let plan = supervisor.plan()?;
    let daemon = supervisor.daemon_status()?;
    let runtime_children = if daemon.running {
        supervisor
            .runtime()?
            .children
            .into_iter()
            .map(|child| (child.env_name.clone(), child))
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };

    Ok(ServiceSnapshot {
        daemon,
        planned_children: plan
            .children
            .into_iter()
            .map(|child| (child.env_name.clone(), child))
            .collect(),
        skipped_envs: plan
            .skipped_envs
            .into_iter()
            .map(|skipped| (skipped.env_name, skipped.reason))
            .collect(),
        runtime_children,
    })
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
    let resolved_process = resolve_summary_process(env_service, &meta.name, planned_child);
    let resolved_issue = resolved_process
        .as_ref()
        .and_then(|result| result.as_ref().err().cloned());
    let resolved_process = resolved_process.and_then(Result::ok);
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
                    .map(|process| process.binding_kind.clone())
            })
            .or_else(|| binding.as_ref().map(|(kind, _)| kind.clone())),
        binding_name: planned_child
            .map(|child| child.binding_name.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .map(|process| process.binding_name.clone())
            })
            .or_else(|| binding.as_ref().map(|(_, name)| name.clone())),
        command: planned_child
            .and_then(|child| child.command.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .and_then(|process| process.command.clone())
            }),
        binary_path: planned_child
            .and_then(|child| child.binary_path.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .and_then(|process| process.binary_path.clone())
            }),
        runtime_source_kind: planned_child
            .and_then(|child| child.runtime_source_kind.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .and_then(|process| process.runtime_source_kind.clone())
            }),
        runtime_release_version: planned_child
            .and_then(|child| child.runtime_release_version.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .and_then(|process| process.runtime_release_version.clone())
            }),
        runtime_release_channel: planned_child
            .and_then(|child| child.runtime_release_channel.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .and_then(|process| process.runtime_release_channel.clone())
            }),
        args: planned_child
            .map(|child| child.args.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .map(|process| process.args.clone())
            })
            .unwrap_or_default(),
        run_dir: planned_child
            .map(|child| child.run_dir.clone())
            .or_else(|| {
                resolved_process
                    .as_ref()
                    .map(|process| display_path(&process.run_dir))
            })
            .unwrap_or_else(|| meta.root.clone()),
        gateway_port,
        installed,
        loaded,
        running,
        desired_running,
        ocm_service_installed: daemon.installed,
        ocm_service_loaded: daemon.loaded,
        ocm_service_running: daemon.running,
        ocm_service_pid: daemon.pid,
        ocm_service_state: daemon.state.clone(),
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

fn resolve_summary_process(
    env_service: &EnvironmentService<'_>,
    name: &str,
    planned_child: Option<&SupervisorChildSpec>,
) -> Option<Result<GatewayProcessSpec, String>> {
    if planned_child.is_some() {
        return None;
    }
    Some(env_service.resolve_gateway_process(name, true))
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
        return Some("OCM background service is not installed".to_string());
    }
    if desired_running {
        if let Some(reason) = skipped_reason {
            return Some(reason.clone());
        }
        if !daemon.running {
            return Some("OCM background service is not running".to_string());
        }
        if !running {
            return Some("env gateway is not running under the OCM background service".to_string());
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
        if trimmed.starts_with("OPENCLAW_CONFIG_PATH => ") {
            continue;
        }
        if trimmed.starts_with("OPENCLAW_GATEWAY_PORT => ") {}
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
        if line.starts_with("ExecStart=") {
            continue;
        }
        if line.starts_with("WorkingDirectory=") {
            continue;
        }
        if line.starts_with("Environment=") {}
    }

    status.loaded = load_state.as_deref() == Some("loaded")
        || unit_file_state
            .as_deref()
            .is_some_and(|value| !matches!(value, "not-found" | "masked"));
    status.running = active_state.as_deref() == Some("active");
    status.state = sub_state.or(active_state);
}
