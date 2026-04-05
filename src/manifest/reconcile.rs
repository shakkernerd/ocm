use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::env::{CreateEnvSnapshotOptions, EnvironmentService};

use super::{
    OcmManifest, apply_manifest_launcher_binding, apply_manifest_runtime_binding,
    apply_manifest_service_install, ensure_manifest_env,
};

#[derive(Clone, Debug, Default)]
pub struct ManifestReconcileOptions {
    pub snapshot_existing_env: bool,
    pub rollback_on_failure: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManifestReconcileSummary {
    pub manifest_path: String,
    pub env_name: String,
    pub env_root: String,
    pub env_existed: bool,
    pub env_created: bool,
    pub runtime_changed: bool,
    pub launcher_changed: bool,
    pub service_changed: bool,
    pub desired_runtime: Option<String>,
    pub desired_launcher: Option<String>,
    pub desired_service_install: Option<bool>,
    pub snapshot_id: Option<String>,
    pub rolled_back: bool,
    pub service_installed: bool,
    pub service_loaded: bool,
    pub service_running: bool,
}

pub fn reconcile_manifest(
    manifest_path: &Path,
    manifest: &OcmManifest,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ManifestReconcileSummary, String> {
    reconcile_manifest_with_options(
        manifest_path,
        manifest,
        env,
        cwd,
        ManifestReconcileOptions::default(),
    )
}

pub fn reconcile_manifest_with_options(
    manifest_path: &Path,
    manifest: &OcmManifest,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    _options: ManifestReconcileOptions,
) -> Result<ManifestReconcileSummary, String> {
    let env_summary = ensure_manifest_env(manifest, env, cwd)?;
    let mut current = env_summary.env;
    let snapshot_id = if !env_summary.created && _options.snapshot_existing_env {
        let snapshot =
            EnvironmentService::new(env, cwd).create_snapshot(CreateEnvSnapshotOptions {
                env_name: current.name.clone(),
                label: Some("manifest-apply".to_string()),
            })?;
        Some(snapshot.id)
    } else {
        None
    };

    let runtime_summary = apply_manifest_runtime_binding(manifest, &current, env, cwd)?;
    current = runtime_summary.env;

    let launcher_summary = apply_manifest_launcher_binding(manifest, &current, env, cwd)?;
    current = launcher_summary.env;

    let service_summary = apply_manifest_service_install(manifest, &current, env, cwd)?;

    Ok(ManifestReconcileSummary {
        manifest_path: manifest_path.display().to_string(),
        env_name: current.name.clone(),
        env_root: current.root.clone(),
        env_existed: !env_summary.created,
        env_created: env_summary.created,
        runtime_changed: runtime_summary.changed,
        launcher_changed: launcher_summary.changed,
        service_changed: service_summary.changed,
        desired_runtime: runtime_summary.desired_runtime,
        desired_launcher: launcher_summary.desired_launcher,
        desired_service_install: service_summary.desired_service_install,
        snapshot_id,
        rolled_back: false,
        service_installed: service_summary.service.installed,
        service_loaded: service_summary.service.loaded,
        service_running: service_summary.service.running,
    })
}
