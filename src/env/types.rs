use serde::Serialize;

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
