use std::collections::BTreeMap;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::inspect::ServiceSummary;
use super::service_backend_support_error;
use crate::env::EnvironmentService;
use crate::supervisor::SupervisorService;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceActionSummary {
    pub env_name: String,
    pub service_kind: String,
    pub action: String,
    pub installed: bool,
    pub loaded: bool,
    pub running: bool,
    pub desired_running: bool,
    pub gateway_port: u32,
    pub binding_kind: Option<String>,
    pub binding_name: Option<String>,
    pub stdout_path: Option<String>,
    pub stderr_path: Option<String>,
    pub warnings: Vec<String>,
}

pub type ServiceInstallSummary = ServiceActionSummary;

#[derive(Clone, Copy)]
enum ServiceSupervisorPolicy {
    LeaveAsIs,
    EnsureRunning,
}

struct RestartActionStatus {
    summary: ServiceSummary,
    warnings: Vec<String>,
    observed_restart: bool,
}

pub fn install_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceInstallSummary, String> {
    update_service(
        name,
        "install",
        Some(true),
        Some(false),
        true,
        ServiceSupervisorPolicy::EnsureRunning,
        env,
        cwd,
    )
}

pub fn start_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    update_service(
        name,
        "start",
        Some(true),
        Some(true),
        true,
        ServiceSupervisorPolicy::EnsureRunning,
        env,
        cwd,
    )
}

pub fn stop_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    update_service(
        name,
        "stop",
        Some(true),
        Some(false),
        false,
        ServiceSupervisorPolicy::LeaveAsIs,
        env,
        cwd,
    )
}

pub fn restart_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    ensure_gateway_binding(name, env, cwd)?;
    let env_service = EnvironmentService::new(env, cwd);
    let meta = env_service.get(name)?;
    if !(meta.service_enabled && meta.service_running) {
        return update_service(
            name,
            "restart",
            Some(true),
            Some(true),
            false,
            ServiceSupervisorPolicy::EnsureRunning,
            env,
            cwd,
        );
    }

    let before = super::inspect::service_status_fast(name, env, cwd)?;
    if !before.ocm_service_running {
        return update_service(
            name,
            "restart",
            Some(true),
            Some(true),
            false,
            ServiceSupervisorPolicy::EnsureRunning,
            env,
            cwd,
        );
    }

    let supervisor = SupervisorService::new(env, cwd);
    let request_id = supervisor.request_child_restart(name)?;
    let restart_result = wait_for_restart_action_summary(name, before.child_pid, env, cwd);
    match restart_result {
        Ok(mut status) => {
            if status.observed_restart {
                if let Err(clear_error) = supervisor.clear_child_restart_request(name, &request_id)
                {
                    status.warnings.push(format!(
                        "restart completed, but failed to clear restart request: {clear_error}"
                    ));
                }
            }
            Ok(service_action_summary(
                "restart",
                status.summary,
                status.warnings,
            ))
        }
        Err(restart_error) => Err(restart_error),
    }
}

pub fn uninstall_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    update_service(
        name,
        "uninstall",
        Some(false),
        Some(false),
        false,
        ServiceSupervisorPolicy::LeaveAsIs,
        env,
        cwd,
    )
}

fn ensure_gateway_binding(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(), String> {
    EnvironmentService::new(env, cwd)
        .resolve_gateway_process(name, true)
        .map(|_| ())
}

fn ensure_supervisor_running(env: &BTreeMap<String, String>, cwd: &Path) -> Result<(), String> {
    if let Some(error) = service_backend_support_error(env) {
        return Err(error);
    }
    let supervisor = SupervisorService::new(env, cwd);
    supervisor.ensure_daemon_running()?;
    Ok(())
}

fn update_service(
    name: &str,
    action: &str,
    service_enabled: Option<bool>,
    service_running: Option<bool>,
    require_binding: bool,
    supervisor_policy: ServiceSupervisorPolicy,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    if require_binding {
        ensure_gateway_binding(name, env, cwd)?;
    }
    EnvironmentService::new(env, cwd).set_service_policy(name, service_enabled, service_running)?;
    if let ServiceSupervisorPolicy::EnsureRunning = supervisor_policy {
        ensure_supervisor_running(env, cwd)?;
    }
    let (summary, warnings) = wait_for_action_summary(name, action, env, cwd)?;
    Ok(service_action_summary(action, summary, warnings))
}

fn wait_for_action_summary(
    name: &str,
    action: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(ServiceSummary, Vec<String>), String> {
    let should_wait_for_stop = matches!(action, "stop" | "uninstall");
    if !should_wait_for_stop {
        return Ok((
            super::inspect::service_status_fast(name, env, cwd)?,
            Vec::new(),
        ));
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut latest = super::inspect::service_status_fast(name, env, cwd)?;
    while Instant::now() < deadline {
        if !latest.running {
            return Ok((latest, Vec::new()));
        }
        sleep(Duration::from_millis(100));
        latest = super::inspect::service_status_fast(name, env, cwd)?;
    }

    let mut warnings = Vec::new();
    if latest.running {
        warnings.push(
            "gateway is still shutting down; check service status again in a moment".to_string(),
        );
    }
    Ok((latest, warnings))
}

fn wait_for_restart_action_summary(
    name: &str,
    previous_pid: Option<u32>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RestartActionStatus, String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut latest = super::inspect::service_status_fast(name, env, cwd)?;
    while Instant::now() < deadline {
        if latest.running
            && latest
                .child_pid
                .is_some_and(|child_pid| previous_pid != Some(child_pid))
        {
            return Ok(RestartActionStatus {
                summary: latest,
                warnings: Vec::new(),
                observed_restart: true,
            });
        }
        sleep(Duration::from_millis(100));
        latest = super::inspect::service_status_fast(name, env, cwd)?;
    }

    let warning = match previous_pid {
        Some(previous_pid) => {
            format!("gateway restart is still in progress; previous child pid was {previous_pid}")
        }
        None => "gateway restart is still in progress; no replacement child pid has been observed"
            .to_string(),
    };
    Ok(RestartActionStatus {
        summary: latest,
        warnings: vec![warning],
        observed_restart: false,
    })
}

fn service_action_summary(
    action: &str,
    summary: ServiceSummary,
    warnings: Vec<String>,
) -> ServiceActionSummary {
    ServiceActionSummary {
        env_name: summary.env_name,
        service_kind: summary.service_kind,
        action: action.to_string(),
        installed: summary.installed,
        loaded: summary.loaded,
        running: summary.running,
        desired_running: summary.desired_running,
        gateway_port: summary.gateway_port,
        binding_kind: summary.binding_kind,
        binding_name: summary.binding_name,
        stdout_path: summary.stdout_path,
        stderr_path: summary.stderr_path,
        warnings,
    }
}
