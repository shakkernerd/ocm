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

pub fn summarize_env(meta: &EnvMeta) -> EnvSummary {
    let paths = derive_env_paths(Path::new(&meta.root));
    EnvSummary {
        name: meta.name.clone(),
        root: display_path(&paths.root),
        openclaw_home: display_path(&paths.openclaw_home),
        state_dir: display_path(&paths.state_dir),
        config_path: display_path(&paths.config_path),
        workspace_dir: display_path(&paths.workspace_dir),
        gateway_port: meta.gateway_port,
        default_version: meta.default_version.clone(),
        protected: meta.protected,
        created_at: meta.created_at,
        last_used_at: meta.last_used_at,
    }
}

pub fn list_environments(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<EnvMeta>, String> {
    let stores = ensure_store(env, cwd)?;
    let files = load_json_files(&stores.envs_dir)?;
    let mut out: Vec<EnvMeta> = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

pub fn get_environment(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let safe_name = validate_name(name, "Environment name")?;
    let path = env_meta_path(&safe_name, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!("environment \"{safe_name}\" does not exist"));
    }
    read_json(&path)
}

pub fn save_environment(
    mut meta: EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let safe_name = validate_name(&meta.name, "Environment name")?;
    meta.name = safe_name;
    meta.kind = "ocm-env".to_string();
    meta.root = display_path(&clean_path(Path::new(&meta.root)));
    meta.updated_at = now_utc();

    let path = env_meta_path(&meta.name, env, cwd)?;
    write_json(&path, &meta)?;
    Ok(meta)
}

pub fn create_environment(
    options: CreateEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let name = validate_name(&options.name, "Environment name")?;
    let meta_path = env_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("environment \"{name}\" already exists"));
    }

    let root = if let Some(root) = options.root.as_deref() {
        resolve_absolute_path(root, env, cwd)?
    } else {
        default_env_root(&name, env, cwd)?
    };

    let paths = derive_env_paths(&root);
    if path_exists(&paths.root) {
        let mut entries = fs::read_dir(&paths.root).map_err(|error| error.to_string())?;
        if entries.next().is_some() {
            return Err(format!(
                "root already exists and is not empty: {}",
                display_path(&paths.root)
            ));
        }
    }

    ensure_dir(&paths.root)?;
    ensure_dir(&paths.state_dir)?;
    ensure_dir(&paths.workspace_dir)?;

    let created_at = now_utc();
    let marker = EnvMarker {
        kind: "ocm-env-marker".to_string(),
        name: name.clone(),
        created_at,
    };
    write_json(&paths.marker_path, &marker)?;

    let meta = EnvMeta {
        kind: "ocm-env".to_string(),
        name,
        root: display_path(&paths.root),
        gateway_port: options.gateway_port,
        default_version: options.default_version,
        protected: options.protected,
        created_at,
        updated_at: created_at,
        last_used_at: None,
    };
    save_environment(meta, env, cwd)
}

pub fn remove_environment(
    name: &str,
    force: bool,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let meta = get_environment(name, env, cwd)?;
    if meta.protected && !force {
        return Err(format!(
            "environment \"{}\" is protected; re-run with --force",
            meta.name
        ));
    }

    let paths = derive_env_paths(Path::new(&meta.root));
    let root_exists = path_exists(&paths.root);
    let marker_exists = path_exists(&paths.marker_path);

    if root_exists && !marker_exists && !force {
        let marker_name = paths
            .marker_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(".ocm-env.json");
        return Err(format!(
            "refusing to delete {} without {}; re-run with --force",
            display_path(&paths.root),
            marker_name
        ));
    }

    if root_exists {
        fs::remove_dir_all(&paths.root).map_err(|error| error.to_string())?;
    }

    let meta_path = env_meta_path(&meta.name, env, cwd)?;
    match fs::remove_file(meta_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    Ok(meta)
}

pub fn list_versions(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<VersionMeta>, String> {
    let stores = ensure_store(env, cwd)?;
    let files = load_json_files(&stores.versions_dir)?;
    let mut out: Vec<VersionMeta> = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

pub fn get_version(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<VersionMeta, String> {
    let safe_name = validate_name(name, "Version name")?;
    let path = version_meta_path(&safe_name, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!("version \"{safe_name}\" does not exist"));
    }
    read_json(&path)
}

pub fn add_version(
    options: AddVersionOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<VersionMeta, String> {
    let name = validate_name(&options.name, "Version name")?;
    let meta_path = version_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("version \"{name}\" already exists"));
    }

    let command = options.command.trim();
    if command.is_empty() {
        return Err("version command is required".to_string());
    }

    let version_cwd = match options.cwd.as_deref() {
        Some(raw) if !raw.trim().is_empty() => {
            Some(display_path(&resolve_absolute_path(raw, env, cwd)?))
        }
        _ => None,
    };
    let description = options
        .description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let created_at = now_utc();
    let meta = VersionMeta {
        kind: "ocm-version".to_string(),
        name,
        command: command.to_string(),
        cwd: version_cwd,
        description,
        created_at,
        updated_at: created_at,
    };
    write_json(&meta_path, &meta)?;
    Ok(meta)
}

pub fn remove_version(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<VersionMeta, String> {
    let meta = get_version(name, env, cwd)?;
    let path = version_meta_path(&meta.name, env, cwd)?;
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(meta)
}

pub fn select_prune_candidates(envs: &[EnvMeta], older_than_days: i64) -> Vec<EnvMeta> {
    let cutoff = now_utc() - Duration::days(older_than_days);
    envs.iter()
        .filter(|meta| !meta.protected)
        .filter(|meta| meta.last_used_at.unwrap_or(meta.created_at) < cutoff)
        .cloned()
        .collect()
}
