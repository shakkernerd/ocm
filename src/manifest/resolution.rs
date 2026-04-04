use std::path::Path;

use super::{ManifestResolution, find_manifest_path, load_manifest};

pub fn resolve_manifest(start: &Path) -> Result<Option<ManifestResolution>, String> {
    let Some(path) = find_manifest_path(start)? else {
        return Ok(None);
    };
    let manifest = load_manifest(&path)?;
    Ok(Some(ManifestResolution { path, manifest }))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::resolve_manifest;

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join("ocm-manifest-resolution-tests")
            .join(format!("{label}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn resolve_manifest_loads_a_discovered_manifest() {
        let root = temp_root("loads");
        let nested = root.join("workspace").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let manifest_path = root.join("ocm.yaml");
        std::fs::write(
            &manifest_path,
            "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\n",
        )
        .unwrap();

        let resolved = resolve_manifest(&nested).unwrap().unwrap();
        assert_eq!(resolved.path, manifest_path);
        assert_eq!(resolved.manifest.env.name, "mira");
        assert_eq!(
            resolved
                .manifest
                .runtime
                .as_ref()
                .and_then(|runtime| runtime.channel.as_deref()),
            Some("stable")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_manifest_returns_none_when_no_manifest_is_present() {
        let root = temp_root("missing");
        let nested = root.join("workspace");
        std::fs::create_dir_all(&nested).unwrap();

        let resolved = resolve_manifest(&nested).unwrap();
        assert!(resolved.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_manifest_returns_parse_errors_with_the_manifest_path() {
        let root = temp_root("parse-error");
        let manifest_path = root.join("ocm.yaml");
        std::fs::write(&manifest_path, "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\n  version: 2026.4.4\n").unwrap();

        let error = resolve_manifest(&root).unwrap_err();
        assert_eq!(
            error,
            format!(
                "failed to parse {}: manifest runtime accepts only one of name, version, or channel",
                manifest_path.display()
            )
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
