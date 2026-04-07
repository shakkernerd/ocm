use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;

use super::platform::{
    OCM_GATEWAY_LABEL_PREFIX, ServiceManagerKind, global_service_definition_path,
    managed_service_identity, service_definition_dir, service_definition_extension,
    service_manager_kind,
};
use crate::env::{EnvMeta, EnvironmentService, resolve_execution_binding};
use crate::infra::shell::{build_openclaw_env, quote_posix};
use crate::launcher::{build_launcher_command, resolve_launcher_run_dir};
use crate::runtime::resolve_runtime_launch;
use crate::store::{
    derive_env_paths, display_path, get_environment, get_launcher, get_runtime_verified,
    list_environments, resolve_ocm_home,
};

pub(crate) const GLOBAL_GATEWAY_LABEL: &str = "ai.openclaw.gateway";

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
    pub desired_gateway_port: Option<u32>,
    pub installed_gateway_port: Option<u32>,
    pub openclaw_state: String,
    pub openclaw_detail: Option<String>,
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
    pub definition_drift: bool,
    pub live_exec_unverified: bool,
    pub orphaned_live_service: bool,
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
    pub openclaw_state: String,
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
    pub(crate) state_dir: Option<String>,
    pub(crate) openclaw_home: Option<String>,
    pub(crate) gateway_port: Option<u32>,
    pub(crate) program_arguments: Vec<String>,
    pub(crate) working_directory: Option<String>,
}

const GATEWAY_PROBE_TIMEOUT_MS: u64 = 120;
const GATEWAY_HTTP_PROBE_TIMEOUT_MS: u64 = 350;
const GATEWAY_HEALTH_COMMAND_TIMEOUT_MS: u64 = 1500;
const GATEWAY_HEALTH_PROCESS_TIMEOUT_MS: u64 = 8000;
const GATEWAY_HEALTH_PROCESS_POLL_MS: u64 = 25;

#[derive(Clone, Copy, Debug)]
enum ServiceProbeDepth {
    Fast,
    Deep,
}

#[derive(Clone, Debug)]
struct OpenClawProbeResult {
    state: String,
    detail: Option<String>,
}

#[derive(Clone, Debug)]
enum OpenClawProbeSpec {
    Shell {
        command: String,
        run_dir: PathBuf,
    },
    Direct {
        command: String,
        args: Vec<String>,
        run_dir: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServiceExecutionDetails {
    command: Option<String>,
    binary_path: Option<String>,
    args: Vec<String>,
    program_arguments: Vec<String>,
    run_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServiceEnvironmentDetails {
    config_path: Option<String>,
    state_dir: Option<String>,
    openclaw_home: Option<String>,
    gateway_port: Option<u32>,
}

#[derive(Clone, Debug)]
enum HealthzProbeResult {
    Gateway,
    Unavailable(String),
    WrongService(String),
}

#[derive(Clone, Debug)]
enum OpenClawHealthCommandProbe {
    Healthy,
    AuthRequired(String),
    RespondingButInvalid(String),
}

pub fn list_services(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummaryList, String> {
    let envs = list_environments(env, cwd)?;
    let global = inspect_job(GLOBAL_GATEWAY_LABEL, &global_plist_path(env), env);
    let global_env_name = matched_env_name_in(&envs, global.config_path.as_deref());
    let mut services = Vec::with_capacity(envs.len());
    for meta in envs {
        services.push(build_service_summary(
            meta,
            &global,
            global_env_name.as_deref(),
            env,
            cwd,
            ServiceProbeDepth::Fast,
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
    service_status_with_depth(name, env, cwd, ServiceProbeDepth::Deep)
}

pub fn service_status_fast(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceSummary, String> {
    service_status_with_depth(name, env, cwd, ServiceProbeDepth::Fast)
}

fn service_status_with_depth(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    depth: ServiceProbeDepth,
) -> Result<ServiceSummary, String> {
    let meta = get_environment(name, env, cwd)?;
    let envs = list_environments(env, cwd)?;
    let global = inspect_job(GLOBAL_GATEWAY_LABEL, &global_plist_path(env), env);
    let global_env_name = matched_env_name_in(&envs, global.config_path.as_deref());
    build_service_summary(meta, &global, global_env_name.as_deref(), env, cwd, depth)
}

pub fn discover_services(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<DiscoveredServiceList, String> {
    let envs = list_environments(env, cwd)?;
    let mut env_config_paths = BTreeMap::new();
    for meta in &envs {
        let config_path = display_path(&derive_env_paths(Path::new(&meta.root)).config_path);
        env_config_paths.insert(config_path, meta.name.clone());
    }

    let launch_agents_dir = launch_agents_dir(env);
    let mut services = Vec::new();
    let mut seen_labels = BTreeSet::new();
    if launch_agents_dir.exists() {
        for entry in fs::read_dir(&launch_agents_dir).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let plist_path = entry.path();
            if plist_path.extension().and_then(|value| value.to_str())
                != Some(service_definition_extension(service_manager_kind(env)))
            {
                continue;
            }

            let label = read_service_label(&plist_path, env)?
                .or_else(|| {
                    plist_path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .map(|value| value.to_string())
                })
                .unwrap_or_else(|| display_path(&plist_path));
            let status = inspect_job(&label, &plist_path, env);
            if let Some(summary) =
                build_discovered_service_summary(label, plist_path, status, &env_config_paths, env)?
            {
                seen_labels.insert(summary.label.clone());
                services.push(summary);
            }
        }
    }

    for meta in &envs {
        let identity = managed_service_identity(&meta.name, env, cwd)?;
        if seen_labels.contains(&identity.label) {
            continue;
        }
        let status = inspect_job(&identity.label, &identity.definition_path, env);
        if !(status.loaded || status.running) {
            continue;
        }
        if let Some(summary) = build_discovered_service_summary(
            identity.label.clone(),
            identity.definition_path.clone(),
            status,
            &env_config_paths,
            env,
        )? {
            seen_labels.insert(identity.label);
            services.push(summary);
        }
    }

    let global_path = global_service_definition_path(env);
    if !seen_labels.contains(GLOBAL_GATEWAY_LABEL) {
        let status = inspect_job(GLOBAL_GATEWAY_LABEL, &global_path, env);
        if status.loaded || status.running {
            if let Some(summary) = build_discovered_service_summary(
                GLOBAL_GATEWAY_LABEL.to_string(),
                global_path,
                status,
                &env_config_paths,
                env,
            )? {
                services.push(summary);
            }
        }
    }

    services.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then(left.plist_path.cmp(&right.plist_path))
    });

    Ok(DiscoveredServiceList { services })
}

fn build_discovered_service_summary(
    label: String,
    plist_path: PathBuf,
    status: LaunchdJobStatus,
    env_config_paths: &BTreeMap<String, String>,
    env: &BTreeMap<String, String>,
) -> Result<Option<DiscoveredServiceSummary>, String> {
    let definition_exists = plist_path.exists();
    let config_path = status.config_path.clone().or(if definition_exists {
        read_service_environment_value(&plist_path, "OPENCLAW_CONFIG_PATH", env)?
    } else {
        None
    });
    let state_dir = status.state_dir.clone().or(if definition_exists {
        read_service_environment_value(&plist_path, "OPENCLAW_STATE_DIR", env)?
    } else {
        None
    });
    let openclaw_home = status.openclaw_home.clone().or(if definition_exists {
        read_service_environment_value(&plist_path, "OPENCLAW_HOME", env)?
    } else {
        None
    });
    let program_arguments = if status.program_arguments.is_empty() {
        if definition_exists {
            read_service_program_arguments(&plist_path, env)?
        } else {
            Vec::new()
        }
    } else {
        status.program_arguments.clone()
    };
    let program = if definition_exists {
        read_service_program(&plist_path, env)?
    } else {
        None
    }
    .or_else(|| program_arguments.first().cloned());
    let working_directory = status.working_directory.clone().or(if definition_exists {
        read_service_working_directory(&plist_path, env)?
    } else {
        None
    });
    let gateway_port = status.gateway_port.or(if definition_exists {
        read_service_environment_value(&plist_path, "OPENCLAW_GATEWAY_PORT", env)?
            .and_then(|value| value.parse::<u32>().ok())
    } else {
        None
    });

    if !looks_like_openclaw_service(
        &label,
        program.as_deref(),
        &program_arguments,
        config_path.as_deref(),
        state_dir.as_deref(),
        openclaw_home.as_deref(),
        gateway_port,
    ) {
        return Ok(None);
    }

    let matched_env_name = config_path
        .as_deref()
        .and_then(|value| env_config_paths.get(value))
        .cloned();
    let source_kind = discovered_source_kind(&label).to_string();
    let (adoptable, adopt_reason) = discover_adoption_state(
        &source_kind,
        matched_env_name.as_deref(),
        config_path.as_deref(),
        env,
    );

    Ok(Some(DiscoveredServiceSummary {
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
        openclaw_state: detect_openclaw_state_fast(
            gateway_port,
            status.installed,
            status.loaded,
            status.running,
        )
        .state,
        program,
        program_arguments,
        working_directory,
        matched_env_name,
        adoptable,
        adopt_reason,
    }))
}

fn build_service_summary(
    meta: EnvMeta,
    global: &LaunchdJobStatus,
    global_env_name: Option<&str>,
    process_env: &BTreeMap<String, String>,
    cwd: &Path,
    depth: ServiceProbeDepth,
) -> Result<ServiceSummary, String> {
    let service = EnvironmentService::new(process_env, cwd);
    let env_meta = service.apply_effective_gateway_port(meta)?;
    let env_paths = derive_env_paths(Path::new(&env_meta.root));
    let managed = managed_service_identity(&env_meta.name, process_env, cwd)?;
    let managed_status = inspect_job(&managed.label, &managed.definition_path, process_env);
    let launch = resolve_service_launch(&env_meta, process_env, cwd, false);
    let env_config_path = display_path(&env_paths.config_path);
    let expected_service_env = ServiceEnvironmentDetails {
        config_path: Some(env_config_path.clone()),
        state_dir: Some(display_path(&env_paths.state_dir)),
        openclaw_home: Some(display_path(&env_paths.openclaw_home)),
        gateway_port: env_meta.gateway_port,
    };
    let global_matches_env = global
        .config_path
        .as_deref()
        .map(|value| value == env_config_path)
        .unwrap_or(false);
    let latest_backup_plist_path =
        if service_manager_kind(process_env) == ServiceManagerKind::Launchd {
            latest_matching_global_backup_path(&env_config_path, process_env, cwd)?
        } else {
            None
        };
    let backup_available = latest_backup_plist_path.is_some();
    let can_adopt_global = service_manager_kind(process_env) == ServiceManagerKind::Launchd
        && global.installed
        && global_matches_env;
    let can_restore_global = service_manager_kind(process_env) == ServiceManagerKind::Launchd
        && !global.installed
        && backup_available;

    let (binding_kind, binding_name, expected_exec, mut issue) = match launch {
        Ok(launch) => {
            let binding_kind = match &launch {
                ServiceLaunchSpec::Launcher { .. } => Some("launcher".to_string()),
                ServiceLaunchSpec::Runtime { .. } => Some("runtime".to_string()),
            };
            let binding_name = match &launch {
                ServiceLaunchSpec::Launcher { binding_name, .. }
                | ServiceLaunchSpec::Runtime { binding_name, .. } => Some(binding_name.clone()),
            };
            (
                binding_kind,
                binding_name,
                Some(service_execution_from_launch_spec(launch)),
                None,
            )
        }
        Err(error) => (None, None, None, Some(error)),
    };
    let live_service = managed_status.loaded || managed_status.running;
    let live_exec = service_execution_from_status(&managed_status, Path::new(&env_meta.root));
    let launchd_live_exec_unverified = service_manager_kind(process_env)
        == ServiceManagerKind::Launchd
        && live_service
        && live_exec.is_none();
    let orphaned_live_service = live_service && !managed_status.installed;
    let installed_exec = if managed_status.installed || live_service {
        if live_exec.is_some() {
            live_exec
        } else if managed_status.installed {
            read_service_execution(
                &managed.definition_path,
                process_env,
                Path::new(&env_meta.root),
            )?
        } else {
            None
        }
    } else {
        None
    };
    let installed_service_env = if managed_status.installed || live_service {
        Some(ServiceEnvironmentDetails {
            config_path: managed_status.config_path.clone(),
            state_dir: managed_status.state_dir.clone(),
            openclaw_home: managed_status.openclaw_home.clone(),
            gateway_port: managed_status.gateway_port,
        })
    } else {
        None
    };
    let actual_gateway_port = managed_status.gateway_port.or(env_meta.gateway_port);
    let definition_drift = installed_exec
        .as_ref()
        .zip(expected_exec.as_ref())
        .is_some_and(|(installed, expected)| {
            installed.program_arguments != expected.program_arguments
                || installed.run_dir != expected.run_dir
        })
        || installed_service_env
            .as_ref()
            .is_some_and(|installed| installed != &expected_service_env);
    if definition_drift {
        let refresh_action = if managed_status.loaded || managed_status.running {
            "service restart"
        } else {
            "service start"
        };
        let detail = format!(
            "installed service definition does not match the current env binding; run {refresh_action} {} to refresh it",
            env_meta.name
        );
        issue = Some(match issue {
            Some(existing) => format!("{existing}; {detail}"),
            None => detail,
        });
    } else if launchd_live_exec_unverified {
        let detail = format!(
            "launchd does not expose live command details for loaded services; run service restart {} to fully verify the current binding",
            env_meta.name
        );
        issue = Some(match issue {
            Some(existing) => format!("{existing}; {detail}"),
            None => detail,
        });
    } else if orphaned_live_service {
        let detail = format!(
            "managed service definition is missing while the service is still loaded; run service restart {} to rewrite it",
            env_meta.name
        );
        issue = Some(match issue {
            Some(existing) => format!("{existing}; {detail}"),
            None => detail,
        });
    }
    let display_exec = match installed_exec.as_ref() {
        Some(exec) => Some(exec),
        None if live_service && !managed_status.installed => None,
        None => expected_exec.as_ref(),
    };
    let (command, binary_path, args, run_dir) = match display_exec {
        Some(exec) => (
            exec.command.clone(),
            exec.binary_path.clone(),
            exec.args.clone(),
            display_path(&exec.run_dir),
        ),
        None => (
            None,
            None,
            Vec::new(),
            display_path(Path::new(&env_meta.root)),
        ),
    };
    let probe_spec = if definition_drift || launchd_live_exec_unverified {
        None
    } else {
        display_exec.and_then(|execution| {
            service_execution_health_probe_spec(execution, actual_gateway_port)
        })
    };

    let openclaw_probe = match depth {
        ServiceProbeDepth::Fast => detect_openclaw_state_fast(
            actual_gateway_port,
            managed_status.installed,
            managed_status.loaded,
            managed_status.running,
        ),
        ServiceProbeDepth::Deep => detect_openclaw_state_detailed(
            &env_meta,
            actual_gateway_port,
            managed_status.installed,
            managed_status.loaded,
            managed_status.running,
            probe_spec.as_ref(),
            process_env,
        ),
    };

    Ok(ServiceSummary {
        env_name: env_meta.name,
        service_kind: "gateway".to_string(),
        managed_label: managed.label,
        managed_plist_path: display_path(&managed.definition_path),
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_env_name: global_env_name.map(|value| value.to_string()),
        binding_kind,
        binding_name,
        command,
        binary_path,
        args,
        run_dir,
        gateway_port: actual_gateway_port.unwrap_or_default(),
        desired_gateway_port: env_meta.gateway_port,
        installed_gateway_port: managed_status.gateway_port,
        openclaw_state: openclaw_probe.state,
        openclaw_detail: openclaw_probe.detail,
        installed: managed_status.installed,
        loaded: managed_status.loaded,
        running: managed_status.running,
        pid: managed_status.pid,
        state: managed_status.state,
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
        definition_drift,
        live_exec_unverified: launchd_live_exec_unverified,
        orphaned_live_service,
        issue,
    })
}

fn service_execution_from_launch_spec(launch: ServiceLaunchSpec) -> ServiceExecutionDetails {
    match launch {
        ServiceLaunchSpec::Launcher {
            command, run_dir, ..
        } => ServiceExecutionDetails {
            command: Some(command.clone()),
            binary_path: None,
            args: Vec::new(),
            program_arguments: vec!["/bin/sh".to_string(), "-lc".to_string(), command],
            run_dir,
        },
        ServiceLaunchSpec::Runtime {
            binary_path,
            args,
            run_dir,
            ..
        } => {
            let mut program_arguments = vec![binary_path.clone()];
            program_arguments.extend(args.iter().cloned());
            ServiceExecutionDetails {
                command: None,
                binary_path: Some(binary_path),
                args,
                program_arguments,
                run_dir,
            }
        }
    }
}

fn read_service_execution(
    service_path: &Path,
    env: &BTreeMap<String, String>,
    fallback_run_dir: &Path,
) -> Result<Option<ServiceExecutionDetails>, String> {
    if !service_path.exists() {
        return Ok(None);
    }

    let program_arguments = read_service_program_arguments(service_path, env)?;
    if program_arguments.is_empty() {
        return Ok(None);
    }

    let run_dir = read_service_working_directory(service_path, env)?
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback_run_dir.to_path_buf());

    if program_arguments.len() == 3
        && program_arguments[0] == "/bin/sh"
        && program_arguments[1] == "-lc"
    {
        return Ok(Some(ServiceExecutionDetails {
            command: Some(program_arguments[2].clone()),
            binary_path: None,
            args: Vec::new(),
            program_arguments,
            run_dir,
        }));
    }

    let binary_path = program_arguments.first().cloned();
    let args = program_arguments
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();

    Ok(Some(ServiceExecutionDetails {
        command: None,
        binary_path,
        args,
        program_arguments,
        run_dir,
    }))
}

fn service_execution_from_status(
    status: &LaunchdJobStatus,
    fallback_run_dir: &Path,
) -> Option<ServiceExecutionDetails> {
    if status.program_arguments.is_empty() {
        return None;
    }

    let run_dir = status
        .working_directory
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback_run_dir.to_path_buf());
    let program_arguments = status.program_arguments.clone();

    if program_arguments.len() == 3
        && program_arguments[0] == "/bin/sh"
        && program_arguments[1] == "-lc"
    {
        return Some(ServiceExecutionDetails {
            command: Some(program_arguments[2].clone()),
            binary_path: None,
            args: Vec::new(),
            program_arguments,
            run_dir,
        });
    }

    let binary_path = program_arguments.first().cloned();
    let args = program_arguments
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>();

    Some(ServiceExecutionDetails {
        command: None,
        binary_path,
        args,
        program_arguments,
        run_dir,
    })
}

fn service_execution_health_probe_spec(
    execution: &ServiceExecutionDetails,
    gateway_port: Option<u32>,
) -> Option<OpenClawProbeSpec> {
    let gateway_args = gateway_run_args(gateway_port?);
    let health_args = health_probe_args();

    if let Some(command) = execution.command.as_ref() {
        let gateway_suffix = format!(
            " {}",
            gateway_args
                .iter()
                .map(|arg| quote_posix(arg))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let base = command.strip_suffix(&gateway_suffix)?;
        let health_suffix = health_args
            .iter()
            .map(|arg| quote_posix(arg))
            .collect::<Vec<_>>()
            .join(" ");
        return Some(OpenClawProbeSpec::Shell {
            command: format!("{base} {health_suffix}"),
            run_dir: execution.run_dir.clone(),
        });
    }

    if execution.args != gateway_args {
        return None;
    }

    Some(OpenClawProbeSpec::Direct {
        command: execution.binary_path.clone()?,
        args: health_args,
        run_dir: execution.run_dir.clone(),
    })
}

fn gateway_run_args(port: u32) -> Vec<String> {
    vec![
        "gateway".to_string(),
        "run".to_string(),
        "--port".to_string(),
        port.to_string(),
    ]
}

fn health_probe_args() -> Vec<String> {
    vec![
        "health".to_string(),
        "--json".to_string(),
        "--timeout".to_string(),
        GATEWAY_HEALTH_COMMAND_TIMEOUT_MS.to_string(),
    ]
}

fn detect_openclaw_state_fast(
    gateway_port: Option<u32>,
    installed: bool,
    loaded: bool,
    running: bool,
) -> OpenClawProbeResult {
    if let Some(port) = gateway_port {
        if gateway_port_reachable(port) {
            return OpenClawProbeResult {
                state: "healthy".to_string(),
                detail: None,
            };
        }
    }

    if running || loaded {
        return OpenClawProbeResult {
            state: "unreachable".to_string(),
            detail: None,
        };
    }
    if installed || gateway_port.is_some() {
        return OpenClawProbeResult {
            state: "stopped".to_string(),
            detail: None,
        };
    }
    OpenClawProbeResult {
        state: "unknown".to_string(),
        detail: None,
    }
}

fn detect_openclaw_state_detailed(
    env_meta: &EnvMeta,
    gateway_port: Option<u32>,
    installed: bool,
    loaded: bool,
    running: bool,
    probe_spec: Option<&OpenClawProbeSpec>,
    process_env: &BTreeMap<String, String>,
) -> OpenClawProbeResult {
    let fast = detect_openclaw_state_fast(gateway_port, installed, loaded, running);
    let Some(port) = gateway_port else {
        return fast;
    };

    match probe_gateway_healthz(port) {
        HealthzProbeResult::Gateway => match probe_spec {
            Some(spec) => match probe_openclaw_health_command(spec, env_meta, process_env) {
                OpenClawHealthCommandProbe::Healthy => OpenClawProbeResult {
                    state: "healthy".to_string(),
                    detail: None,
                },
                OpenClawHealthCommandProbe::AuthRequired(detail) => OpenClawProbeResult {
                    state: "auth-required".to_string(),
                    detail: Some(detail),
                },
                OpenClawHealthCommandProbe::RespondingButInvalid(detail) => OpenClawProbeResult {
                    state: "responding-but-invalid".to_string(),
                    detail: Some(detail),
                },
            },
            None => OpenClawProbeResult {
                state: "healthy".to_string(),
                detail: None,
            },
        },
        HealthzProbeResult::WrongService(detail) => OpenClawProbeResult {
            state: "wrong-service".to_string(),
            detail: Some(detail),
        },
        HealthzProbeResult::Unavailable(detail) => {
            if fast.state == "healthy" {
                OpenClawProbeResult {
                    state: "wrong-service".to_string(),
                    detail: Some(detail),
                }
            } else if fast.state == "unreachable" {
                OpenClawProbeResult {
                    state: fast.state,
                    detail: Some(detail),
                }
            } else {
                fast
            }
        }
    }
}

fn gateway_port_reachable(port: u32) -> bool {
    let Ok(port) = u16::try_from(port) else {
        return false;
    };
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    TcpStream::connect_timeout(
        &address.into(),
        Duration::from_millis(GATEWAY_PROBE_TIMEOUT_MS),
    )
    .is_ok()
}

fn probe_gateway_healthz(port: u32) -> HealthzProbeResult {
    let url = format!("http://127.0.0.1:{port}/healthz");
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_millis(GATEWAY_HTTP_PROBE_TIMEOUT_MS))
        .timeout_read(Duration::from_millis(GATEWAY_HTTP_PROBE_TIMEOUT_MS))
        .timeout_write(Duration::from_millis(GATEWAY_HTTP_PROBE_TIMEOUT_MS))
        .build();

    match agent.get(&url).call() {
        Ok(response) => {
            let status = response.status();
            if status != 200 {
                return HealthzProbeResult::WrongService(format!(
                    "gateway /healthz returned HTTP {status}"
                ));
            }

            let body = response.into_string().unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&body) {
                Ok(value)
                    if value["ok"].as_bool() == Some(true)
                        && value["status"].as_str() == Some("live") =>
                {
                    HealthzProbeResult::Gateway
                }
                Ok(value) => HealthzProbeResult::WrongService(format!(
                    "gateway /healthz returned unexpected payload: {value}"
                )),
                Err(_) => {
                    let detail = body.trim();
                    if detail.is_empty() {
                        HealthzProbeResult::WrongService(
                            "gateway /healthz returned an empty response".to_string(),
                        )
                    } else {
                        HealthzProbeResult::WrongService(format!(
                            "gateway /healthz returned unexpected payload: {detail}"
                        ))
                    }
                }
            }
        }
        Err(ureq::Error::Status(status, _)) => {
            HealthzProbeResult::WrongService(format!("gateway /healthz returned HTTP {status}"))
        }
        Err(ureq::Error::Transport(error)) => {
            HealthzProbeResult::Unavailable(format!("gateway /healthz probe failed: {error}"))
        }
    }
}

fn probe_openclaw_health_command(
    probe_spec: &OpenClawProbeSpec,
    env_meta: &EnvMeta,
    process_env: &BTreeMap<String, String>,
) -> OpenClawHealthCommandProbe {
    let probe_env = build_openclaw_env(env_meta, process_env);
    let mut command = match probe_spec {
        OpenClawProbeSpec::Shell { command, run_dir } => {
            let mut probe = if cfg!(windows) {
                let mut probe = Command::new("cmd");
                probe.args(["/C", command.as_str()]);
                probe
            } else {
                let mut probe = Command::new("sh");
                probe.args(["-lc", command.as_str()]);
                probe
            };
            probe.current_dir(run_dir);
            probe
        }
        OpenClawProbeSpec::Direct {
            command,
            args,
            run_dir,
        } => {
            let mut probe = Command::new(command);
            probe.args(args).current_dir(run_dir);
            probe
        }
    };
    command.env_clear().envs(&probe_env);

    let output = match run_probe_command(&mut command) {
        Ok(output) => output,
        Err(error) => {
            return OpenClawHealthCommandProbe::RespondingButInvalid(error);
        }
    };

    if output.status.success() {
        return OpenClawHealthCommandProbe::Healthy;
    }

    let detail = summarize_probe_output(&output.stdout, &output.stderr).unwrap_or_else(|| {
        format!(
            "OpenClaw health probe exited with code {}",
            output.status.code().unwrap_or(1)
        )
    });

    if detail_requires_auth(&detail) {
        OpenClawHealthCommandProbe::AuthRequired(detail)
    } else {
        OpenClawHealthCommandProbe::RespondingButInvalid(detail)
    }
}

fn run_probe_command(command: &mut Command) -> Result<std::process::Output, String> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to run OpenClaw health probe: {error}"))?;
    let deadline = Instant::now() + Duration::from_millis(GATEWAY_HEALTH_PROCESS_TIMEOUT_MS);

    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child.wait_with_output().map_err(|error| {
                    format!("failed to read OpenClaw health probe output: {error}")
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "OpenClaw health probe timed out after {}ms",
                        GATEWAY_HEALTH_PROCESS_TIMEOUT_MS
                    ));
                }
                sleep(Duration::from_millis(GATEWAY_HEALTH_PROCESS_POLL_MS));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("failed to wait for OpenClaw health probe: {error}"));
            }
        }
    }
}

fn summarize_probe_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    for bytes in [stderr, stdout] {
        let text = String::from_utf8_lossy(bytes);
        if let Some(line) = text.lines().find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        }) {
            let detail = line
                .strip_prefix("Health check failed: ")
                .unwrap_or(line)
                .trim();
            return Some(detail.to_string());
        }
    }
    None
}

fn detail_requires_auth(detail: &str) -> bool {
    let lower = detail.to_ascii_lowercase();
    lower.contains("unauthorized")
        || lower.contains("pairing required")
        || lower.contains("auth required")
        || lower.contains("auth_token")
        || lower.contains("auth_password")
        || lower.contains("gateway token")
        || lower.contains("gateway password")
        || lower.contains("token mismatch")
        || lower.contains("token missing")
        || lower.contains("password mismatch")
        || lower.contains("password missing")
        || lower.contains("device identity required")
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
    bootstrap_managed_node: bool,
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
            let launch = resolve_runtime_launch(
                &runtime,
                &gateway_args,
                process_env,
                cwd,
                bootstrap_managed_node,
            )?;
            Ok(ServiceLaunchSpec::Runtime {
                binding_name: name,
                binary_path: launch.program,
                args: launch.args,
                run_dir: Path::new(&env.root).to_path_buf(),
            })
        }
    }
}

pub(crate) fn managed_service_label(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<String, String> {
    Ok(managed_service_identity(name, env, cwd)?.label)
}

pub(crate) fn managed_plist_path(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    Ok(managed_service_identity(name, env, cwd)?.definition_path)
}

pub(crate) fn global_plist_path(env: &BTreeMap<String, String>) -> PathBuf {
    global_service_definition_path(env)
}

pub(crate) fn launch_agents_dir(env: &BTreeMap<String, String>) -> PathBuf {
    service_definition_dir(env)
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
            let pid = value.trim().parse::<u32>().ok().filter(|pid| *pid > 0);
            status.pid = pid;
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
            || !file_name.ends_with(&format!(
                ".{}",
                service_definition_extension(service_manager_kind(env))
            ))
        {
            continue;
        }
        if read_service_environment_value(&path, "OPENCLAW_CONFIG_PATH", env)?.as_deref()
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
    env: &BTreeMap<String, String>,
) -> (bool, Option<String>) {
    if service_manager_kind(env) != ServiceManagerKind::Launchd {
        return match source_kind {
            "openclaw-global" => (
                false,
                Some("moving existing OpenClaw services into OCM is not supported on this backend yet".to_string()),
            ),
            "ocm-managed" => (false, Some("already managed by ocm".to_string())),
            _ => (
                false,
                Some("foreign OpenClaw services are discoverable but not adoptable yet".to_string()),
            ),
        };
    }

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

fn read_service_label(
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => read_plist_string_value(service_path, "Label"),
        ServiceManagerKind::SystemdUser => Ok(service_path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())),
        ServiceManagerKind::Unsupported => Ok(service_path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())),
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

fn read_service_program(
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => read_plist_string_value(service_path, "Program"),
        ServiceManagerKind::SystemdUser => {
            Ok(read_systemd_exec_start(service_path)?.first().cloned())
        }
        ServiceManagerKind::Unsupported => Ok(None),
    }
}

fn read_service_program_arguments(
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => read_plist_array_values(service_path, "ProgramArguments"),
        ServiceManagerKind::SystemdUser => read_systemd_exec_start(service_path),
        ServiceManagerKind::Unsupported => Ok(Vec::new()),
    }
}

fn read_service_working_directory(
    service_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Option<String>, String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => read_plist_string_value(service_path, "WorkingDirectory"),
        ServiceManagerKind::SystemdUser => read_systemd_directive(service_path, "WorkingDirectory"),
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

fn read_systemd_exec_start(service_path: &Path) -> Result<Vec<String>, String> {
    let Some(value) = read_systemd_directive(service_path, "ExecStart")? else {
        return Ok(Vec::new());
    };
    parse_systemd_words(&value)
}

fn read_systemd_directive(service_path: &Path, key: &str) -> Result<Option<String>, String> {
    Ok(read_systemd_directive_values(service_path, key)?
        .into_iter()
        .next())
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
        parse_systemctl_show, parse_systemd_words, read_plist_array_values_from_section,
        string_mentions_openclaw,
    };
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn managed_service_labels_are_store_scoped() {
        let mut env = BTreeMap::new();
        env.insert("OCM_HOME".to_string(), "/tmp/store".to_string());

        let label = managed_service_label("demo", &env, Path::new("/tmp")).unwrap();
        assert!(label.starts_with("ai.openclaw.gateway.ocm."));
        assert!(label.ends_with(".demo"));
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
            discovered_source_kind("ai.openclaw.gateway.ocm.f8587fe2b3.demo"),
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
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "launchd".to_string(),
        );
        let (adoptable, reason) = discover_adoption_state(
            "openclaw-global",
            Some("demo"),
            Some("/tmp/openclaw.json"),
            &env,
        );
        assert!(adoptable);
        assert_eq!(
            reason.as_deref(),
            Some("ready to adopt into env \"demo\" with service adopt-global")
        );

        let (adoptable, reason) =
            discover_adoption_state("foreign", Some("demo"), Some("/tmp/openclaw.json"), &env);
        assert!(!adoptable);
        assert_eq!(
            reason.as_deref(),
            Some("foreign OpenClaw services are discoverable but not adoptable yet")
        );
    }

    #[test]
    fn discover_adoption_state_is_backend_aware() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );

        let (adoptable, reason) = discover_adoption_state(
            "openclaw-global",
            Some("demo"),
            Some("/tmp/openclaw.json"),
            &env,
        );
        assert!(!adoptable);
        assert_eq!(
            reason.as_deref(),
            Some("moving existing OpenClaw services into OCM is not supported on this backend yet")
        );
    }

    #[test]
    fn parse_systemctl_show_extracts_core_fields() {
        let mut status = LaunchdJobStatus::default();
        parse_systemctl_show(
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\nMainPID=4242\n",
            &mut status,
        );

        assert!(status.loaded);
        assert!(status.running);
        assert_eq!(status.pid, Some(4242));
        assert_eq!(status.state.as_deref(), Some("running"));
    }

    #[test]
    fn parse_systemctl_show_extracts_live_launch_details() {
        let mut status = LaunchdJobStatus::default();
        parse_systemctl_show(
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\nMainPID=4242\nExecStart=/bin/sh -lc \"/bin/true gateway run --port 18790\"\nWorkingDirectory=/tmp/live\nEnvironment=\"OPENCLAW_CONFIG_PATH=/tmp/live/.openclaw/openclaw.json\" \"OPENCLAW_STATE_DIR=/tmp/live/.openclaw\" \"OPENCLAW_HOME=/tmp/live\" \"OPENCLAW_GATEWAY_PORT=18790\"\n",
            &mut status,
        );

        assert_eq!(
            status.program_arguments,
            vec![
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "/bin/true gateway run --port 18790".to_string(),
            ]
        );
        assert_eq!(status.working_directory.as_deref(), Some("/tmp/live"));
        assert_eq!(
            status.config_path.as_deref(),
            Some("/tmp/live/.openclaw/openclaw.json")
        );
        assert_eq!(status.gateway_port, Some(18790));
    }

    #[test]
    fn systemd_word_parser_round_trips_quoted_exec_start() {
        let words =
            parse_systemd_words("/bin/sh -lc \"openclaw gateway run --port 18789\"").unwrap();
        assert_eq!(
            words,
            vec![
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "openclaw gateway run --port 18789".to_string()
            ]
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
