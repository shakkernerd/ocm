use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[derive(Clone, Debug)]
pub struct EnvPaths {
    pub root: PathBuf,
    pub openclaw_home: PathBuf,
    pub state_dir: PathBuf,
    pub config_path: PathBuf,
    pub workspace_dir: PathBuf,
    pub marker_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvMeta {
    pub kind: String,
    pub name: String,
    pub root: String,
    pub gateway_port: Option<u32>,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSummary {
    pub name: String,
    pub root: String,
    pub openclaw_home: String,
    pub state_dir: String,
    pub config_path: String,
    pub workspace_dir: String,
    pub gateway_port: Option<u32>,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub forwarded_args: Vec<String>,
    pub run_dir: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvStatusSummary {
    pub env_name: String,
    pub root: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub resolved_kind: Option<String>,
    pub resolved_name: Option<String>,
    pub binary_path: Option<String>,
    pub command: Option<String>,
    pub run_dir: Option<String>,
    pub runtime_source_kind: Option<String>,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub runtime_health: Option<String>,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvDoctorSummary {
    pub env_name: String,
    pub root: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub healthy: bool,
    pub root_status: String,
    pub marker_status: String,
    pub runtime_status: String,
    pub launcher_status: String,
    pub resolution_status: String,
    pub resolved_kind: Option<String>,
    pub resolved_name: Option<String>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvMarkerRepairSummary {
    pub env_name: String,
    pub root: String,
    pub marker_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCleanupActionSummary {
    pub kind: String,
    pub description: String,
    pub applied: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCleanupSummary {
    pub env_name: String,
    pub root: String,
    pub apply: bool,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub healthy_before: bool,
    pub healthy_after: Option<bool>,
    pub actions: Vec<EnvCleanupActionSummary>,
    pub issues_before: Vec<String>,
    pub issues_after: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCleanupBatchSummary {
    pub apply: bool,
    pub count: usize,
    pub results: Vec<EnvCleanupSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvMarker {
    pub kind: String,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
pub struct StorePaths {
    pub home: PathBuf,
    pub envs_dir: PathBuf,
    pub launchers_dir: PathBuf,
    pub runtimes_dir: PathBuf,
    pub snapshots_dir: PathBuf,
}

#[derive(Clone, Debug)]
pub struct CreateEnvironmentOptions {
    pub name: String,
    pub root: Option<String>,
    pub gateway_port: Option<u32>,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
}

#[derive(Clone, Debug)]
pub struct CloneEnvironmentOptions {
    pub source_name: String,
    pub name: String,
    pub root: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ExportEnvironmentOptions {
    pub name: String,
    pub output: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvExportSummary {
    pub name: String,
    pub root: String,
    pub archive_path: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
}

#[derive(Clone, Debug)]
pub struct ImportEnvironmentOptions {
    pub archive: String,
    pub name: Option<String>,
    pub root: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvImportSummary {
    pub name: String,
    pub source_name: String,
    pub root: String,
    pub archive_path: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotMeta {
    pub kind: String,
    pub id: String,
    pub env_name: String,
    #[serde(default)]
    pub label: Option<String>,
    pub archive_path: String,
    pub source_root: String,
    pub gateway_port: Option<u32>,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotSummary {
    pub id: String,
    pub env_name: String,
    pub label: Option<String>,
    pub archive_path: String,
    pub source_root: String,
    pub gateway_port: Option<u32>,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotRestoreSummary {
    pub env_name: String,
    pub snapshot_id: String,
    pub label: Option<String>,
    pub root: String,
    pub archive_path: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotRemoveSummary {
    pub env_name: String,
    pub snapshot_id: String,
    pub label: Option<String>,
    pub archive_path: String,
}

#[derive(Clone, Debug)]
pub struct CreateEnvSnapshotOptions {
    pub env_name: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct RestoreEnvSnapshotOptions {
    pub env_name: String,
    pub snapshot_id: String,
}

#[derive(Clone, Debug)]
pub struct RemoveEnvSnapshotOptions {
    pub env_name: String,
    pub snapshot_id: String,
}
