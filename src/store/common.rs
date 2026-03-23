use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) fn path_exists(path: &Path) -> bool {
    path.exists()
}

pub(crate) fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())
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
    fs::write(path, raw).map_err(|error| error.to_string())
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
