use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use crate::env::{CreateEnvironmentOptions, EnvMeta, EnvironmentService};

use super::OcmManifest;

#[derive(Clone, Debug, Serialize)]
pub struct ManifestEnvApplySummary {
    pub env: EnvMeta,
    pub created: bool,
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
