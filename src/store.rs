use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use time::{Duration, OffsetDateTime};

use crate::paths::{
    clean_path, default_env_root, derive_env_paths, display_path, env_meta_path,
    resolve_absolute_path, resolve_store_paths, validate_name, version_meta_path,
};
use crate::types::{
    AddVersionOptions, CreateEnvironmentOptions, EnvMarker, EnvMeta, EnvSummary, StorePaths,
    VersionMeta,
};

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn path_exists(path: &Path) -> bool {
    path.exists()
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }

    let mut raw = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    raw.push('\n');
    fs::write(path, raw).map_err(|error| error.to_string())
}

fn load_json_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
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

pub fn ensure_store(env: &BTreeMap<String, String>, cwd: &Path) -> Result<StorePaths, String> {
    let stores = resolve_store_paths(env, cwd)?;
    ensure_dir(&stores.home)?;
    ensure_dir(&stores.envs_dir)?;
    ensure_dir(&stores.versions_dir)?;
    Ok(stores)
}
