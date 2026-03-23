mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use time::{Duration, OffsetDateTime};

use crate::paths::{
    clean_path, default_env_root, derive_env_paths, display_path, env_meta_path,
    launcher_meta_path, resolve_absolute_path, resolve_store_paths, validate_name,
};
use crate::types::{
    AddLauncherOptions, CreateEnvironmentOptions, EnvMarker, EnvMeta, EnvSummary, LauncherMeta,
    StorePaths,
};
use common::{ensure_dir, load_json_files, path_exists, read_json, write_json};

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub fn ensure_store(env: &BTreeMap<String, String>, cwd: &Path) -> Result<StorePaths, String> {
    let stores = resolve_store_paths(env, cwd)?;
    ensure_dir(&stores.home)?;
    ensure_dir(&stores.envs_dir)?;
    ensure_dir(&stores.launchers_dir)?;
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
        default_launcher: meta.default_launcher.clone(),
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
        default_launcher: options.default_launcher,
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

pub fn list_launchers(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<LauncherMeta>, String> {
    let stores = ensure_store(env, cwd)?;
    let files = load_json_files(&stores.launchers_dir)?;
    let mut out: Vec<LauncherMeta> = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

pub fn get_launcher(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<LauncherMeta, String> {
    let safe_name = validate_name(name, "Launcher name")?;
    let path = launcher_meta_path(&safe_name, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!("launcher \"{safe_name}\" does not exist"));
    }
    read_json(&path)
}

pub fn add_launcher(
    options: AddLauncherOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<LauncherMeta, String> {
    let name = validate_name(&options.name, "Launcher name")?;
    let meta_path = launcher_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("launcher \"{name}\" already exists"));
    }

    let command = options.command.trim();
    if command.is_empty() {
        return Err("launcher command is required".to_string());
    }

    let launcher_cwd = match options.cwd.as_deref() {
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
    let meta = LauncherMeta {
        kind: "ocm-launcher".to_string(),
        name,
        command: command.to_string(),
        cwd: launcher_cwd,
        description,
        created_at,
        updated_at: created_at,
    };
    write_json(&meta_path, &meta)?;
    Ok(meta)
}

pub fn remove_launcher(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<LauncherMeta, String> {
    let meta = get_launcher(name, env, cwd)?;
    let path = launcher_meta_path(&meta.name, env, cwd)?;
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
