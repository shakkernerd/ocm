use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) fn path_exists(path: &Path) -> bool {
    path.exists()
}

pub(crate) fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())
}

pub(crate) fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<(), String> {
    ensure_dir(destination)?;

    let entries = fs::read_dir(source).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        let file_type = metadata.file_type();

        if file_type.is_symlink() {
            copy_symlink(&source_path, &destination_path)?;
            continue;
        }

        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path)?;
            continue;
        }

        if let Some(parent) = destination_path.parent() {
            ensure_dir(parent)?;
        }
        fs::copy(&source_path, &destination_path).map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn copy_symlink(source_path: &Path, destination_path: &Path) -> Result<(), String> {
    if let Some(parent) = destination_path.parent() {
        ensure_dir(parent)?;
    }
    let target = fs::read_link(source_path).map_err(|error| error.to_string())?;

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, destination_path).map_err(|error| error.to_string())?;
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(&target, destination_path)
            .or_else(|_| std::os::windows::fs::symlink_dir(&target, destination_path))
            .map_err(|error| error.to_string())?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        return Err(format!(
            "copying symlinks is not supported on this platform: {}",
            source_path.display()
        ));
    }

    Ok(())
}

pub(crate) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

pub(crate) fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    let mut raw = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    raw.push('\n');
    let parent = path
        .parent()
        .ok_or_else(|| format!("json path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("json path has no file name: {}", path.display()))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temp_path = parent.join(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));

    fs::write(&temp_path, raw).map_err(|error| error.to_string())?;
    fs::rename(&temp_path, path).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        error.to_string()
    })
}

pub(crate) fn load_json_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error.to_string()),
    };

    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value == "json")
                .unwrap_or(false)
        {
            files.push(path);
        }
    }

    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::copy_dir_recursive;

    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_root(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join("ocm-copy-dir-tests")
            .join(format!("{label}-{}-{id}", std::process::id()))
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_preserves_broken_symlinks() {
        let root = temp_root("broken-symlink");
        let source = root.join("source");
        let destination = root.join("destination");
        fs::create_dir_all(source.join("node_modules/.bin")).unwrap();
        fs::write(source.join("node_modules/package.json"), "{}\n").unwrap();
        std::os::unix::fs::symlink(
            "../missing-package/bin/tool",
            source.join("node_modules/.bin/tool"),
        )
        .unwrap();

        copy_dir_recursive(&source, &destination).unwrap();

        let copied_link = destination.join("node_modules/.bin/tool");
        let metadata = fs::symlink_metadata(&copied_link).unwrap();
        assert!(metadata.file_type().is_symlink());
        assert_eq!(
            fs::read_link(&copied_link).unwrap(),
            PathBuf::from("../missing-package/bin/tool")
        );
        assert!(!copied_link.exists());
        assert_eq!(
            fs::read_to_string(destination.join("node_modules/package.json")).unwrap(),
            "{}\n"
        );

        fs::remove_dir_all(root).unwrap();
    }
}
