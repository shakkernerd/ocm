use std::collections::BTreeMap;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::inspect::ServiceSummary;
use super::service_backend_support_error;
use crate::env::EnvironmentService;
use crate::store::{restore_environment_service_policy, set_environment_service_policy};
use crate::supervisor::{SupervisorService, sync_supervisor_if_present};

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

#[derive(Clone, Copy)]
enum ServiceUpdate {
    Install,
    Start,
    Stop,
    Restart,
    Uninstall,
}

impl ServiceUpdate {
    fn settings(
        self,
    ) -> (
        &'static str,
        Option<bool>,
        Option<bool>,
        bool,
        ServiceSupervisorPolicy,
    ) {
        match self {
            Self::Install => (
                "install",
                Some(true),
                Some(false),
                true,
                ServiceSupervisorPolicy::EnsureRunning,
            ),
            Self::Start => (
                "start",
                Some(true),
                Some(true),
                true,
                ServiceSupervisorPolicy::EnsureRunning,
            ),
            Self::Stop => (
                "stop",
                Some(true),
                Some(false),
                false,
                ServiceSupervisorPolicy::LeaveAsIs,
            ),
            Self::Restart => (
                "restart",
                Some(true),
                Some(true),
                false,
                ServiceSupervisorPolicy::EnsureRunning,
            ),
            Self::Uninstall => (
                "uninstall",
                Some(false),
                Some(false),
                false,
                ServiceSupervisorPolicy::LeaveAsIs,
            ),
        }
    }
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
    update_service(name, ServiceUpdate::Install, env, cwd)
}

pub fn start_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    update_service(name, ServiceUpdate::Start, env, cwd)
}

pub fn stop_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    update_service(name, ServiceUpdate::Stop, env, cwd)
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
        return update_service(name, ServiceUpdate::Restart, env, cwd);
    }

    let before = super::inspect::service_status_fast(name, env, cwd)?;
    if !before.ocm_service_running {
        return update_service(name, ServiceUpdate::Restart, env, cwd);
    }

    let supervisor = SupervisorService::new(env, cwd);
    let mut request_id = supervisor.request_child_restart(name)?;
    let restart_result = wait_for_restart_action_summary(name, before.child_pid, env, cwd);
    match restart_result {
        Ok(mut status) => {
            if !status.observed_restart {
                let (_, recovery_request_id) = supervisor
                    .recover_child_restart_with_request_id(name)
                    .map_err(|error| {
                        format!(
                            "gateway restart was not observed and targeted supervisor recovery failed: {error}"
                        )
                    })?;
                request_id = recovery_request_id;
                status = wait_for_restart_action_summary_with_timeout(
                    name,
                    before.child_pid,
                    Duration::from_secs(5),
                    env,
                    cwd,
                )?;
                if !status.observed_restart {
                    return Err(
                        "gateway restart was not observed after targeted supervisor recovery"
                            .to_string(),
                    );
                }
                status
                    .warnings
                    .push("gateway restart required targeted supervisor recovery".to_string());
            }
            if let Err(clear_error) = supervisor.clear_child_restart_request(name, &request_id) {
                status.warnings.push(format!(
                    "restart completed, but failed to clear restart request: {clear_error}"
                ));
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
    update_service(name, ServiceUpdate::Uninstall, env, cwd)
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

fn ensure_supervisor_running_locked(
    supervisor: &SupervisorService<'_>,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    if let Some(error) = service_backend_support_error(env) {
        return Err(error);
    }
    supervisor.ensure_daemon_running_locked()?;
    Ok(())
}

fn update_service(
    name: &str,
    update: ServiceUpdate,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let (action, service_enabled, service_running, require_binding, supervisor_policy) =
        update.settings();
    if require_binding {
        ensure_gateway_binding(name, env, cwd)?;
    }
    let supervisor = SupervisorService::new(env, cwd);
    // Service policy and the shared daemon are one lifecycle decision. Holding
    // this lock prevents another store or command from racing ownership/teardown.
    let _lifecycle_lock = supervisor.lock_daemon_lifecycle()?;
    supervisor.validate_daemon_owner_locked()?;
    let daemon_before = supervisor.daemon_status()?;
    if let ServiceSupervisorPolicy::EnsureRunning = supervisor_policy {
        if let Err(error) = ensure_supervisor_running_locked(&supervisor, env) {
            return match supervisor.restore_daemon_state_locked(&daemon_before) {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(format!(
                    "{error}; failed to restore the previous daemon state: {rollback_error}"
                )),
            };
        }
    }
    let change =
        match set_environment_service_policy(name, service_enabled, service_running, env, cwd) {
            Ok(change) => change,
            Err(error) => {
                return match supervisor.restore_daemon_state_locked(&daemon_before) {
                    Ok(()) => Err(error),
                    Err(rollback_error) => Err(format!(
                        "{error}; failed to restore the previous daemon state: {rollback_error}"
                    )),
                };
            }
        };
    let update_result = sync_supervisor_if_present(env, cwd).and_then(|_| {
        if matches!(update, ServiceUpdate::Stop | ServiceUpdate::Uninstall)
            && !supervisor.has_desired_running_services()?
        {
            supervisor.uninstall_daemon_locked()?;
        }
        Ok(())
    });
    if let Err(error) = update_result {
        let mut rollback_errors = Vec::new();
        match restore_environment_service_policy(&change, env, cwd) {
            Ok(restored) => {
                if restored && let Err(rollback_error) = sync_supervisor_if_present(env, cwd) {
                    rollback_errors.push(rollback_error);
                }
            }
            Err(rollback_error) => rollback_errors.push(rollback_error),
        }
        if let Err(rollback_error) = supervisor.restore_daemon_state_locked(&daemon_before) {
            rollback_errors.push(rollback_error);
        }
        return if rollback_errors.is_empty() {
            Err(error)
        } else {
            Err(format!(
                "{error}; failed to restore the previous service state: {}",
                rollback_errors.join("; ")
            ))
        };
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
    wait_for_restart_action_summary_with_timeout(
        name,
        previous_pid,
        Duration::from_secs(30),
        env,
        cwd,
    )
}

fn wait_for_restart_action_summary_with_timeout(
    name: &str,
    previous_pid: Option<u32>,
    timeout: Duration,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RestartActionStatus, String> {
    let deadline = Instant::now() + timeout;
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
