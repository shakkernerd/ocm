use std::path::{Path, PathBuf};

pub const MANIFEST_FILE_NAME: &str = "ocm.yaml";

pub fn find_manifest_path(start: &Path) -> Result<Option<PathBuf>, String> {
    if start.is_file() && looks_like_manifest_file(start) {
        return Ok(Some(start.to_path_buf()));
    }

    let mut cursor = if start.is_dir() {
        start.to_path_buf()
    } else {
        start
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("cannot search for {MANIFEST_FILE_NAME} from an empty path"))?
    };

    loop {
        let candidate = cursor.join(MANIFEST_FILE_NAME);
        if candidate.is_file() {
            return Ok(Some(candidate));
        }

        if !cursor.pop() {
            return Ok(None);
        }
    }
}

fn looks_like_manifest_file(path: &Path) -> bool {
    if path
        .file_name()
        .is_some_and(|value| value == std::ffi::OsStr::new(MANIFEST_FILE_NAME))
    {
        return true;
    }

    matches!(
        path.extension().and_then(|value| value.to_str()),
        Some("yaml" | "yml")
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{MANIFEST_FILE_NAME, find_manifest_path};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir()
            .join("ocm-manifest-discovery-tests")
            .join(format!("{label}-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn find_manifest_path_finds_the_current_directory_manifest() {
        let root = temp_root("current-dir");
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        std::fs::write(&manifest_path, "schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

        let found = find_manifest_path(&root).unwrap();
        assert_eq!(found.as_deref(), Some(manifest_path.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_manifest_path_walks_up_to_parent_directories() {
        let root = temp_root("parent-dir");
        let nested = root.join("workspace").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        std::fs::write(&manifest_path, "schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

        let found = find_manifest_path(&nested).unwrap();
        assert_eq!(found.as_deref(), Some(manifest_path.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_manifest_path_returns_none_when_no_manifest_exists() {
        let root = temp_root("missing");
        let nested = root.join("workspace");
        std::fs::create_dir_all(&nested).unwrap();

        let found = find_manifest_path(&nested).unwrap();
        assert!(found.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_manifest_path_accepts_file_paths_by_searching_their_parent() {
        let root = temp_root("file-start");
        let nested = root.join("workspace");
        std::fs::create_dir_all(&nested).unwrap();
        let file_path = nested.join("notes.txt");
        std::fs::write(&file_path, "hello").unwrap();
        let manifest_path = root.join(MANIFEST_FILE_NAME);
        std::fs::write(&manifest_path, "schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

        let found = find_manifest_path(&file_path).unwrap();
        assert_eq!(found.as_deref(), Some(manifest_path.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn find_manifest_path_accepts_existing_yaml_files_directly() {
        let root = temp_root("direct-manifest-file");
        let nested = root.join("workspace");
        std::fs::create_dir_all(&nested).unwrap();
        let manifest_path = nested.join("demo.yaml");
        std::fs::write(&manifest_path, "schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

        let found = find_manifest_path(&manifest_path).unwrap();
        assert_eq!(found.as_deref(), Some(manifest_path.as_path()));

        let _ = std::fs::remove_dir_all(&root);
    }
}
