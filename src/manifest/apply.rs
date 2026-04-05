use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::env::{CreateEnvironmentOptions, EnvMeta, EnvironmentService};

use super::{ManifestRuntime, OcmManifest};

#[derive(Clone, Debug, Serialize)]
pub struct ManifestEnvApplySummary {
    pub env: EnvMeta,
    pub created: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManifestRuntimeApplySummary {
    pub env: EnvMeta,
    pub changed: bool,
    pub desired_runtime: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ManifestLauncherApplySummary {
    pub env: EnvMeta,
    pub changed: bool,
    pub desired_launcher: Option<String>,
}

pub fn ensure_manifest_env(
    manifest: &OcmManifest,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ManifestEnvApplySummary, String> {
    let service = EnvironmentService::new(env, cwd);
    if let Some(existing) = service.find(&manifest.env.name)? {
        return Ok(ManifestEnvApplySummary {
            env: existing,
            created: false,
        });
    }

    let created = service.create(CreateEnvironmentOptions {
        name: manifest.env.name.clone(),
        root: None,
        gateway_port: None,
        default_runtime: None,
        default_launcher: None,
        protected: false,
    })?;

    Ok(ManifestEnvApplySummary {
        env: created,
        created: true,
    })
}

pub fn apply_manifest_runtime_binding(
    manifest: &OcmManifest,
    current: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ManifestRuntimeApplySummary, String> {
    let service = EnvironmentService::new(env, cwd);
    let desired_runtime = resolve_manifest_runtime(&service, manifest.runtime.as_ref())?;
    if desired_runtime == current.default_runtime {
        return Ok(ManifestRuntimeApplySummary {
            env: current.clone(),
            changed: false,
            desired_runtime,
        });
    }

    let updated = match desired_runtime.as_deref() {
        Some(runtime_name) => service.set_runtime(&current.name, runtime_name)?,
        None => current.clone(),
    };

    Ok(ManifestRuntimeApplySummary {
        env: updated,
        changed: true,
        desired_runtime,
    })
}

pub fn apply_manifest_launcher_binding(
    manifest: &OcmManifest,
    current: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ManifestLauncherApplySummary, String> {
    let service = EnvironmentService::new(env, cwd);
    let desired_launcher = manifest
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.name.clone())
        .filter(|name| !name.trim().is_empty());

    if desired_launcher == current.default_launcher {
        return Ok(ManifestLauncherApplySummary {
            env: current.clone(),
            changed: false,
            desired_launcher,
        });
    }

    let updated = match desired_launcher.as_deref() {
        Some(launcher_name) => service.set_launcher(&current.name, launcher_name)?,
        None => current.clone(),
    };

    Ok(ManifestLauncherApplySummary {
        env: updated,
        changed: true,
        desired_launcher,
    })
}

fn resolve_manifest_runtime(
    service: &EnvironmentService<'_>,
    runtime: Option<&ManifestRuntime>,
) -> Result<Option<String>, String> {
    let Some(runtime) = runtime else {
        return Ok(None);
    };

    service.resolve_runtime_binding_request(
        runtime.name.clone(),
        runtime.version.clone(),
        runtime.channel.clone(),
        "manifest runtime",
    )
}
