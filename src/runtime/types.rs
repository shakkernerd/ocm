use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

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
pub struct RuntimeUpdateBatchSummary {
    pub count: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub results: Vec<RuntimeUpdateSummary>,
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
