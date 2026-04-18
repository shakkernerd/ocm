mod binding;
mod execution;
mod health;
mod inspect;
mod lifecycle;
mod snapshots;

use std::collections::BTreeMap;
use std::path::Path;

pub use execution::ExecutionSummary;
pub use execution::{
    ExecutionBinding, GatewayProcessSpec, ResolvedExecution, resolve_execution_binding,
    resolve_gateway_process_spec, resolve_runtime_run_dir,
};
pub use health::{
    EnvCleanupActionSummary, EnvCleanupBatchSummary, EnvCleanupSummary, EnvDoctorSummary,
    EnvMarkerRepairSummary,
};
pub use inspect::EnvStatusSummary;
pub use lifecycle::{
    CloneEnvironmentOptions, CreateEnvironmentOptions, EnvExportSummary, EnvImportSummary,
    EnvMarker, EnvMeta, EnvSummary, ExportEnvironmentOptions, ImportEnvironmentOptions,
    default_service_enabled, default_service_running, select_prune_candidates,
};
pub use snapshots::{
    CreateEnvSnapshotOptions, EnvSnapshotRemoveSummary, EnvSnapshotRestoreSummary,
    EnvSnapshotSummary, RemoveEnvSnapshotOptions, RestoreEnvSnapshotOptions,
    select_snapshot_prune_candidates,
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
