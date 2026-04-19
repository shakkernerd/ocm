use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::env::EnvironmentService;
use crate::service::inspect::inspect_job;
use crate::service::platform::{
    ManagedServiceDefinition, activate_managed_service, managed_service_identity,
    write_managed_service_definition,
};
use crate::store::{
    display_path, ensure_dir, ensure_store, list_environments, now_utc, read_json,
    resolve_ocm_home, supervisor_logs_dir, supervisor_runtime_path, supervisor_state_path,
    write_json,
};

const SUPERVISOR_STATE_KIND: &str = "ocm-supervisor-state";
const SUPERVISOR_RUNTIME_KIND: &str = "ocm-supervisor-runtime";
const DAEMON_SERVICE_NAME: &str = "ocm";
const SUPERVISOR_POLL_INTERVAL_MS: u64 = 200;
const SUPERVISOR_RESTART_DELAY_MS: u64 = 400;
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
    pub children: Vec<SupervisorRuntimeChild>,
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
        let runtime_children = if daemon.running {
            self.runtime()?.children
        } else {
            Vec::new()
        };

        Ok(SupervisorInspection {
            daemon,
            planned_children: state.children,
            skipped_envs: state.skipped_envs,
            runtime_children,
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
        self.install_daemon()
    }

    pub fn daemon_status(&self) -> Result<SupervisorDaemonSummary, String> {
        self.daemon_summary("status")
    }
    fn build_state(&self) -> Result<SupervisorState, String> {
        ensure_store(self.env, self.cwd)?;
        let ocm_home = resolve_ocm_home(self.env, self.cwd)?;
        let logs_dir = supervisor_logs_dir(self.env, self.cwd)?;
        let env_service = EnvironmentService::new(self.env, self.cwd);
        let mut envs = list_environments(self.env, self.cwd)?;
        envs.sort_by(|left, right| left.name.cmp(&right.name));

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
            match env_service.resolve_gateway_process(&name, false) {
                Ok(process) => {
                    let args = process.args.clone();
                    let child_port = process
                        .process_env
                        .get("OPENCLAW_GATEWAY_PORT")
                        .ok_or_else(|| format!("failed to resolve child port for env \"{name}\""))?
                        .parse::<u32>()
                        .map_err(|_| format!("failed to parse child port for env \"{name}\""))?;
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
                        process_env: process.process_env,
                    });
                }
                Err(reason) => skipped_envs.push(SkippedSupervisorEnv {
                    env_name: name,
                    reason,
                }),
            }
        }

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
        let definition = self.supervisor_daemon_definition()?;
        write_managed_service_definition(&definition, self.env)?;
        activate_managed_service(&definition.label, &definition.definition_path, self.env)?;
        self.daemon_summary(action)
    }

    fn daemon_summary(&self, action: &str) -> Result<SupervisorDaemonSummary, String> {
        ensure_store(self.env, self.cwd)?;
        let ocm_home = resolve_ocm_home(self.env, self.cwd)?;
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        let identity = managed_service_identity(DAEMON_SERVICE_NAME, self.env, self.cwd)?;
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
        let identity = managed_service_identity(DAEMON_SERVICE_NAME, self.env, self.cwd)?;
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
        queue_missing_children(
            &mut pending,
            &running,
            &active_state.children,
            0,
            Instant::now(),
        );
        start_due_children(&mut running, &mut pending)?;
        write_supervisor_runtime_state(&runtime_path, &active_state.ocm_home, &running)?;
        let mut managed_child_count = active_state.children.len();
        let mut child_results = Vec::new();

        while !stop_requested.load(Ordering::SeqCst) {
            let mut runtime_dirty = refresh_active_state(
                state_path,
                &mut active_state,
                &mut managed_child_count,
                &mut running,
                &mut pending,
            );
            runtime_dirty |= process_exited_children(
                state_path,
                &stop_requested,
                &mut active_state,
                &mut managed_child_count,
                &mut running,
                &mut pending,
                &mut child_results,
            )?;

            runtime_dirty |= start_due_children(&mut running, &mut pending)?;
            if runtime_dirty {
                write_supervisor_runtime_state(&runtime_path, &active_state.ocm_home, &running)?;
            }

            if !stop_requested.load(Ordering::SeqCst) {
                sleep(Duration::from_millis(SUPERVISOR_POLL_INTERVAL_MS));
            }
        }

        for (_, mut running_child) in running {
            stop_supervisor_child(&mut running_child);
        }
        write_supervisor_runtime_state(&runtime_path, &active_state.ocm_home, &BTreeMap::new())?;

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
}

#[derive(Clone)]
struct PendingSupervisorChild {
    spec: SupervisorChildSpec,
    restart_count: usize,
    retry_at: Instant,
}

struct ExitedSupervisorChild {
    env_name: String,
    exit_code: Option<i32>,
    restart_count: usize,
}

fn spawn_running_child(
    spec: SupervisorChildSpec,
    restart_count: usize,
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

    Command::new(program)
        .args(program_arguments.iter().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .env_clear()
        .envs(&spec.process_env)
        .current_dir(Path::new(&spec.run_dir))
        .spawn()
        .map_err(|error| {
            format!(
                "failed starting env \"{}\" with {}: {error}",
                spec.env_name,
                child_binding_label(spec)
            )
        })
}

fn supervisor_program_arguments(spec: &SupervisorChildSpec) -> Vec<String> {
    match (&spec.command, &spec.binary_path) {
        (Some(command), _) => {
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
        (None, Some(binary_path)) => {
            let mut program_arguments = vec![binary_path.clone()];
            program_arguments.extend(spec.args.iter().cloned());
            program_arguments
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
) -> bool {
    let Some(next_state) = read_updated_supervisor_state(state_path, active_state) else {
        return false;
    };
    let runtime_dirty = reconcile_running_children(running, pending, &next_state);
    *managed_child_count = next_state.children.len();
    *active_state = next_state;
    runtime_dirty
}

fn reconcile_running_children(
    running: &mut BTreeMap<String, RunningSupervisorChild>,
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
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
                        PendingSupervisorChild {
                            spec: next_spec.clone(),
                            restart_count: 0,
                            retry_at: Instant::now(),
                        },
                    );
                }
            }
            None => {
                pending.remove(&env_name);
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
                PendingSupervisorChild {
                    spec: next_spec.clone(),
                    restart_count: 0,
                    retry_at: Instant::now(),
                },
            );
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
            exited.push(ExitedSupervisorChild {
                env_name: env_name.clone(),
                exit_code: status.code(),
                restart_count: running_child.restart_count,
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
    child_results: &mut Vec<SupervisorChildRunResult>,
) -> Result<bool, String> {
    let exited = collect_exited_children(running)?;
    let mut runtime_dirty = false;

    for exited_child in exited {
        let Some(previous_child) = running.remove(&exited_child.env_name) else {
            continue;
        };
        runtime_dirty = true;
        eprintln!(
            "ocm service: {} exited with {}; restarting",
            previous_child.spec.env_name,
            exited_child
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string())
        );
        child_results.push(child_run_result(
            &previous_child.spec,
            exited_child.exit_code,
            exited_child.restart_count,
        ));
        if stop_requested.load(Ordering::SeqCst) {
            break;
        }

        runtime_dirty |= refresh_active_state(
            state_path,
            active_state,
            managed_child_count,
            running,
            pending,
        );
        if let Some(next_spec) = active_child_spec(active_state, &exited_child.env_name).cloned() {
            let next_restart_count = if next_spec == previous_child.spec {
                exited_child.restart_count + 1
            } else {
                0
            };
            pending.insert(
                exited_child.env_name,
                PendingSupervisorChild {
                    spec: next_spec,
                    restart_count: next_restart_count,
                    retry_at: Instant::now() + Duration::from_millis(SUPERVISOR_RESTART_DELAY_MS),
                },
            );
        }
    }

    Ok(runtime_dirty)
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
        eprintln!(
            "ocm service: starting new env {} ({})",
            next_spec.env_name,
            child_binding_label(next_spec)
        );
        pending.insert(
            next_spec.env_name.clone(),
            PendingSupervisorChild {
                spec: next_spec.clone(),
                restart_count,
                retry_at,
            },
        );
    }
}

fn start_due_children(
    running: &mut BTreeMap<String, RunningSupervisorChild>,
    pending: &mut BTreeMap<String, PendingSupervisorChild>,
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
        match spawn_running_child(next_child.spec.clone(), next_child.restart_count) {
            Ok(running_child) => {
                pending.remove(&env_name);
                running.insert(env_name, running_child);
                runtime_dirty = true;
            }
            Err(error) => {
                eprintln!("{error}");
                if let Some(entry) = pending.get_mut(&env_name) {
                    entry.retry_at =
                        Instant::now() + Duration::from_millis(SUPERVISOR_RESTART_DELAY_MS);
                }
            }
        }
    }

    Ok(runtime_dirty)
}

fn stop_supervisor_child(running_child: &mut RunningSupervisorChild) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-TERM", &running_child.child.id().to_string()])
            .status();
        for _ in 0..20 {
            match running_child.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => sleep(Duration::from_millis(50)),
                Err(_) => break,
            }
        }
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

fn write_supervisor_runtime_state(
    runtime_path: &Path,
    ocm_home: &str,
    running: &BTreeMap<String, RunningSupervisorChild>,
) -> Result<(), String> {
    if let Some(parent) = runtime_path.parent() {
        ensure_dir(parent)?;
    }

    let mut children = running
        .values()
        .map(supervisor_runtime_child)
        .collect::<Vec<_>>();
    children.sort_by(|left, right| left.env_name.cmp(&right.env_name));

    write_json(
        runtime_path,
        &SupervisorRuntimeState {
            kind: SUPERVISOR_RUNTIME_KIND.to_string(),
            ocm_home: ocm_home.to_string(),
            updated_at: now_utc(),
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
