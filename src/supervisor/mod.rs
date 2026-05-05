use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::env::{EnvironmentService, resolve_gateway_process_spec};
use crate::service::inspect::inspect_job;
use crate::service::platform::{
    ManagedServiceDefinition, ServiceManagerKind, activate_managed_service,
    managed_service_identity, service_manager_kind, write_managed_service_definition,
};
use crate::store::{
    display_path, ensure_dir, ensure_store, list_environments, now_utc,
    openclaw_port_family_available, openclaw_port_family_range, read_json, resolve_ocm_home,
    supervisor_logs_dir, supervisor_runtime_path, supervisor_state_path, write_json,
};

const SUPERVISOR_STATE_KIND: &str = "ocm-supervisor-state";
const SUPERVISOR_RUNTIME_KIND: &str = "ocm-supervisor-runtime";
const SUPERVISOR_POLL_INTERVAL_MS: u64 = 200;
const SUPERVISOR_RESTART_DELAY_MS: u64 = 1_000;
const SUPERVISOR_MAX_RESTART_DELAY_MS: u64 = 30_000;
const SUPERVISOR_STABLE_RUN_MS: u64 = 10_000;
const DEFAULT_SERVICE_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
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
const OPENCLAW_SERVICE_MARKER: &str = "openclaw";
const OPENCLAW_GATEWAY_SERVICE_KIND: &str = "gateway";
const OPENCLAW_RESTART_HANDOFF_FILE: &str = "gateway-supervisor-restart-handoff.json";
const OPENCLAW_RESTART_HANDOFF_KIND: &str = "gateway-supervisor-restart-handoff";
const OPENCLAW_RESTART_HANDOFF_TTL_MS: u64 = 60_000;
const OPENCLAW_RESTART_HANDOFF_MAX_BYTES: u64 = 4096;
const OPENCLAW_RESTART_HANDOFF_MAX_INTENT_ID_CHARS: usize = 120;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorChildSpec {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub runtime_source_kind: Option<String>,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub args: Vec<String>,
    pub run_dir: String,
    pub child_port: u32,
    pub stdout_path: String,
    pub stderr_path: String,
    pub process_env: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedSupervisorEnv {
    pub env_name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorState {
    pub kind: String,
    pub ocm_home: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    pub children: Vec<SupervisorChildSpec>,
    pub skipped_envs: Vec<SkippedSupervisorEnv>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorView {
    pub state_path: String,
    pub persisted: bool,
    pub kind: String,
    pub ocm_home: String,
    #[serde(with = "time::serde::rfc3339")]
    pub generated_at: OffsetDateTime,
    pub children: Vec<SupervisorChildSpec>,
    pub skipped_envs: Vec<SkippedSupervisorEnv>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorDaemonSummary {
    pub action: String,
    pub managed_label: String,
    pub definition_path: String,
    pub state_path: String,
    pub ocm_home: String,
    pub executable_path: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub installed: bool,
    pub loaded: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub state: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorChildRunResult {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub restart_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorRunSummary {
    pub state_path: String,
    pub once: bool,
    pub child_count: usize,
    pub stopped_by_signal: bool,
    pub child_results: Vec<SupervisorChildRunResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorRuntimeChild {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub pid: u32,
    pub restart_count: usize,
    pub child_port: u32,
    pub stdout_path: String,
    pub stderr_path: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorRuntimeState {
    pub kind: String,
    pub ocm_home: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub services: Vec<SupervisorRuntimeService>,
    pub children: Vec<SupervisorRuntimeChild>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorRuntimeService {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub gateway_state: String,
    pub restart_count: usize,
    pub child_port: u32,
    pub pid: Option<u32>,
    pub stdout_path: String,
    pub stderr_path: String,
    pub last_exit_code: Option<i32>,
    pub last_error: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_event_at: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub next_retry_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorRuntimeView {
    pub runtime_path: String,
    pub present: bool,
    pub kind: String,
    pub ocm_home: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    pub children: Vec<SupervisorRuntimeChild>,
}

#[derive(Clone, Debug)]
pub struct SupervisorInspection {
    pub daemon: SupervisorDaemonSummary,
    pub planned_children: Vec<SupervisorChildSpec>,
    pub skipped_envs: Vec<SkippedSupervisorEnv>,
    pub runtime_children: Vec<SupervisorRuntimeChild>,
    pub runtime_services: Vec<SupervisorRuntimeService>,
}

pub struct SupervisorService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

pub fn sync_supervisor_if_present(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<bool, String> {
    let state_path = supervisor_state_path(env, cwd)?;
    let runtime_path = supervisor_runtime_path(env, cwd)?;
    if !state_path.exists() && !runtime_path.exists() {
        return Ok(false);
    }
    SupervisorService::new(env, cwd).sync()?;
    Ok(true)
}

impl<'a> SupervisorService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn plan(&self) -> Result<SupervisorView, String> {
        let state = self.build_state()?;
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        Ok(view_from_state(&state_path, false, state))
    }

    pub fn sync(&self) -> Result<SupervisorView, String> {
        let state = self.build_state()?;
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        if let Some(parent) = state_path.parent() {
            ensure_dir(parent)?;
        }
        ensure_dir(&supervisor_logs_dir(self.env, self.cwd)?)?;
        write_json(&state_path, &state)?;
        Ok(view_from_state(&state_path, true, state))
    }

    pub fn runtime(&self) -> Result<SupervisorRuntimeView, String> {
        let runtime_path = supervisor_runtime_path(self.env, self.cwd)?;
        if !runtime_path.exists() {
            let ocm_home = resolve_ocm_home(self.env, self.cwd)?;
            return Ok(SupervisorRuntimeView {
                runtime_path: display_path(&runtime_path),
                present: false,
                kind: SUPERVISOR_RUNTIME_KIND.to_string(),
                ocm_home: display_path(&ocm_home),
                updated_at: now_utc(),
                children: Vec::new(),
            });
        }

        let runtime = read_json::<SupervisorRuntimeState>(&runtime_path)?;
        Ok(SupervisorRuntimeView {
            runtime_path: display_path(&runtime_path),
            present: true,
            kind: runtime.kind,
            ocm_home: runtime.ocm_home,
            updated_at: runtime.updated_at,
            children: runtime.children,
        })
    }

    pub fn inspect(&self) -> Result<SupervisorInspection, String> {
        let state = self.build_state()?;
        let daemon = self.daemon_status()?;
        let runtime = if daemon.running {
            self.read_runtime_state()?
        } else {
            None
        };

        Ok(SupervisorInspection {
            daemon,
            planned_children: state.children,
            skipped_envs: state.skipped_envs,
            runtime_children: runtime
                .as_ref()
                .map(|runtime| runtime.children.clone())
                .unwrap_or_default(),
            runtime_services: runtime.map(|runtime| runtime.services).unwrap_or_default(),
        })
    }

    pub fn run(&self, once: bool) -> Result<SupervisorRunSummary, String> {
        let (state_path, state) = self.read_persisted_state()?;
        if once {
            return self.run_once(&state_path, state);
        }
        self.run_until_stopped(&state_path, state)
    }

    pub fn install_daemon(&self) -> Result<SupervisorDaemonSummary, String> {
        self.refresh_daemon("install")
    }

    pub fn ensure_daemon_running(&self) -> Result<SupervisorDaemonSummary, String> {
        let _ = self.sync()?;
        let status = self.daemon_status()?;
        if status.running {
            return Ok(status);
        }
        self.activate_daemon("install")
    }

    pub fn daemon_status(&self) -> Result<SupervisorDaemonSummary, String> {
        self.daemon_summary("status")
    }
    fn build_state(&self) -> Result<SupervisorState, String> {
        ensure_store(self.env, self.cwd)?;
        let ocm_home = resolve_ocm_home(self.env, self.cwd)?;
        let logs_dir = supervisor_logs_dir(self.env, self.cwd)?;
        let identity = managed_service_identity(self.env, self.cwd)?;
        let service_manager = service_manager_kind(self.env);
        let env_service = EnvironmentService::new(self.env, self.cwd);
        let mut envs = list_environments(self.env, self.cwd)?;
        envs.sort_by(|left, right| left.name.cmp(&right.name));
        let envs = env_service.apply_effective_gateway_ports(envs)?;

        let mut children = Vec::new();
        let mut skipped_envs = Vec::new();
        for env_meta in envs {
            let name = env_meta.name.clone();
            if !env_meta.service_enabled {
                skipped_envs.push(SkippedSupervisorEnv {
                    env_name: name,
                    reason: "service is disabled".to_string(),
                });
                continue;
            }
            if !env_meta.service_running {
                skipped_envs.push(SkippedSupervisorEnv {
                    env_name: name,
                    reason: "service is stopped".to_string(),
                });
                continue;
            }
            match resolve_gateway_process_spec(&env_meta, self.env, self.cwd, false) {
                Ok(process) => {
                    let args = process.args.clone();
                    let child_port = process
                        .process_env
                        .get("OPENCLAW_GATEWAY_PORT")
                        .ok_or_else(|| format!("failed to resolve child port for env \"{name}\""))?
                        .parse::<u32>()
                        .map_err(|_| format!("failed to parse child port for env \"{name}\""))?;
                    let process_env = build_supervised_openclaw_env(
                        process.process_env,
                        &identity.label,
                        service_manager,
                    );
                    children.push(SupervisorChildSpec {
                        env_name: name,
                        binding_kind: process.binding_kind,
                        binding_name: process.binding_name,
                        command: process.command,
                        binary_path: process.binary_path,
                        runtime_source_kind: process.runtime_source_kind,
                        runtime_release_version: process.runtime_release_version,
                        runtime_release_channel: process.runtime_release_channel,
                        args,
                        run_dir: display_path(&process.run_dir),
                        child_port,
                        stdout_path: display_path(
                            &logs_dir.join(format!("{}.stdout.log", process.env_name)),
                        ),
                        stderr_path: display_path(
                            &logs_dir.join(format!("{}.stderr.log", process.env_name)),
                        ),
                        process_env,
                    });
                }
                Err(reason) => skipped_envs.push(SkippedSupervisorEnv {
                    env_name: name,
                    reason,
                }),
            }
        }
        let (children, additional_skipped) = filter_conflicting_supervisor_children(children);
        skipped_envs.extend(additional_skipped);
        skipped_envs.sort_by(|left, right| left.env_name.cmp(&right.env_name));

        Ok(SupervisorState {
            kind: SUPERVISOR_STATE_KIND.to_string(),
            ocm_home: display_path(&ocm_home),
            generated_at: now_utc(),
            children,
            skipped_envs,
        })
    }

    fn read_persisted_state(&self) -> Result<(PathBuf, SupervisorState), String> {
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        if !state_path.exists() {
            return Err(
                "service state has not been written yet; run \"ocm service install <env>\" or \"ocm service start <env>\" first".to_string(),
            );
        }
        let state = read_json(&state_path)?;
        Ok((state_path, state))
    }

    fn refresh_daemon(&self, action: &str) -> Result<SupervisorDaemonSummary, String> {
        let _ = self.sync()?;
        self.activate_daemon(action)
    }

    fn activate_daemon(&self, action: &str) -> Result<SupervisorDaemonSummary, String> {
        let definition = self.supervisor_daemon_definition()?;
        write_managed_service_definition(&definition, self.env)?;
        activate_managed_service(&definition.label, &definition.definition_path, self.env)?;
        self.daemon_summary(action)
    }

    fn daemon_summary(&self, action: &str) -> Result<SupervisorDaemonSummary, String> {
        ensure_store(self.env, self.cwd)?;
        let ocm_home = resolve_ocm_home(self.env, self.cwd)?;
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        let identity = managed_service_identity(self.env, self.cwd)?;
        let logs_dir = supervisor_logs_dir(self.env, self.cwd)?;
        let stdout_path = logs_dir.join("daemon.stdout.log");
        let stderr_path = logs_dir.join("daemon.stderr.log");
        let status = inspect_job(&identity.label, &identity.definition_path, self.env);
        let executable_path = self.supervisor_executable_path()?;

        Ok(SupervisorDaemonSummary {
            action: action.to_string(),
            managed_label: identity.label,
            definition_path: display_path(&identity.definition_path),
            state_path: display_path(&state_path),
            ocm_home: display_path(&ocm_home),
            executable_path: display_path(&executable_path),
            stdout_path: display_path(&stdout_path),
            stderr_path: display_path(&stderr_path),
            installed: status.installed,
            loaded: status.loaded,
            running: status.running,
            pid: status.pid,
            state: status.state,
        })
    }

    fn supervisor_daemon_definition(&self) -> Result<ManagedServiceDefinition, String> {
        let ocm_home = resolve_ocm_home(self.env, self.cwd)?;
        let identity = managed_service_identity(self.env, self.cwd)?;
        let logs_dir = supervisor_logs_dir(self.env, self.cwd)?;
        let executable_path = self.supervisor_executable_path()?;

        Ok(ManagedServiceDefinition {
            label: identity.label,
            description: format!(
                "OCM background service for store {}",
                display_path(&ocm_home)
            ),
            definition_path: identity.definition_path,
            program_arguments: vec![
                display_path(&executable_path),
                "__daemon".to_string(),
                "run".to_string(),
            ],
            working_directory: ocm_home.clone(),
            stdout_path: logs_dir.join("daemon.stdout.log"),
            stderr_path: logs_dir.join("daemon.stderr.log"),
            environment: supervisor_service_environment(self.env, &ocm_home, &executable_path),
        })
    }

    fn supervisor_executable_path(&self) -> Result<PathBuf, String> {
        std::env::current_exe().map_err(|error| {
            format!(
                "failed to resolve the current ocm executable for the OCM background service: {error}"
            )
        })
    }

    fn read_runtime_state(&self) -> Result<Option<SupervisorRuntimeState>, String> {
        let runtime_path = supervisor_runtime_path(self.env, self.cwd)?;
        if !runtime_path.exists() {
            return Ok(None);
        }

        Ok(Some(read_json::<SupervisorRuntimeState>(&runtime_path)?))
    }

    fn run_once(
        &self,
        state_path: &Path,
        state: SupervisorState,
    ) -> Result<SupervisorRunSummary, String> {
        let mut child_results = Vec::with_capacity(state.children.len());
        for spec in state.children {
            eprintln!(
                "ocm service: starting {} ({})",
                spec.env_name,
                child_binding_label(&spec)
            );
            let mut child = spawn_supervisor_child(&spec)?;
            let status = child.wait().map_err(|error| {
                format!("failed waiting for env \"{}\": {error}", spec.env_name)
            })?;
            child_results.push(child_run_result(&spec, status.code(), 0));
        }

        Ok(SupervisorRunSummary {
            state_path: display_path(state_path),
            once: true,
            child_count: child_results.len(),
            stopped_by_signal: false,
            child_results,
        })
    }

    fn run_until_stopped(
        &self,
        state_path: &Path,
        state: SupervisorState,
    ) -> Result<SupervisorRunSummary, String> {
        let stop_requested = Arc::new(AtomicBool::new(false));
        let signal_flag = Arc::clone(&stop_requested);
        ctrlc::set_handler(move || {
            signal_flag.store(true, Ordering::SeqCst);
        })
        .map_err(|error| format!("failed to install service signal handler: {error}"))?;

        let runtime_path = supervisor_runtime_path(self.env, self.cwd)?;
        let mut active_state = state;
        let mut running = BTreeMap::new();
        let mut pending = BTreeMap::new();
        let mut inactive = BTreeMap::new();
        queue_missing_children(
            &mut pending,
            &running,
            &active_state.children,
            0,
            Instant::now(),
        );
        start_due_children(&mut running, &mut pending, &mut inactive)?;
        write_supervisor_runtime_state(
            &runtime_path,
            &active_state.ocm_home,
            &running,
            &pending,
            &inactive,
        )?;
        let mut managed_child_count = active_state.children.len();
        let mut child_results = Vec::new();

        while !stop_requested.load(Ordering::SeqCst) {
            let mut runtime_dirty = refresh_active_state(
                state_path,
                &mut active_state,
                &mut managed_child_count,
                &mut running,
                &mut pending,
                &mut inactive,
            );
            runtime_dirty |= process_exited_children(
                state_path,
                &stop_requested,
                &mut active_state,
                &mut managed_child_count,
                &mut running,
                &mut pending,
                &mut inactive,
                &mut child_results,
            )?;

            runtime_dirty |= start_due_children(&mut running, &mut pending, &mut inactive)?;
            if runtime_dirty {
                write_supervisor_runtime_state(
                    &runtime_path,
                    &active_state.ocm_home,
                    &running,
                    &pending,
                    &inactive,
                )?;
            }

            if !stop_requested.load(Ordering::SeqCst) {
                sleep(Duration::from_millis(SUPERVISOR_POLL_INTERVAL_MS));
            }
        }

        for (_, mut running_child) in running {
            stop_supervisor_child(&mut running_child);
        }
        write_supervisor_runtime_state(
            &runtime_path,
            &active_state.ocm_home,
            &BTreeMap::new(),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )?;

        Ok(SupervisorRunSummary {
            state_path: display_path(state_path),
            once: false,
            child_count: managed_child_count,
            stopped_by_signal: true,
            child_results,
        })
    }
}

struct RunningSupervisorChild {
    spec: SupervisorChildSpec,
    child: Child,
    restart_count: usize,
    quick_clean_restart_count: usize,
    started_at: Instant,
}

#[derive(Clone)]
struct PendingSupervisorChild {
    spec: SupervisorChildSpec,
    restart_count: usize,
    quick_clean_restart_count: usize,
    retry_at: Instant,
    retry_at_utc: OffsetDateTime,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
    last_event_at: Option<OffsetDateTime>,
}

struct ExitedSupervisorChild {
    env_name: String,
    exit_code: Option<i32>,
    restart_count: usize,
    quick_clean_restart_count: usize,
    restart_handoff: Option<OpenClawRestartHandoff>,
    ran_for: Duration,
}

struct ExitedSupervisorChildDecision {
    should_restart: bool,
    quick_clean_handoff: bool,
    log_action: &'static str,
    last_error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenClawRestartHandoff {
    kind: String,
    version: u8,
    intent_id: String,
    pid: u64,
    created_at: u64,
    expires_at: u64,
    source: String,
    restart_kind: String,
    supervisor_mode: String,
}

#[derive(Clone)]
struct InactiveSupervisorChild {
    spec: SupervisorChildSpec,
    gateway_state: String,
    restart_count: usize,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
    last_event_at: Option<OffsetDateTime>,
    next_retry_at: Option<OffsetDateTime>,
}

fn spawn_running_child(
    spec: SupervisorChildSpec,
    restart_count: usize,
    quick_clean_restart_count: usize,
) -> Result<RunningSupervisorChild, String> {
    eprintln!(
        "ocm service: starting {} ({})",
        spec.env_name,
        child_binding_label(&spec)
    );
    Ok(RunningSupervisorChild {
        child: spawn_supervisor_child(&spec)?,
        spec,
        restart_count,
        quick_clean_restart_count,
        started_at: Instant::now(),
    })
}

fn spawn_supervisor_child(spec: &SupervisorChildSpec) -> Result<Child, String> {
    let program_arguments = supervisor_program_arguments(spec);
    let Some(program) = program_arguments.first() else {
        return Err(format!(
            "service child env \"{}\" is missing program arguments",
            spec.env_name
        ));
    };
    if let Some(parent) = Path::new(&spec.stdout_path).parent() {
        ensure_dir(parent)?;
    }
    if let Some(parent) = Path::new(&spec.stderr_path).parent() {
        ensure_dir(parent)?;
    }
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&spec.stdout_path)
        .map_err(|error| {
            format!(
                "failed opening stdout log for env \"{}\": {error}",
                spec.env_name
            )
        })?;
    let stderr = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&spec.stderr_path)
        .map_err(|error| {
            format!(
                "failed opening stderr log for env \"{}\": {error}",
                spec.env_name
            )
        })?;

    let mut command = Command::new(program);
    command
        .args(program_arguments.iter().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env_clear()
        .envs(&spec.process_env)
        .current_dir(Path::new(&spec.run_dir));
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    command.spawn().map_err(|error| {
        format!(
            "failed starting env \"{}\" with {}: {error}",
            spec.env_name,
            child_binding_label(spec)
        )
    })
}

fn supervisor_program_arguments(spec: &SupervisorChildSpec) -> Vec<String> {
    match (&spec.binary_path, &spec.command) {
        (Some(binary_path), _) => {
            let mut program_arguments = vec![binary_path.clone()];
            program_arguments.extend(spec.args.iter().cloned());
            program_arguments
        }
        (None, Some(command)) => {
            if cfg!(windows) {
                vec!["cmd".to_string(), "/C".to_string(), command.to_string()]
            } else {
                vec![
                    "/bin/sh".to_string(),
                    "-lc".to_string(),
                    command.to_string(),
                ]
            }
        }
        (None, None) => Vec::new(),
    }
}

fn child_binding_label(spec: &SupervisorChildSpec) -> String {
    format!("{}:{}", spec.binding_kind, spec.binding_name)
}

fn child_run_result(
    spec: &SupervisorChildSpec,
    exit_code: Option<i32>,
    restart_count: usize,
) -> SupervisorChildRunResult {
    SupervisorChildRunResult {
        env_name: spec.env_name.clone(),
        binding_kind: spec.binding_kind.clone(),
        binding_name: spec.binding_name.clone(),
        exit_code,
        success: exit_code == Some(0),
        restart_count,
    }
}

fn view_from_state(state_path: &Path, persisted: bool, state: SupervisorState) -> SupervisorView {
    SupervisorView {
        state_path: display_path(state_path),
        persisted,
        kind: state.kind,
        ocm_home: state.ocm_home,
        generated_at: state.generated_at,
        children: state.children,
        skipped_envs: state.skipped_envs,
    }
}

fn child_map(children: &[SupervisorChildSpec]) -> BTreeMap<String, SupervisorChildSpec> {
    children
        .iter()
        .cloned()
        .map(|child| (child.env_name.clone(), child))
        .collect()
}

fn read_updated_supervisor_state(
    state_path: &Path,
    active_state: &SupervisorState,
) -> Option<SupervisorState> {
    let next_state = match read_json::<SupervisorState>(state_path) {
        Ok(state) => state,
        Err(error) => {
            eprintln!(
                "ocm service: failed reading updated state {}: {}",
                display_path(state_path),
                error
            );
            return None;
        }
    };
    if supervisor_state_equivalent(active_state, &next_state) {
        return None;
    }
    Some(next_state)
}

fn refresh_active_state(
    state_path: &Path,
    active_state: &mut SupervisorState,
    managed_child_count: &mut usize,
    running: &mut BTreeMap<String, RunningSupervisorChild>,
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
    inactive: &mut BTreeMap<String, InactiveSupervisorChild>,
) -> bool {
    let Some(next_state) = read_updated_supervisor_state(state_path, active_state) else {
        return false;
    };
    let runtime_dirty = reconcile_running_children(running, pending, inactive, &next_state);
    *managed_child_count = next_state.children.len();
    *active_state = next_state;
    runtime_dirty
}

fn reconcile_running_children(
    running: &mut BTreeMap<String, RunningSupervisorChild>,
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
    inactive: &mut BTreeMap<String, InactiveSupervisorChild>,
    desired_state: &SupervisorState,
) -> bool {
    let desired = child_map(&desired_state.children);
    let mut runtime_dirty = false;

    let pending_names = pending.keys().cloned().collect::<Vec<_>>();
    for env_name in pending_names {
        match desired.get(&env_name) {
            Some(next_spec) => {
                let should_replace = pending
                    .get(&env_name)
                    .map(|entry| entry.spec != *next_spec)
                    .unwrap_or(false);
                if should_replace {
                    pending.insert(
                        env_name,
                        pending_supervisor_child(next_spec.clone(), 0, 0, 0, None, None),
                    );
                }
            }
            None => {
                pending.remove(&env_name);
                inactive.remove(&env_name);
            }
        }
    }

    let active_names = running.keys().cloned().collect::<Vec<_>>();
    for env_name in active_names {
        let Some(next_spec) = desired.get(&env_name) else {
            if let Some(mut existing) = running.remove(&env_name) {
                eprintln!(
                    "ocm service: stopping removed env {}",
                    existing.spec.env_name
                );
                stop_supervisor_child(&mut existing);
                inactive.remove(&env_name);
                runtime_dirty = true;
            }
            continue;
        };
        let needs_restart = running
            .get(&env_name)
            .map(|existing| existing.spec != *next_spec)
            .unwrap_or(false);
        if needs_restart {
            let mut existing = running
                .remove(&env_name)
                .expect("running child should exist when needs_restart is true");
            eprintln!(
                "ocm service: reloading {} ({})",
                existing.spec.env_name,
                child_binding_label(next_spec)
            );
            stop_supervisor_child(&mut existing);
            pending.insert(
                env_name,
                pending_supervisor_child(next_spec.clone(), 0, 0, 0, None, None),
            );
            inactive.remove(&existing.spec.env_name);
            runtime_dirty = true;
        }
    }
    queue_missing_children(pending, running, &desired_state.children, 0, Instant::now());
    runtime_dirty
}

fn collect_exited_children(
    running: &mut BTreeMap<String, RunningSupervisorChild>,
) -> Result<Vec<ExitedSupervisorChild>, String> {
    let mut exited = Vec::new();
    for (env_name, running_child) in running {
        if let Some(status) = running_child.child.try_wait().map_err(|error| {
            format!(
                "failed to poll env \"{}\": {error}",
                running_child.spec.env_name
            )
        })? {
            let ran_for = running_child.started_at.elapsed();
            let restart_handoff = if status.code() == Some(0)
                && ran_for < Duration::from_millis(SUPERVISOR_STABLE_RUN_MS)
            {
                consume_openclaw_restart_handoff(&running_child.spec, running_child.child.id())
            } else {
                None
            };
            exited.push(ExitedSupervisorChild {
                env_name: env_name.clone(),
                exit_code: status.code(),
                restart_count: running_child.restart_count,
                quick_clean_restart_count: running_child.quick_clean_restart_count,
                restart_handoff,
                ran_for,
            });
        }
    }
    Ok(exited)
}

fn process_exited_children(
    state_path: &Path,
    stop_requested: &AtomicBool,
    active_state: &mut SupervisorState,
    managed_child_count: &mut usize,
    running: &mut BTreeMap<String, RunningSupervisorChild>,
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
    inactive: &mut BTreeMap<String, InactiveSupervisorChild>,
    child_results: &mut Vec<SupervisorChildRunResult>,
) -> Result<bool, String> {
    let exited = collect_exited_children(running)?;
    let mut runtime_dirty = false;

    for exited_child in exited {
        let Some(previous_child) = running.remove(&exited_child.env_name) else {
            continue;
        };
        runtime_dirty = true;
        child_results.push(child_run_result(
            &previous_child.spec,
            exited_child.exit_code,
            exited_child.restart_count,
        ));
        runtime_dirty |= refresh_active_state(
            state_path,
            active_state,
            managed_child_count,
            running,
            pending,
            inactive,
        );
        let next_spec = active_child_spec(active_state, &exited_child.env_name).cloned();
        let decision = exited_child_restart_decision(
            &exited_child,
            next_spec
                .as_ref()
                .is_some_and(|spec| *spec == previous_child.spec),
            next_spec.is_some(),
        );
        eprintln!(
            "ocm service: {} exited with {}; {}",
            previous_child.spec.env_name,
            exited_child
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            decision.log_action
        );
        if stop_requested.load(Ordering::SeqCst) {
            break;
        }

        if decision.should_restart
            && let Some(next_spec) = next_spec
        {
            let next_restart_count = if next_spec == previous_child.spec {
                exited_child.restart_count + 1
            } else {
                0
            };
            let next_quick_clean_restart_count =
                if next_spec == previous_child.spec && decision.quick_clean_handoff {
                    exited_child.quick_clean_restart_count + 1
                } else {
                    0
                };
            let issue = decision.last_error.clone();
            pending.insert(
                exited_child.env_name.clone(),
                pending_supervisor_child(
                    next_spec.clone(),
                    next_restart_count,
                    next_quick_clean_restart_count,
                    restart_delay_ms(next_restart_count),
                    exited_child.exit_code,
                    issue.clone(),
                ),
            );
            inactive.insert(
                exited_child.env_name,
                inactive_supervisor_child(
                    next_spec,
                    "backoff",
                    next_restart_count,
                    exited_child.exit_code,
                    issue,
                    now_utc(),
                    pending
                        .get(&previous_child.spec.env_name)
                        .map(|child| child.retry_at_utc),
                ),
            );
        } else {
            inactive.insert(
                previous_child.spec.env_name.clone(),
                inactive_supervisor_child(
                    previous_child.spec.clone(),
                    "stopped",
                    exited_child.restart_count,
                    exited_child.exit_code,
                    decision.last_error.clone(),
                    now_utc(),
                    None,
                ),
            );
        }
    }

    Ok(runtime_dirty)
}

fn exited_child_restart_decision(
    exited_child: &ExitedSupervisorChild,
    same_desired_child: bool,
    desired_child_present: bool,
) -> ExitedSupervisorChildDecision {
    let exit_label = exited_child
        .exit_code
        .map(|code| code.to_string())
        .unwrap_or_else(|| "signal".to_string());

    if !desired_child_present {
        return ExitedSupervisorChildDecision {
            should_restart: false,
            quick_clean_handoff: false,
            log_action: "leaving stopped because service is no longer desired running",
            last_error: Some("process exited after service stop; leaving stopped".to_string()),
        };
    }

    let quick_clean_exit = exited_child.exit_code == Some(0)
        && exited_child.ran_for < Duration::from_millis(SUPERVISOR_STABLE_RUN_MS);
    if quick_clean_exit {
        if exited_child.restart_handoff.is_none() {
            return ExitedSupervisorChildDecision {
                should_restart: false,
                quick_clean_handoff: false,
                log_action: "leaving stopped after quick clean exit without restart handoff",
                last_error: Some(
                    "process exited cleanly too quickly without OpenClaw restart handoff; leaving stopped"
                        .to_string(),
                ),
            };
        }

        if !same_desired_child || exited_child.quick_clean_restart_count == 0 {
            return ExitedSupervisorChildDecision {
                should_restart: true,
                quick_clean_handoff: true,
                log_action: "OpenClaw restart handoff, retrying after backoff",
                last_error: Some(
                    "OpenClaw requested supervisor restart handoff; retrying after backoff"
                        .to_string(),
                ),
            };
        }

        return ExitedSupervisorChildDecision {
            should_restart: false,
            quick_clean_handoff: false,
            log_action: "leaving stopped after repeated quick clean exit",
            last_error: Some(
                "process exited cleanly too quickly after restart handoff; leaving stopped to avoid a restart loop"
                    .to_string(),
            ),
        };
    }

    ExitedSupervisorChildDecision {
        should_restart: true,
        quick_clean_handoff: false,
        log_action: "restarting",
        last_error: Some(format!(
            "process exited with {exit_label}; retrying after backoff"
        )),
    }
}

fn openclaw_restart_handoff_path(spec: &SupervisorChildSpec) -> Option<PathBuf> {
    if let Some(state_dir) = spec
        .process_env
        .get("OPENCLAW_STATE_DIR")
        .filter(|value| !value.trim().is_empty())
    {
        return Some(Path::new(state_dir).join(OPENCLAW_RESTART_HANDOFF_FILE));
    }
    spec.process_env
        .get("OPENCLAW_HOME")
        .filter(|value| !value.trim().is_empty())
        .map(|home| {
            Path::new(home)
                .join(".openclaw")
                .join(OPENCLAW_RESTART_HANDOFF_FILE)
        })
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

fn read_openclaw_restart_handoff(path: &Path) -> Option<OpenClawRestartHandoff> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file()
        || openclaw_restart_handoff_has_multiple_links(&metadata)
        || metadata.len() > OPENCLAW_RESTART_HANDOFF_MAX_BYTES
    {
        return None;
    }
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

#[cfg(unix)]
fn openclaw_restart_handoff_has_multiple_links(metadata: &fs::Metadata) -> bool {
    metadata.nlink() > 1
}

#[cfg(not(unix))]
fn openclaw_restart_handoff_has_multiple_links(_metadata: &fs::Metadata) -> bool {
    false
}

fn valid_openclaw_restart_handoff(
    handoff: &OpenClawRestartHandoff,
    exited_pid: u32,
    now_ms: u64,
) -> bool {
    let intent_id = handoff.intent_id.trim();
    if handoff.kind != OPENCLAW_RESTART_HANDOFF_KIND
        || handoff.version != 1
        || intent_id.is_empty()
        || intent_id.chars().count() > OPENCLAW_RESTART_HANDOFF_MAX_INTENT_ID_CHARS
        || handoff.pid != u64::from(exited_pid)
        || handoff.expires_at <= handoff.created_at
        || handoff.expires_at.saturating_sub(handoff.created_at) > OPENCLAW_RESTART_HANDOFF_TTL_MS
        || now_ms < handoff.created_at
        || now_ms > handoff.expires_at
    {
        return false;
    }
    matches!(
        handoff.source.as_str(),
        "config-write"
            | "gateway-update"
            | "operator-restart"
            | "plugin-change"
            | "signal"
            | "unknown"
    ) && matches!(
        handoff.restart_kind.as_str(),
        "full-process" | "update-process"
    ) && matches!(
        handoff.supervisor_mode.as_str(),
        "launchd" | "systemd" | "schtasks" | "external"
    )
}

fn consume_openclaw_restart_handoff(
    spec: &SupervisorChildSpec,
    exited_pid: u32,
) -> Option<OpenClawRestartHandoff> {
    let path = openclaw_restart_handoff_path(spec)?;
    let handoff = read_openclaw_restart_handoff(&path);
    let _ = fs::remove_file(&path);
    handoff.filter(|payload| valid_openclaw_restart_handoff(payload, exited_pid, now_millis()))
}

fn queue_missing_children(
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
    running: &BTreeMap<String, RunningSupervisorChild>,
    desired_children: &[SupervisorChildSpec],
    restart_count: usize,
    retry_at: Instant,
) {
    for next_spec in desired_children {
        if running.contains_key(&next_spec.env_name) || pending.contains_key(&next_spec.env_name) {
            continue;
        }
        pending.insert(
            next_spec.env_name.clone(),
            PendingSupervisorChild {
                spec: next_spec.clone(),
                restart_count,
                quick_clean_restart_count: 0,
                retry_at,
                retry_at_utc: now_utc(),
                last_exit_code: None,
                last_error: None,
                last_event_at: None,
            },
        );
    }
}

fn start_due_children(
    running: &mut BTreeMap<String, RunningSupervisorChild>,
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
    inactive: &mut BTreeMap<String, InactiveSupervisorChild>,
) -> Result<bool, String> {
    let now = Instant::now();
    let due = pending
        .iter()
        .filter_map(|(env_name, child)| (child.retry_at <= now).then_some(env_name.clone()))
        .collect::<Vec<_>>();
    let mut runtime_dirty = false;

    for env_name in due {
        let Some(next_child) = pending.get(&env_name).cloned() else {
            continue;
        };
        if let Err(error) = preflight_supervisor_child_start(&next_child, running) {
            if let Some(entry) = pending.get_mut(&env_name) {
                entry.restart_count += 1;
                let delay_ms = restart_delay_ms(entry.restart_count);
                entry.retry_at = Instant::now() + Duration::from_millis(delay_ms);
                entry.retry_at_utc = now_utc() + time::Duration::milliseconds(delay_ms as i64);
                entry.last_error = Some(error.clone());
                entry.last_event_at = Some(now_utc());
            }
            inactive.insert(
                env_name.clone(),
                inactive_from_pending(
                    pending
                        .get(&env_name)
                        .expect("pending child should still exist after preflight failure"),
                    "backoff",
                ),
            );
            eprintln!("{error}");
            continue;
        }
        match spawn_running_child(
            next_child.spec.clone(),
            next_child.restart_count,
            next_child.quick_clean_restart_count,
        ) {
            Ok(running_child) => {
                pending.remove(&env_name);
                running.insert(env_name.clone(), running_child);
                inactive.remove(&env_name);
                runtime_dirty = true;
            }
            Err(error) => {
                eprintln!("{error}");
                if let Some(entry) = pending.get_mut(&env_name) {
                    entry.restart_count += 1;
                    let delay_ms = restart_delay_ms(entry.restart_count);
                    entry.retry_at = Instant::now() + Duration::from_millis(delay_ms);
                    entry.retry_at_utc = now_utc() + time::Duration::milliseconds(delay_ms as i64);
                    entry.last_error = Some(error.clone());
                    entry.last_event_at = Some(now_utc());
                }
                inactive.insert(
                    env_name.clone(),
                    inactive_from_pending(
                        pending
                            .get(&env_name)
                            .expect("pending child should still exist after spawn failure"),
                        "backoff",
                    ),
                );
            }
        }
    }

    Ok(runtime_dirty)
}

fn stop_supervisor_child(running_child: &mut RunningSupervisorChild) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", running_child.child.id());
        let _ = Command::new("kill")
            .args(["-TERM", "--", &process_group])
            .status();
        for _ in 0..20 {
            match running_child.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }
        let _ = Command::new("kill")
            .args(["-KILL", "--", &process_group])
            .status();
    }
    let _ = running_child.child.kill();
    let _ = running_child.child.wait();
}

fn supervisor_state_equivalent(left: &SupervisorState, right: &SupervisorState) -> bool {
    left.kind == right.kind
        && left.ocm_home == right.ocm_home
        && left.children == right.children
        && left.skipped_envs == right.skipped_envs
}

fn active_child_spec<'a>(
    state: &'a SupervisorState,
    env_name: &str,
) -> Option<&'a SupervisorChildSpec> {
    state
        .children
        .iter()
        .find(|child| child.env_name == env_name)
}

fn filter_conflicting_supervisor_children(
    children: Vec<SupervisorChildSpec>,
) -> (Vec<SupervisorChildSpec>, Vec<SkippedSupervisorEnv>) {
    let mut kept = Vec::new();
    let mut skipped = Vec::new();
    let mut claimed_run_dirs = BTreeMap::new();

    for child in children {
        if child.binding_kind != "runtime" {
            if let Some(existing_env) = claimed_run_dirs.get(&child.run_dir) {
                skipped.push(SkippedSupervisorEnv {
                    env_name: child.env_name,
                    reason: format!(
                        "source-backed run dir is already claimed by env \"{}\": {}",
                        existing_env, child.run_dir
                    ),
                });
                continue;
            }
            claimed_run_dirs.insert(child.run_dir.clone(), child.env_name.clone());
        }
        kept.push(child);
    }

    (kept, skipped)
}

fn pending_supervisor_child(
    spec: SupervisorChildSpec,
    restart_count: usize,
    quick_clean_restart_count: usize,
    delay_ms: u64,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
) -> PendingSupervisorChild {
    PendingSupervisorChild {
        spec,
        restart_count,
        quick_clean_restart_count,
        retry_at: Instant::now() + Duration::from_millis(delay_ms),
        retry_at_utc: now_utc() + time::Duration::milliseconds(delay_ms as i64),
        last_exit_code,
        last_error,
        last_event_at: Some(now_utc()),
    }
}

fn inactive_supervisor_child(
    spec: SupervisorChildSpec,
    gateway_state: &str,
    restart_count: usize,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
    last_event_at: OffsetDateTime,
    next_retry_at: Option<OffsetDateTime>,
) -> InactiveSupervisorChild {
    InactiveSupervisorChild {
        spec,
        gateway_state: gateway_state.to_string(),
        restart_count,
        last_exit_code,
        last_error,
        last_event_at: Some(last_event_at),
        next_retry_at,
    }
}

fn inactive_from_pending(
    pending_child: &PendingSupervisorChild,
    gateway_state: &str,
) -> InactiveSupervisorChild {
    InactiveSupervisorChild {
        spec: pending_child.spec.clone(),
        gateway_state: gateway_state.to_string(),
        restart_count: pending_child.restart_count,
        last_exit_code: pending_child.last_exit_code,
        last_error: pending_child.last_error.clone(),
        last_event_at: pending_child.last_event_at,
        next_retry_at: Some(pending_child.retry_at_utc),
    }
}

fn restart_delay_ms(restart_count: usize) -> u64 {
    let exponent = restart_count.saturating_sub(1).min(6) as u32;
    SUPERVISOR_RESTART_DELAY_MS
        .saturating_mul(2u64.saturating_pow(exponent))
        .min(SUPERVISOR_MAX_RESTART_DELAY_MS)
}

fn preflight_supervisor_child_start(
    pending_child: &PendingSupervisorChild,
    running: &BTreeMap<String, RunningSupervisorChild>,
) -> Result<(), String> {
    if !openclaw_port_family_available(pending_child.spec.child_port) {
        let (start_port, end_port) = openclaw_port_family_range(pending_child.spec.child_port);
        return Err(format!(
            "refusing to start env \"{}\": OpenClaw port family {}-{} is already in use",
            pending_child.spec.env_name, start_port, end_port
        ));
    }

    let has_run_dir_conflict = running.values().any(|running_child| {
        running_child.spec.run_dir == pending_child.spec.run_dir
            && running_child.spec.binding_kind != "runtime"
    });
    if has_run_dir_conflict {
        return Err(format!(
            "refusing to start env \"{}\": source-backed run dir is already active at {}",
            pending_child.spec.env_name, pending_child.spec.run_dir
        ));
    }

    Ok(())
}

fn supervisor_service_environment(
    process_env: &BTreeMap<String, String>,
    ocm_home: &Path,
    executable_path: &Path,
) -> BTreeMap<String, String> {
    let mut service_env = BTreeMap::new();
    if let Some(home) = process_env
        .get("HOME")
        .filter(|value| !value.trim().is_empty())
    {
        service_env.insert("HOME".to_string(), home.trim().to_string());
    }
    service_env.insert(
        "PATH".to_string(),
        process_env
            .get("PATH")
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.trim().to_string())
            .unwrap_or_else(|| DEFAULT_SERVICE_PATH.to_string()),
    );
    if let Some(tmpdir) = process_env
        .get("TMPDIR")
        .filter(|value| !value.trim().is_empty())
    {
        service_env.insert("TMPDIR".to_string(), tmpdir.trim().to_string());
    }
    for key in SERVICE_PROXY_ENV_KEYS {
        if let Some(value) = process_env
            .get(key)
            .filter(|value| !value.trim().is_empty())
        {
            service_env.insert(key.to_string(), value.trim().to_string());
        }
    }
    for key in SERVICE_EXTRA_ENV_KEYS {
        if let Some(value) = process_env
            .get(key)
            .filter(|value| !value.trim().is_empty())
        {
            service_env.insert(key.to_string(), value.trim().to_string());
        }
    }
    service_env.insert("OCM_HOME".to_string(), display_path(ocm_home));
    service_env.insert("OCM_SELF".to_string(), display_path(executable_path));
    service_env
}

fn build_supervised_openclaw_env(
    mut process_env: BTreeMap<String, String>,
    service_label: &str,
    service_manager: ServiceManagerKind,
) -> BTreeMap<String, String> {
    process_env.insert(
        "OPENCLAW_SERVICE_MARKER".to_string(),
        OPENCLAW_SERVICE_MARKER.to_string(),
    );
    process_env.insert(
        "OPENCLAW_SERVICE_KIND".to_string(),
        OPENCLAW_GATEWAY_SERVICE_KIND.to_string(),
    );
    match service_manager {
        ServiceManagerKind::Launchd => {
            process_env.insert(
                "OPENCLAW_LAUNCHD_LABEL".to_string(),
                service_label.to_string(),
            );
        }
        ServiceManagerKind::SystemdUser => {
            process_env.insert(
                "OPENCLAW_SYSTEMD_UNIT".to_string(),
                format!("{service_label}.service"),
            );
        }
        ServiceManagerKind::Unsupported => {}
    }
    process_env
}

fn write_supervisor_runtime_state(
    runtime_path: &Path,
    ocm_home: &str,
    running: &BTreeMap<String, RunningSupervisorChild>,
    pending: &BTreeMap<String, PendingSupervisorChild>,
    inactive: &BTreeMap<String, InactiveSupervisorChild>,
) -> Result<(), String> {
    if let Some(parent) = runtime_path.parent() {
        ensure_dir(parent)?;
    }

    let mut children = running
        .values()
        .map(supervisor_runtime_child)
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.env_name.cmp(&right.env_name));
    let mut services = running
        .values()
        .map(supervisor_runtime_service_running)
        .collect::<Vec<_>>();
    services.extend(pending.values().map(|child| {
        supervisor_runtime_service_inactive(&inactive_from_pending(child, "backoff"))
    }));
    services.extend(
        inactive
            .values()
            .filter(|child| !pending.contains_key(&child.spec.env_name))
            .map(supervisor_runtime_service_inactive),
    );
    services.sort_by(|left, right| left.env_name.cmp(&right.env_name));

    write_json(
        runtime_path,
        &SupervisorRuntimeState {
            kind: SUPERVISOR_RUNTIME_KIND.to_string(),
            ocm_home: ocm_home.to_string(),
            updated_at: now_utc(),
            services,
            children,
        },
    )
}

fn supervisor_runtime_child(running_child: &RunningSupervisorChild) -> SupervisorRuntimeChild {
    SupervisorRuntimeChild {
        env_name: running_child.spec.env_name.clone(),
        binding_kind: running_child.spec.binding_kind.clone(),
        binding_name: running_child.spec.binding_name.clone(),
        pid: running_child.child.id(),
        restart_count: running_child.restart_count,
        child_port: running_child.spec.child_port,
        stdout_path: running_child.spec.stdout_path.clone(),
        stderr_path: running_child.spec.stderr_path.clone(),
    }
}

fn supervisor_runtime_service_running(
    running_child: &RunningSupervisorChild,
) -> SupervisorRuntimeService {
    SupervisorRuntimeService {
        env_name: running_child.spec.env_name.clone(),
        binding_kind: running_child.spec.binding_kind.clone(),
        binding_name: running_child.spec.binding_name.clone(),
        gateway_state: "running".to_string(),
        restart_count: running_child.restart_count,
        child_port: running_child.spec.child_port,
        pid: Some(running_child.child.id()),
        stdout_path: running_child.spec.stdout_path.clone(),
        stderr_path: running_child.spec.stderr_path.clone(),
        last_exit_code: None,
        last_error: None,
        last_event_at: None,
        next_retry_at: None,
    }
}

fn supervisor_runtime_service_inactive(
    inactive_child: &InactiveSupervisorChild,
) -> SupervisorRuntimeService {
    SupervisorRuntimeService {
        env_name: inactive_child.spec.env_name.clone(),
        binding_kind: inactive_child.spec.binding_kind.clone(),
        binding_name: inactive_child.spec.binding_name.clone(),
        gateway_state: inactive_child.gateway_state.clone(),
        restart_count: inactive_child.restart_count,
        child_port: inactive_child.spec.child_port,
        pid: None,
        stdout_path: inactive_child.spec.stdout_path.clone(),
        stderr_path: inactive_child.spec.stderr_path.clone(),
        last_exit_code: inactive_child.last_exit_code,
        last_error: inactive_child.last_error.clone(),
        last_event_at: inactive_child.last_event_at,
        next_retry_at: inactive_child.next_retry_at,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exited_child(
        exit_code: Option<i32>,
        restart_count: usize,
        ran_for: Duration,
    ) -> ExitedSupervisorChild {
        exited_child_with_clean_count(exit_code, restart_count, 0, ran_for)
    }

    fn exited_child_with_clean_count(
        exit_code: Option<i32>,
        restart_count: usize,
        quick_clean_restart_count: usize,
        ran_for: Duration,
    ) -> ExitedSupervisorChild {
        exited_child_with_handoff(
            exit_code,
            restart_count,
            quick_clean_restart_count,
            None,
            ran_for,
        )
    }

    fn exited_child_with_handoff(
        exit_code: Option<i32>,
        restart_count: usize,
        quick_clean_restart_count: usize,
        restart_handoff: Option<OpenClawRestartHandoff>,
        ran_for: Duration,
    ) -> ExitedSupervisorChild {
        ExitedSupervisorChild {
            env_name: "demo".to_string(),
            exit_code,
            restart_count,
            quick_clean_restart_count,
            restart_handoff,
            ran_for,
        }
    }

    fn restart_handoff(pid: u64) -> OpenClawRestartHandoff {
        OpenClawRestartHandoff {
            kind: OPENCLAW_RESTART_HANDOFF_KIND.to_string(),
            version: 1,
            intent_id: "intent-123".to_string(),
            pid,
            created_at: 1_000,
            expires_at: 61_000,
            source: "plugin-change".to_string(),
            restart_kind: "full-process".to_string(),
            supervisor_mode: "launchd".to_string(),
        }
    }

    #[test]
    fn desired_quick_clean_exit_with_handoff_restarts() {
        let decision = exited_child_restart_decision(
            &exited_child_with_handoff(
                Some(0),
                0,
                0,
                Some(restart_handoff(1234)),
                Duration::from_millis(50),
            ),
            true,
            true,
        );

        assert!(decision.should_restart);
        assert_eq!(
            decision.log_action,
            "OpenClaw restart handoff, retrying after backoff"
        );
        assert!(
            decision
                .last_error
                .unwrap()
                .contains("supervisor restart handoff")
        );
    }

    #[test]
    fn desired_quick_clean_exit_without_handoff_stays_stopped() {
        let decision = exited_child_restart_decision(
            &exited_child(Some(0), 0, Duration::from_millis(50)),
            true,
            true,
        );

        assert!(!decision.should_restart);
        assert!(
            decision
                .last_error
                .unwrap()
                .contains("without OpenClaw restart handoff")
        );
    }

    #[test]
    fn quick_clean_exit_after_service_stop_stays_stopped() {
        let decision = exited_child_restart_decision(
            &exited_child(Some(0), 0, Duration::from_millis(50)),
            false,
            false,
        );

        assert!(!decision.should_restart);
        assert!(decision.last_error.unwrap().contains("after service stop"));
    }

    #[test]
    fn repeated_quick_clean_exit_stays_stopped() {
        let decision = exited_child_restart_decision(
            &exited_child_with_handoff(
                Some(0),
                1,
                1,
                Some(restart_handoff(1234)),
                Duration::from_millis(50),
            ),
            true,
            true,
        );

        assert!(!decision.should_restart);
        assert!(
            decision
                .last_error
                .unwrap()
                .contains("avoid a restart loop")
        );
    }

    #[test]
    fn previous_crash_retry_does_not_consume_clean_handoff_allowance() {
        let decision = exited_child_restart_decision(
            &exited_child_with_handoff(
                Some(0),
                3,
                0,
                Some(restart_handoff(1234)),
                Duration::from_millis(50),
            ),
            true,
            true,
        );

        assert!(decision.should_restart);
        assert!(decision.quick_clean_handoff);
    }

    #[test]
    fn nonzero_exit_still_restarts() {
        let decision = exited_child_restart_decision(
            &exited_child(Some(1), 0, Duration::from_millis(50)),
            true,
            true,
        );

        assert!(decision.should_restart);
        assert_eq!(
            decision.last_error.unwrap(),
            "process exited with 1; retrying after backoff"
        );
    }

    #[test]
    fn supervised_child_env_sets_openclaw_service_hints() {
        let process_env = build_supervised_openclaw_env(
            BTreeMap::new(),
            "ai.openclaw.ocm",
            ServiceManagerKind::Launchd,
        );

        assert_eq!(
            process_env
                .get("OPENCLAW_SERVICE_MARKER")
                .map(String::as_str),
            Some("openclaw")
        );
        assert_eq!(
            process_env.get("OPENCLAW_SERVICE_KIND").map(String::as_str),
            Some("gateway")
        );
        assert_eq!(
            process_env
                .get("OPENCLAW_LAUNCHD_LABEL")
                .map(String::as_str),
            Some("ai.openclaw.ocm")
        );
    }

    #[test]
    fn restart_handoff_validation_requires_matching_pid_and_supported_window() {
        assert!(valid_openclaw_restart_handoff(
            &restart_handoff(1234),
            1234,
            2_000
        ));

        assert!(!valid_openclaw_restart_handoff(
            &restart_handoff(4321),
            1234,
            2_000
        ));

        let mut long_ttl = restart_handoff(1234);
        long_ttl.expires_at = long_ttl.created_at + OPENCLAW_RESTART_HANDOFF_TTL_MS + 1;
        assert!(!valid_openclaw_restart_handoff(&long_ttl, 1234, 2_000));

        let mut missing_intent = restart_handoff(1234);
        missing_intent.intent_id.clear();
        assert!(!valid_openclaw_restart_handoff(
            &missing_intent,
            1234,
            2_000
        ));
    }
}
