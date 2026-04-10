use std::collections::BTreeMap;

use serde::Serialize;

use crate::manifest::{
    ManifestApplyPlan, ManifestReconcileSummary, ManifestServiceState, OcmManifest,
};

use super::{RenderProfile, format_key_value_lines};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestPathSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestShowSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
    pub manifest: Option<OcmManifest>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestResolveSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
    pub env_name: Option<String>,
    pub env_exists: bool,
    pub env_root: Option<String>,
    pub current_runtime: Option<String>,
    pub current_launcher: Option<String>,
    pub current_service_installed: bool,
    pub current_service: Option<ManifestServiceState>,
    pub desired_runtime: Option<String>,
    pub desired_launcher: Option<String>,
    pub desired_service_install: Option<bool>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestDriftSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
    pub env_name: Option<String>,
    pub env_exists: bool,
    pub current_runtime: Option<String>,
    pub current_launcher: Option<String>,
    pub current_service_installed: bool,
    pub current_service: Option<ManifestServiceState>,
    pub desired_runtime: Option<String>,
    pub desired_launcher: Option<String>,
    pub aligned: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestPlanSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
    pub env_exists: bool,
    pub env_root: Option<String>,
    pub plan: Option<ManifestApplyPlan>,
}

#[derive(Debug, Serialize)]
pub struct UpSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
    pub dry_run: bool,
    pub env_exists: bool,
    pub env_root: Option<String>,
    pub plan: Option<ManifestApplyPlan>,
    pub result: Option<ManifestReconcileSummary>,
}

pub fn manifest_path(summary: &ManifestPathSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_path_pretty(summary);
    }
    manifest_path_raw(summary)
}

pub fn up_summary(summary: &UpSummary, profile: RenderProfile) -> Vec<String> {
    manifest_apply_summary("Manifest up", summary, profile)
}

pub fn sync_summary(summary: &UpSummary, profile: RenderProfile) -> Vec<String> {
    manifest_apply_summary("Manifest sync", summary, profile)
}

fn manifest_apply_summary(title: &str, summary: &UpSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return up_summary_pretty(title, summary);
    }
    up_summary_raw(summary)
}

fn up_summary_pretty(title: &str, summary: &UpSummary) -> Vec<String> {
    if summary.dry_run {
        let Some(plan) = summary.plan.as_ref() else {
            return vec![
                title.to_string(),
                String::new(),
                format!("No ocm.yaml found from {}", summary.search_root),
            ];
        };

        return vec![
            title.to_string(),
            String::new(),
            format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
            format!(
                "Mode: {}",
                if summary.dry_run { "dry-run" } else { "apply" }
            ),
            format!("Env: {}", plan.env_name),
            format!("Create env: {}", plan.create_env),
            format!(
                "Desired runtime: {}",
                plan.desired_runtime.as_deref().unwrap_or("none")
            ),
            format!(
                "Desired launcher: {}",
                plan.desired_launcher.as_deref().unwrap_or("none")
            ),
            format!(
                "Desired service install: {}",
                plan.desired_service_install
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string())
            ),
            format!(
                "Current service: {}",
                plan.current_service
                    .as_ref()
                    .map(manifest_service_state_label)
                    .unwrap_or_else(|| "none".to_string())
            ),
            format!("Service changed: {}", plan.service_changed),
        ];
    }

    let Some(result) = summary.result.as_ref() else {
        return vec![
            title.to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ];
    };

    vec![
        title.to_string(),
        String::new(),
        format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
        "Mode: apply".to_string(),
        format!("Env: {}", result.env_name),
        format!("Env root: {}", result.env_root),
        format!("Env created: {}", result.env_created),
        format!(
            "Snapshot: {}",
            result.snapshot_id.as_deref().unwrap_or("none")
        ),
        format!("Runtime changed: {}", result.runtime_changed),
        format!("Launcher changed: {}", result.launcher_changed),
        format!("Service changed: {}", result.service_changed),
        format!("Service installed: {}", result.service_installed),
    ]
}

fn up_summary_raw(summary: &UpSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    lines.insert("dryRun".to_string(), summary.dry_run.to_string());
    lines.insert("envExists".to_string(), summary.env_exists.to_string());
    lines.insert(
        "envRoot".to_string(),
        summary
            .env_root
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );

    if let Some(plan) = summary.plan.as_ref() {
        lines.insert("env".to_string(), plan.env_name.clone());
        lines.insert("createEnv".to_string(), plan.create_env.to_string());
        lines.insert(
            "desiredRuntime".to_string(),
            plan.desired_runtime
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        );
        lines.insert(
            "desiredLauncher".to_string(),
            plan.desired_launcher
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        );
        lines.insert(
            "desiredServiceInstall".to_string(),
            plan.desired_service_install
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
        );
        if let Some(service) = plan.current_service.as_ref() {
            lines.insert(
                "currentServiceInstalled".to_string(),
                service.installed.to_string(),
            );
            lines.insert(
                "currentServiceLoaded".to_string(),
                service.loaded.to_string(),
            );
            lines.insert(
                "currentServiceRunning".to_string(),
                service.running.to_string(),
            );
            lines.insert(
                "currentServiceDefinitionDrift".to_string(),
                service.definition_drift.to_string(),
            );
            lines.insert(
                "currentServiceLiveExecUnverified".to_string(),
                service.live_exec_unverified.to_string(),
            );
            lines.insert(
                "currentServiceOrphanedLive".to_string(),
                service.orphaned_live_service.to_string(),
            );
        }
        lines.insert(
            "serviceChanged".to_string(),
            plan.service_changed.to_string(),
        );
    }

    if let Some(result) = summary.result.as_ref() {
        lines.insert("env".to_string(), result.env_name.clone());
        lines.insert("envCreated".to_string(), result.env_created.to_string());
        lines.insert(
            "snapshotId".to_string(),
            result
                .snapshot_id
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        );
        lines.insert(
            "runtimeChanged".to_string(),
            result.runtime_changed.to_string(),
        );
        lines.insert(
            "launcherChanged".to_string(),
            result.launcher_changed.to_string(),
        );
        lines.insert(
            "serviceChanged".to_string(),
            result.service_changed.to_string(),
        );
        lines.insert(
            "serviceInstalled".to_string(),
            result.service_installed.to_string(),
        );
    }

    format_key_value_lines(lines)
}

fn manifest_path_pretty(summary: &ManifestPathSummary) -> Vec<String> {
    if let Some(path) = summary.path.as_deref() {
        vec!["Manifest path".to_string(), String::new(), path.to_string()]
    } else {
        vec![
            "Manifest path".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ]
    }
}

fn manifest_path_raw(summary: &ManifestPathSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    format_key_value_lines(lines)
}

pub fn manifest_show(summary: &ManifestShowSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_show_pretty(summary);
    }
    manifest_show_raw(summary)
}

fn manifest_show_pretty(summary: &ManifestShowSummary) -> Vec<String> {
    let Some(manifest) = summary.manifest.as_ref() else {
        return vec![
            "Manifest".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ];
    };

    let runtime = manifest
        .runtime
        .as_ref()
        .and_then(|runtime| {
            runtime
                .name
                .as_deref()
                .or(runtime.version.as_deref())
                .or(runtime.channel.as_deref())
        })
        .unwrap_or("none");
    let launcher = manifest
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.name.as_deref())
        .unwrap_or("none");
    let service_install = manifest
        .service
        .as_ref()
        .and_then(|service| service.install)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());

    vec![
        "Manifest".to_string(),
        String::new(),
        format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
        format!("Schema: {}", manifest.schema),
        format!("Env: {}", manifest.env.name),
        format!("Runtime: {runtime}"),
        format!("Launcher: {launcher}"),
        format!("Service install: {service_install}"),
    ]
}

fn manifest_show_raw(summary: &ManifestShowSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    if let Some(manifest) = summary.manifest.as_ref() {
        lines.insert("schema".to_string(), manifest.schema.clone());
        lines.insert("env".to_string(), manifest.env.name.clone());
        lines.insert(
            "runtime".to_string(),
            manifest
                .runtime
                .as_ref()
                .and_then(|runtime| {
                    runtime
                        .name
                        .as_deref()
                        .or(runtime.version.as_deref())
                        .or(runtime.channel.as_deref())
                })
                .unwrap_or("none")
                .to_string(),
        );
        lines.insert(
            "launcher".to_string(),
            manifest
                .launcher
                .as_ref()
                .and_then(|launcher| launcher.name.as_deref())
                .unwrap_or("none")
                .to_string(),
        );
        lines.insert(
            "serviceInstall".to_string(),
            manifest
                .service
                .as_ref()
                .and_then(|service| service.install)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
        );
    }
    format_key_value_lines(lines)
}

pub fn manifest_resolve(summary: &ManifestResolveSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_resolve_pretty(summary);
    }
    manifest_resolve_raw(summary)
}

fn manifest_resolve_pretty(summary: &ManifestResolveSummary) -> Vec<String> {
    let Some(env_name) = summary.env_name.as_deref() else {
        return vec![
            "Manifest resolution".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ];
    };

    vec![
        "Manifest resolution".to_string(),
        String::new(),
        format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
        format!("Env: {env_name}"),
        format!("Env exists: {}", summary.env_exists),
        format!(
            "Env root: {}",
            summary.env_root.as_deref().unwrap_or("none")
        ),
        format!(
            "Current runtime: {}",
            summary.current_runtime.as_deref().unwrap_or("none")
        ),
        format!(
            "Current launcher: {}",
            summary.current_launcher.as_deref().unwrap_or("none")
        ),
        format!(
            "Current service installed: {}",
            summary.current_service_installed
        ),
        format!(
            "Current service: {}",
            summary
                .current_service
                .as_ref()
                .map(manifest_service_state_label)
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "Desired runtime: {}",
            summary.desired_runtime.as_deref().unwrap_or("none")
        ),
        format!(
            "Desired launcher: {}",
            summary.desired_launcher.as_deref().unwrap_or("none")
        ),
        format!(
            "Desired service install: {}",
            summary
                .desired_service_install
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
    ]
}

fn manifest_resolve_raw(summary: &ManifestResolveSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    lines.insert(
        "env".to_string(),
        summary
            .env_name
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("envExists".to_string(), summary.env_exists.to_string());
    lines.insert(
        "envRoot".to_string(),
        summary
            .env_root
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "currentRuntime".to_string(),
        summary
            .current_runtime
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "currentLauncher".to_string(),
        summary
            .current_launcher
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "currentServiceInstalled".to_string(),
        summary.current_service_installed.to_string(),
    );
    if let Some(service) = summary.current_service.as_ref() {
        lines.insert(
            "currentServiceLoaded".to_string(),
            service.loaded.to_string(),
        );
        lines.insert(
            "currentServiceRunning".to_string(),
            service.running.to_string(),
        );
        lines.insert(
            "currentServiceDefinitionDrift".to_string(),
            service.definition_drift.to_string(),
        );
        lines.insert(
            "currentServiceLiveExecUnverified".to_string(),
            service.live_exec_unverified.to_string(),
        );
        lines.insert(
            "currentServiceOrphanedLive".to_string(),
            service.orphaned_live_service.to_string(),
        );
    }
    lines.insert(
        "desiredRuntime".to_string(),
        summary
            .desired_runtime
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "desiredLauncher".to_string(),
        summary
            .desired_launcher
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "desiredServiceInstall".to_string(),
        summary
            .desired_service_install
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_string()),
    );
    format_key_value_lines(lines)
}

pub fn manifest_drift(summary: &ManifestDriftSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_drift_pretty(summary);
    }
    manifest_drift_raw(summary)
}

fn manifest_drift_pretty(summary: &ManifestDriftSummary) -> Vec<String> {
    let Some(env_name) = summary.env_name.as_deref() else {
        return vec![
            "Manifest drift".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ];
    };

    let mut lines = vec![
        "Manifest drift".to_string(),
        String::new(),
        format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
        format!("Env: {env_name}"),
        format!("Aligned: {}", summary.aligned),
        format!("Env exists: {}", summary.env_exists),
        format!(
            "Desired runtime: {}",
            summary.desired_runtime.as_deref().unwrap_or("none")
        ),
        format!(
            "Current runtime: {}",
            summary.current_runtime.as_deref().unwrap_or("none")
        ),
        format!(
            "Desired launcher: {}",
            summary.desired_launcher.as_deref().unwrap_or("none")
        ),
        format!(
            "Current launcher: {}",
            summary.current_launcher.as_deref().unwrap_or("none")
        ),
        format!(
            "Current service installed: {}",
            summary.current_service_installed
        ),
        format!(
            "Current service: {}",
            summary
                .current_service
                .as_ref()
                .map(manifest_service_state_label)
                .unwrap_or_else(|| "none".to_string())
        ),
    ];
    if !summary.issues.is_empty() {
        lines.push(String::new());
        lines.push("Issues:".to_string());
        lines.extend(summary.issues.iter().map(|issue| format!("  - {issue}")));
    }
    lines
}

fn manifest_drift_raw(summary: &ManifestDriftSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    lines.insert(
        "env".to_string(),
        summary
            .env_name
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("envExists".to_string(), summary.env_exists.to_string());
    lines.insert("aligned".to_string(), summary.aligned.to_string());
    lines.insert(
        "desiredRuntime".to_string(),
        summary
            .desired_runtime
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "currentRuntime".to_string(),
        summary
            .current_runtime
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "desiredLauncher".to_string(),
        summary
            .desired_launcher
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "currentLauncher".to_string(),
        summary
            .current_launcher
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    lines.insert(
        "currentServiceInstalled".to_string(),
        summary.current_service_installed.to_string(),
    );
    if let Some(service) = summary.current_service.as_ref() {
        lines.insert(
            "currentServiceLoaded".to_string(),
            service.loaded.to_string(),
        );
        lines.insert(
            "currentServiceRunning".to_string(),
            service.running.to_string(),
        );
        lines.insert(
            "currentServiceDefinitionDrift".to_string(),
            service.definition_drift.to_string(),
        );
        lines.insert(
            "currentServiceLiveExecUnverified".to_string(),
            service.live_exec_unverified.to_string(),
        );
        lines.insert(
            "currentServiceOrphanedLive".to_string(),
            service.orphaned_live_service.to_string(),
        );
    }
    if summary.issues.is_empty() {
        lines.insert("issues".to_string(), "none".to_string());
    } else {
        lines.insert("issues".to_string(), summary.issues.join(" | "));
    }
    format_key_value_lines(lines)
}

pub fn manifest_plan(summary: &ManifestPlanSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_plan_pretty(summary);
    }
    manifest_plan_raw(summary)
}

fn manifest_plan_pretty(summary: &ManifestPlanSummary) -> Vec<String> {
    let Some(plan) = summary.plan.as_ref() else {
        return vec![
            "Manifest plan".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ];
    };

    vec![
        "Manifest plan".to_string(),
        String::new(),
        format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
        format!("Env: {}", plan.env_name),
        format!("Env exists: {}", summary.env_exists),
        format!("Create env: {}", plan.create_env),
        format!(
            "Desired runtime: {}",
            plan.desired_runtime.as_deref().unwrap_or("none")
        ),
        format!(
            "Desired launcher: {}",
            plan.desired_launcher.as_deref().unwrap_or("none")
        ),
        format!("Runtime changed: {}", plan.runtime_changed),
        format!("Launcher changed: {}", plan.launcher_changed),
        format!(
            "Desired service install: {}",
            plan.desired_service_install
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "Current service: {}",
            plan.current_service
                .as_ref()
                .map(manifest_service_state_label)
                .unwrap_or_else(|| "none".to_string())
        ),
        format!("Service changed: {}", plan.service_changed),
    ]
}

fn manifest_plan_raw(summary: &ManifestPlanSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    lines.insert("envExists".to_string(), summary.env_exists.to_string());
    lines.insert(
        "envRoot".to_string(),
        summary
            .env_root
            .clone()
            .unwrap_or_else(|| "none".to_string()),
    );
    if let Some(plan) = summary.plan.as_ref() {
        lines.insert("env".to_string(), plan.env_name.clone());
        lines.insert("createEnv".to_string(), plan.create_env.to_string());
        lines.insert(
            "desiredRuntime".to_string(),
            plan.desired_runtime
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        );
        lines.insert(
            "desiredLauncher".to_string(),
            plan.desired_launcher
                .clone()
                .unwrap_or_else(|| "none".to_string()),
        );
        lines.insert(
            "runtimeChanged".to_string(),
            plan.runtime_changed.to_string(),
        );
        lines.insert(
            "launcherChanged".to_string(),
            plan.launcher_changed.to_string(),
        );
        lines.insert(
            "desiredServiceInstall".to_string(),
            plan.desired_service_install
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
        );
        if let Some(service) = plan.current_service.as_ref() {
            lines.insert(
                "currentServiceInstalled".to_string(),
                service.installed.to_string(),
            );
            lines.insert(
                "currentServiceLoaded".to_string(),
                service.loaded.to_string(),
            );
            lines.insert(
                "currentServiceRunning".to_string(),
                service.running.to_string(),
            );
            lines.insert(
                "currentServiceDefinitionDrift".to_string(),
                service.definition_drift.to_string(),
            );
            lines.insert(
                "currentServiceLiveExecUnverified".to_string(),
                service.live_exec_unverified.to_string(),
            );
            lines.insert(
                "currentServiceOrphanedLive".to_string(),
                service.orphaned_live_service.to_string(),
            );
        }
        lines.insert(
            "serviceChanged".to_string(),
            plan.service_changed.to_string(),
        );
    }
    format_key_value_lines(lines)
}

fn manifest_service_state_label(state: &ManifestServiceState) -> String {
    if state.orphaned_live_service {
        return "orphaned-live".to_string();
    }
    if state.definition_drift {
        return "definition-drift".to_string();
    }
    if state.live_exec_unverified {
        return "live-unverified".to_string();
    }
    if state.running {
        return "running".to_string();
    }
    if state.loaded {
        return "loaded".to_string();
    }
    if state.installed {
        return "installed".to_string();
    }
    "absent".to_string()
}
