use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::sleep;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::env::EnvironmentService;
use crate::store::{
    derive_env_paths, display_path, ensure_dir, ensure_store, list_environments, now_utc,
    read_json, resolve_ocm_home, supervisor_logs_dir, supervisor_state_path, write_json,
};

const SUPERVISOR_STATE_KIND: &str = "ocm-supervisor-state";
const DEFAULT_START_MODE: &str = "on-demand";
const SUPERVISOR_POLL_INTERVAL_MS: u64 = 200;
const SUPERVISOR_RESTART_DELAY_MS: u64 = 400;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupervisorChildSpec {
    pub env_name: String,
    pub start_mode: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub runtime_source_kind: Option<String>,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub args: Vec<String>,
    pub program_arguments: Vec<String>,
    pub run_dir: String,
    pub child_port: u32,
    pub openclaw_home: String,
    pub openclaw_state_dir: String,
    pub openclaw_config_path: String,
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
pub struct SupervisorStatusSummary {
    pub state_path: String,
    pub state_present: bool,
    pub in_sync: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub planned_generated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub persisted_generated_at: Option<OffsetDateTime>,
    pub planned_child_count: usize,
    pub persisted_child_count: usize,
    pub planned_skipped_env_count: usize,
    pub persisted_skipped_env_count: usize,
    pub missing_children: Vec<String>,
    pub extra_children: Vec<String>,
    pub changed_children: Vec<String>,
    pub skipped_env_changes: Vec<String>,
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

pub struct SupervisorService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

pub fn sync_supervisor_if_present(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<bool, String> {
    let state_path = supervisor_state_path(env, cwd)?;
    if !state_path.exists() {
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

    pub fn show(&self) -> Result<SupervisorView, String> {
        let (state_path, state) = self.read_persisted_state()?;
        Ok(view_from_state(&state_path, true, state))
    }

    pub fn run(&self, once: bool) -> Result<SupervisorRunSummary, String> {
        let (state_path, state) = self.read_persisted_state()?;
        if once {
            return self.run_once(&state_path, state);
        }
        self.run_until_stopped(&state_path, state)
    }

    pub fn status(&self) -> Result<SupervisorStatusSummary, String> {
        let planned = self.build_state()?;
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        let persisted = if state_path.exists() {
            Some(read_json::<SupervisorState>(&state_path)?)
        } else {
            None
        };

        let planned_children = child_map(&planned.children);
        let planned_skipped = skipped_map(&planned.skipped_envs);
        let persisted_children = persisted
            .as_ref()
            .map(|state| child_map(&state.children))
            .unwrap_or_default();
        let persisted_skipped = persisted
            .as_ref()
            .map(|state| skipped_map(&state.skipped_envs))
            .unwrap_or_default();

        let mut missing_children = planned_children
            .keys()
            .filter(|name| !persisted_children.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        let mut extra_children = persisted_children
            .keys()
            .filter(|name| !planned_children.contains_key(*name))
            .cloned()
            .collect::<Vec<_>>();
        let mut changed_children = planned_children
            .iter()
            .filter_map(|(name, child)| match persisted_children.get(name) {
                Some(existing) if existing == child => None,
                Some(_) => Some(name.clone()),
                None => None,
            })
            .collect::<Vec<_>>();
        let mut skipped_env_changes = planned_skipped
            .iter()
            .filter_map(|(name, reason)| match persisted_skipped.get(name) {
                Some(existing) if existing == reason => None,
                _ => Some(name.clone()),
            })
            .collect::<Vec<_>>();
        skipped_env_changes.extend(
            persisted_skipped
                .keys()
                .filter(|name| !planned_skipped.contains_key(*name))
                .cloned(),
        );

        missing_children.sort();
        extra_children.sort();
        changed_children.sort();
        skipped_env_changes.sort();
        skipped_env_changes.dedup();

        Ok(SupervisorStatusSummary {
            state_path: display_path(&state_path),
            state_present: persisted.is_some(),
            in_sync: persisted.is_some()
                && missing_children.is_empty()
                && extra_children.is_empty()
                && changed_children.is_empty()
                && skipped_env_changes.is_empty(),
            planned_generated_at: planned.generated_at,
            persisted_generated_at: persisted.as_ref().map(|state| state.generated_at),
            planned_child_count: planned.children.len(),
            persisted_child_count: persisted
                .as_ref()
                .map(|state| state.children.len())
                .unwrap_or(0),
            planned_skipped_env_count: planned.skipped_envs.len(),
            persisted_skipped_env_count: persisted
                .as_ref()
                .map(|state| state.skipped_envs.len())
                .unwrap_or(0),
            missing_children,
            extra_children,
            changed_children,
            skipped_env_changes,
        })
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
            match env_service.resolve_gateway_process(&name, false) {
                Ok(process) => {
                    let paths = derive_env_paths(Path::new(&env_meta.root));
                    let program_arguments = process.program_arguments();
                    let args = process.args.clone();
                    let child_port = process
                        .process_env
                        .get("OPENCLAW_GATEWAY_PORT")
                        .ok_or_else(|| format!("failed to resolve child port for env \"{name}\""))?
                        .parse::<u32>()
                        .map_err(|_| format!("failed to parse child port for env \"{name}\""))?;
                    children.push(SupervisorChildSpec {
                        env_name: name,
                        start_mode: DEFAULT_START_MODE.to_string(),
                        binding_kind: process.binding_kind,
                        binding_name: process.binding_name,
                        command: process.command,
                        binary_path: process.binary_path,
                        runtime_source_kind: process.runtime_source_kind,
                        runtime_release_version: process.runtime_release_version,
                        runtime_release_channel: process.runtime_release_channel,
                        args,
                        program_arguments,
                        run_dir: display_path(&process.run_dir),
                        child_port,
                        openclaw_home: display_path(&paths.openclaw_home),
                        openclaw_state_dir: display_path(&paths.state_dir),
                        openclaw_config_path: display_path(&paths.config_path),
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
                "supervisor state has not been synced yet; run \"ocm supervisor sync\"".to_string(),
            );
        }
        let state = read_json(&state_path)?;
        Ok((state_path, state))
    }

    fn run_once(
        &self,
        state_path: &Path,
        state: SupervisorState,
    ) -> Result<SupervisorRunSummary, String> {
        let mut child_results = Vec::with_capacity(state.children.len());
        for spec in state.children {
            eprintln!(
                "ocm supervisor: starting {} ({})",
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
        .map_err(|error| format!("failed to install supervisor signal handler: {error}"))?;

        let mut running = state
            .children
            .into_iter()
            .map(|spec| spawn_running_child(spec, 0))
            .collect::<Result<Vec<_>, _>>()?;
        let managed_child_count = running.len();
        let mut child_results = Vec::new();

        while !stop_requested.load(Ordering::SeqCst) {
            for running_child in &mut running {
                if let Some(status) = running_child.child.try_wait().map_err(|error| {
                    format!(
                        "failed to poll env \"{}\": {error}",
                        running_child.spec.env_name
                    )
                })? {
                    let exit_code = status.code();
                    eprintln!(
                        "ocm supervisor: {} exited with {}; restarting",
                        running_child.spec.env_name,
                        exit_code
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "signal".to_string())
                    );
                    child_results.push(child_run_result(
                        &running_child.spec,
                        exit_code,
                        running_child.restart_count,
                    ));
                    sleep(Duration::from_millis(SUPERVISOR_RESTART_DELAY_MS));
                    if stop_requested.load(Ordering::SeqCst) {
                        break;
                    }
                    *running_child = spawn_running_child(
                        running_child.spec.clone(),
                        running_child.restart_count + 1,
                    )?;
                }
            }
            if !stop_requested.load(Ordering::SeqCst) {
                sleep(Duration::from_millis(SUPERVISOR_POLL_INTERVAL_MS));
            }
        }

        for mut running_child in running {
            let _ = running_child.child.kill();
            let _ = running_child.child.wait();
        }

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

fn spawn_running_child(
    spec: SupervisorChildSpec,
    restart_count: usize,
) -> Result<RunningSupervisorChild, String> {
    eprintln!(
        "ocm supervisor: starting {} ({})",
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
    let Some(program) = spec.program_arguments.first() else {
        return Err(format!(
            "supervisor child env \"{}\" is missing program arguments",
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
        .args(spec.program_arguments.iter().skip(1))
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

fn skipped_map(skipped_envs: &[SkippedSupervisorEnv]) -> BTreeMap<String, String> {
    skipped_envs
        .iter()
        .map(|skipped| (skipped.env_name.clone(), skipped.reason.clone()))
        .collect()
}
