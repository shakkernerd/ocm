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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMeta {
    pub kind: String,
    pub name: String,
    pub binary_path: String,
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
