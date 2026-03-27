use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

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

pub use crate::env::{
    EnvCleanupActionSummary, EnvCleanupBatchSummary, EnvCleanupSummary, EnvDoctorSummary,
    EnvMarkerRepairSummary, EnvStatusSummary, ExecutionSummary,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvMarker {
    pub kind: String,
    pub name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
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
