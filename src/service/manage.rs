use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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
    let env_service = EnvironmentService::new(env, cwd);
    let meta = env_service.get(name)?;
    if meta.service_enabled && meta.service_running {
        env_service.set_service_policy(name, Some(true), Some(false))?;
    }
    update_service(
        name,
        "restart",
        Some(true),
        Some(true),
        true,
        ServiceSupervisorPolicy::EnsureRunning,
        env,
        cwd,
    )
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

pub fn service_logs(
    name: &str,
    stream: &str,
    tail_lines: Option<usize>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceLogSummary, String> {
    let summary = super::inspect::service_status_fast(name, env, cwd)?;
    let stream = normalize_stream(stream)?;
    let path = match stream {
        "stdout" => summary.stdout_path.clone(),
        "stderr" => summary.stderr_path.clone(),
        _ => unreachable!("normalize_stream validates the log stream"),
    }
    .ok_or_else(|| format!("no {stream} log path is available for env \"{name}\""))?;
    if !Path::new(&path).exists() {
        return Err(format!(
            "{stream} log does not exist for env \"{name}\": {path}"
        ));
    }

    let raw = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let content = match tail_lines {
        Some(limit) => tail_text(&raw, limit),
        None => raw,
    };

    Ok(ServiceLogSummary {
        env_name: name.to_string(),
        service_kind: "gateway".to_string(),
        stream: stream.to_string(),
        path,
        tail_lines,
        content,
    })
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
    action_summary(name, action, Vec::new(), env, cwd)
}

fn action_summary(
    name: &str,
    action: &str,
    warnings: Vec<String>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let summary = super::inspect::service_status_fast(name, env, cwd)?;
    Ok(service_action_summary(action, summary, warnings))
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

fn normalize_stream(stream: &str) -> Result<&str, String> {
    match stream {
        "stdout" | "stderr" => Ok(stream),
        _ => Err(format!("unsupported log stream: {stream}")),
    }
}

fn tail_text(text: &str, tail_lines: usize) -> String {
    if tail_lines == 0 {
        return String::new();
    }
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(tail_lines);
    let mut output = lines[start..].join("\n");
    if text.ends_with('\n') && !output.is_empty() {
        output.push('\n');
    }
    output
}
