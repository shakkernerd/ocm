mod binding;
mod execution;
mod health;
mod inspect;
mod lifecycle;
mod snapshots;
mod types;

use std::collections::BTreeMap;
use std::path::Path;

pub use execution::{
    ExecutionBinding, ResolvedExecution, resolve_execution_binding, resolve_runtime_run_dir,
};
pub use types::{
    EnvCleanupActionSummary, EnvCleanupBatchSummary, EnvCleanupSummary, EnvDoctorSummary, EnvMarker,
    EnvMarkerRepairSummary, EnvMeta, EnvStatusSummary, EnvSummary, ExecutionSummary,
};

pub struct EnvironmentService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> EnvironmentService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }
}
