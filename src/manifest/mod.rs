mod discovery;
mod resolution;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use discovery::{MANIFEST_FILE_NAME, find_manifest_path};
pub use resolution::resolve_manifest;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct OcmManifest {
    pub schema: String,
    pub env: ManifestEnv,
    #[serde(default)]
    pub runtime: Option<ManifestRuntime>,
    #[serde(default)]
    pub launcher: Option<ManifestLauncher>,
    #[serde(default)]
    pub service: Option<ManifestService>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct ManifestEnv {
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ManifestRuntime {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ManifestLauncher {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ManifestService {
    #[serde(default)]
    pub install: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestResolution {
    pub path: PathBuf,
    pub manifest: OcmManifest,
}

pub fn load_manifest(path: &Path) -> Result<OcmManifest, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    parse_manifest(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

pub fn parse_manifest(raw: &str) -> Result<OcmManifest, String> {
    let manifest: OcmManifest = serde_yaml::from_str(raw).map_err(|error| error.to_string())?;
    validate_manifest(manifest)
}

pub fn validate_manifest(manifest: OcmManifest) -> Result<OcmManifest, String> {
    if manifest.schema.trim().is_empty() {
        return Err("manifest schema is required".to_string());
    }
    if manifest.env.name.trim().is_empty() {
        return Err("manifest env.name is required".to_string());
    }

    let launcher_selected = manifest
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.name.as_deref())
        .is_some_and(|value| !value.trim().is_empty());

    if let Some(runtime) = manifest.runtime.as_ref() {
        let selectors = [
            runtime
                .channel
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            runtime
                .version
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
            runtime
                .name
                .as_deref()
                .filter(|value| !value.trim().is_empty()),
        ]
        .into_iter()
        .flatten()
        .count();

        if selectors > 1 {
            return Err(
                "manifest runtime accepts only one of name, version, or channel".to_string(),
            );
        }
        if selectors == 1 && launcher_selected {
            return Err(
                "manifest accepts either a runtime selector or a launcher selector, not both"
                    .to_string(),
            );
        }
    }

    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{load_manifest, parse_manifest};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn parse_manifest_accepts_the_minimal_shape() {
        let manifest = parse_manifest(
            "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\nservice:\n  install: true\n",
        )
        .unwrap();

        assert_eq!(manifest.schema, "ocm/v1");
        assert_eq!(manifest.env.name, "mira");
        assert_eq!(
            manifest
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.channel.as_deref()),
            Some("stable")
        );
        assert_eq!(
            manifest
                .service
                .as_ref()
                .and_then(|service| service.install),
            Some(true)
        );
    }

    #[test]
    fn parse_manifest_rejects_conflicting_runtime_selectors() {
        let error = parse_manifest(
            "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\n  version: 2026.4.4\n",
        )
        .unwrap_err();

        assert_eq!(
            error,
            "manifest runtime accepts only one of name, version, or channel"
        );
    }

    #[test]
    fn parse_manifest_rejects_runtime_and_launcher_together() {
        let error = parse_manifest(
            "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\nlauncher:\n  name: dev\n",
        )
        .unwrap_err();

        assert_eq!(
            error,
            "manifest accepts either a runtime selector or a launcher selector, not both"
        );
    }

    #[test]
    fn load_manifest_reads_yaml_from_disk() {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join("ocm-manifest-tests")
            .join(format!("manifest-load-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path: PathBuf = root.join("ocm.yaml");
        std::fs::write(&path, "schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

        let manifest = load_manifest(&path).unwrap();
        assert_eq!(manifest.schema, "ocm/v1");
        assert_eq!(manifest.env.name, "mira");

        let _ = std::fs::remove_dir_all(&root);
    }
}
