use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::paths::{
    clean_path, default_env_root, derive_env_paths, display_path, env_meta_path,
    resolve_absolute_path, validate_name,
};
use crate::types::{CloneEnvironmentOptions, CreateEnvironmentOptions, EnvMarker, EnvMeta};

use super::common::{
    copy_dir_recursive, ensure_dir, load_json_files, path_exists, read_json, write_json,
};
use super::now_utc;

pub fn list_environments(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<EnvMeta>, String> {
    let stores = super::ensure_store(env, cwd)?;
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
        default_runtime: options.default_runtime,
        default_launcher: options.default_launcher,
        protected: options.protected,
        created_at,
        updated_at: created_at,
        last_used_at: None,
    };
    save_environment(meta, env, cwd)
}

pub fn clone_environment(
    options: CloneEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let source = get_environment(&options.source_name, env, cwd)?;
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
    let target_paths = derive_env_paths(&root);
    if path_exists(&target_paths.root) {
        let mut entries = fs::read_dir(&target_paths.root).map_err(|error| error.to_string())?;
        if entries.next().is_some() {
            return Err(format!(
                "root already exists and is not empty: {}",
                display_path(&target_paths.root)
            ));
        }
    }

    let source_paths = derive_env_paths(Path::new(&source.root));
    if !path_exists(&source_paths.root) {
        return Err(format!(
            "environment root does not exist: {}",
            display_path(&source_paths.root)
        ));
    }
    if !path_exists(&source_paths.marker_path) {
        return Err(format!(
            "refusing to clone {} without {}",
            display_path(&source_paths.root),
            source_paths
                .marker_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(".ocm-env.json")
        ));
    }

    let result = (|| {
        copy_dir_recursive(&source_paths.root, &target_paths.root)?;
        let created_at = now_utc();
        let marker = EnvMarker {
            kind: "ocm-env-marker".to_string(),
            name: name.clone(),
            created_at,
        };
        write_json(&target_paths.marker_path, &marker)?;

        let meta = EnvMeta {
            kind: "ocm-env".to_string(),
            name,
            root: display_path(&target_paths.root),
            gateway_port: source.gateway_port,
            default_runtime: source.default_runtime,
            default_launcher: source.default_launcher,
            protected: source.protected,
            created_at,
            updated_at: created_at,
            last_used_at: None,
        };
        save_environment(meta, env, cwd)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_dir_all(&target_paths.root);
    }

    result
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
