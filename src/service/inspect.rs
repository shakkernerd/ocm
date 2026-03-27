use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use crate::env::{EnvMeta, EnvironmentService, resolve_execution_binding};
use crate::launcher::{build_launcher_command, resolve_launcher_run_dir};
use crate::store::{derive_env_paths, display_path, get_environment, get_launcher, get_runtime_verified, list_environments, resolve_user_home};

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
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSummaryList {
    pub global_label: String,
    pub global_installed: bool,
    pub global_loaded: bool,
    pub global_running: bool,
    pub global_pid: Option<u32>,
    pub global_config_path: Option<String>,
    pub services: Vec<ServiceSummary>,
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
    let mut services = Vec::with_capacity(envs.len());
    for meta in envs {
        services.push(build_service_summary(meta, &global, env, cwd)?);
    }
    services.sort_by(|left, right| left.env_name.cmp(&right.env_name));

    Ok(ServiceSummaryList {
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
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
    let global = inspect_job(GLOBAL_GATEWAY_LABEL, &global_plist_path(env));
    build_service_summary(meta, &global, env, cwd)
}

fn build_service_summary(
    meta: EnvMeta,
    global: &LaunchdJobStatus,
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
        issue,
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
    String::from_utf8_lossy(&output.stdout).trim().parse::<u32>().ok()
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

#[cfg(test)]
mod tests {
    use super::{LaunchdJobStatus, managed_service_label, parse_launchctl_print};

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
}
