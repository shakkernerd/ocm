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
pub struct RuntimeVerifySummary {
    pub name: String,
    pub binary_path: String,
    pub source_kind: String,
    pub source_path: Option<String>,
    pub source_url: Option<String>,
    pub source_manifest_url: Option<String>,
    pub source_sha256: Option<String>,
    pub release_version: Option<String>,
    pub release_channel: Option<String>,
    pub install_root: Option<String>,
    pub healthy: bool,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUpdateSummary {
    pub name: String,
    pub outcome: String,
    pub binary_path: Option<String>,
    pub source_kind: String,
    pub release_version: Option<String>,
    pub release_channel: Option<String>,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBinarySummary {
    pub name: String,
    pub binary_path: String,
    pub source_kind: String,
    pub release_version: Option<String>,
    pub release_channel: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReleaseManifest {
    #[serde(default)]
    pub kind: Option<String>,
    pub releases: Vec<RuntimeRelease>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRelease {
    pub version: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherMeta {
    pub kind: String,
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeSourceKind {
    Registered,
    Installed,
}

impl RuntimeSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::Installed => "installed",
        }
    }
}

fn default_runtime_source_kind() -> RuntimeSourceKind {
    RuntimeSourceKind::Registered
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeReleaseSelectorKind {
    Version,
    Channel,
}

impl RuntimeReleaseSelectorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Channel => "channel",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMeta {
    pub kind: String,
    pub name: String,
    pub binary_path: String,
    #[serde(default = "default_runtime_source_kind")]
    pub source_kind: RuntimeSourceKind,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_manifest_url: Option<String>,
    #[serde(default)]
    pub source_sha256: Option<String>,
    #[serde(default)]
    pub release_version: Option<String>,
    #[serde(default)]
    pub release_channel: Option<String>,
    #[serde(default)]
    pub release_selector_kind: Option<RuntimeReleaseSelectorKind>,
    #[serde(default)]
    pub release_selector_value: Option<String>,
    #[serde(default)]
    pub install_root: Option<String>,
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
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
pub struct AddLauncherOptions {
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct AddRuntimeOptions {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug)]
pub struct InstallRuntimeOptions {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct InstallRuntimeFromUrlOptions {
    pub name: String,
    pub url: String,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct InstallRuntimeFromReleaseOptions {
    pub name: String,
    pub manifest_url: String,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct UpdateRuntimeFromReleaseOptions {
    pub name: String,
    pub version: Option<String>,
    pub channel: Option<String>,
}
