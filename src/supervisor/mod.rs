use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::env::EnvironmentService;
use crate::store::{
    derive_env_paths, display_path, ensure_dir, ensure_store, list_environments, now_utc,
    read_json, resolve_ocm_home, supervisor_logs_dir, supervisor_state_path, write_json,
};

const SUPERVISOR_STATE_KIND: &str = "ocm-supervisor-state";
const DEFAULT_START_MODE: &str = "on-demand";

#[derive(Clone, Debug, Serialize, Deserialize)]
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
    pub env_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

pub struct SupervisorService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
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
        let state_path = supervisor_state_path(self.env, self.cwd)?;
        if !state_path.exists() {
            return Err(
                "supervisor state has not been synced yet; run \"ocm supervisor sync\"".to_string(),
            );
        }
        let state: SupervisorState = read_json(&state_path)?;
        Ok(view_from_state(&state_path, true, state))
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
                        env_overrides: process_env_overrides(&process.process_env, self.env),
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
}

fn process_env_overrides(
    process_env: &BTreeMap<String, String>,
    base_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    process_env
        .iter()
        .filter_map(|(key, value)| match base_env.get(key) {
            Some(existing) if existing == value => None,
            _ => Some((key.clone(), value.clone())),
        })
        .collect()
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
