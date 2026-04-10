use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::env::{CreateEnvSnapshotOptions, EnvironmentService, RestoreEnvSnapshotOptions};
use crate::service::ServiceService;

use super::{
    OcmManifest, apply_manifest_launcher_binding, apply_manifest_runtime_binding,
    apply_manifest_service_install, ensure_manifest_env, plan_manifest_application_with_service,
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
    let current_service = ServiceService::new(env, cwd).status_fast(&current.name)?;
    let plan = plan_manifest_application_with_service(
        manifest,
        Some(&current),
        Some(current_service.installed),
    );
    let service_change_needed = match plan.desired_service_install {
        Some(true) => !current_service.installed,
        Some(false) => {
            current_service.installed || current_service.loaded || current_service.running
        }
        None => false,
    };
    let apply_needed = env_summary.created
        || plan.runtime_changed
        || plan.launcher_changed
        || service_change_needed;

    if !apply_needed {
        return Ok(ManifestReconcileSummary {
            manifest_path: manifest_path.display().to_string(),
            env_name: current.name.clone(),
            env_root: current.root.clone(),
            env_existed: true,
            env_created: false,
            runtime_changed: false,
            launcher_changed: false,
            service_changed: false,
            desired_runtime: plan.desired_runtime,
            desired_launcher: plan.desired_launcher,
            desired_service_install: plan.desired_service_install,
            snapshot_id: None,
            rolled_back: false,
            service_installed: current_service.installed,
            service_loaded: current_service.loaded,
            service_running: current_service.running,
        });
    }

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

    let apply_result = (|| {
        let runtime_summary = apply_manifest_runtime_binding(manifest, &current, env, cwd)?;
        current = runtime_summary.env.clone();

        let launcher_summary = apply_manifest_launcher_binding(manifest, &current, env, cwd)?;
        current = launcher_summary.env.clone();

        let service_summary = apply_manifest_service_install(manifest, &current, env, cwd)?;

        Ok((runtime_summary, launcher_summary, service_summary))
    })();

    let (runtime_summary, launcher_summary, service_summary) = match apply_result {
        Ok(summaries) => summaries,
        Err(error) => {
            if _options.rollback_on_failure && env_summary.created {
                let rollback = rollback_created_manifest_env(&current.name, env, cwd);
                return match rollback {
                    Ok(()) => Err(format!(
                        "{error} (removed newly created env \"{}\")",
                        current.name
                    )),
                    Err(rollback_error) => Err(format!(
                        "{error} (cleanup of newly created env \"{}\" failed: {rollback_error})",
                        current.name
                    )),
                };
            }
            if _options.rollback_on_failure
                && let Some(snapshot_id) = snapshot_id.as_deref()
            {
                let rollback =
                    EnvironmentService::new(env, cwd).restore_snapshot(RestoreEnvSnapshotOptions {
                        env_name: current.name.clone(),
                        snapshot_id: snapshot_id.to_string(),
                    });
                return match rollback {
                    Ok(_) => Err(format!(
                        "{error} (rolled back env \"{}\" from snapshot {snapshot_id})",
                        current.name
                    )),
                    Err(rollback_error) => Err(format!(
                        "{error} (rollback from snapshot {snapshot_id} failed: {rollback_error})"
                    )),
                };
            }
            return Err(error);
        }
    };

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

fn rollback_created_manifest_env(
    env_name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(), String> {
    let service = ServiceService::new(env, cwd);
    let status = service.status_fast(env_name)?;
    if status.installed || status.loaded || status.running {
        service.uninstall(env_name)?;
    }
    EnvironmentService::new(env, cwd).remove(env_name, true)?;
    Ok(())
}
