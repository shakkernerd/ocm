use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::paths::{display_path, resolve_absolute_path, runtime_meta_path, validate_name};
use crate::types::{AddRuntimeOptions, RuntimeMeta};

use super::common::{load_json_files, path_exists, read_json, write_json};
use super::now_utc;

pub fn list_runtimes(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<RuntimeMeta>, String> {
    let stores = super::ensure_store(env, cwd)?;
    let files = load_json_files(&stores.runtimes_dir)?;
    let mut out: Vec<RuntimeMeta> = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

pub fn get_runtime(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let safe_name = validate_name(name, "Runtime name")?;
    let path = runtime_meta_path(&safe_name, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!("runtime \"{safe_name}\" does not exist"));
    }
    read_json(&path)
}

pub fn add_runtime(
    options: AddRuntimeOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = runtime_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("runtime \"{name}\" already exists"));
    }

    let raw_path = options.path.trim();
    if raw_path.is_empty() {
        return Err("runtime path is required".to_string());
    }

    let binary_path = resolve_absolute_path(raw_path, env, cwd)?;
    if !path_exists(&binary_path) {
        return Err(format!(
            "runtime path does not exist: {}",
            display_path(&binary_path)
        ));
    }

    let metadata = fs::metadata(&binary_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!(
            "runtime path must be a file: {}",
            display_path(&binary_path)
        ));
    }

    let description = options
        .description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    let created_at = now_utc();
    let meta = RuntimeMeta {
        kind: "ocm-runtime".to_string(),
        name,
        binary_path: display_path(&binary_path),
        description,
        created_at,
        updated_at: created_at,
    };
    write_json(&meta_path, &meta)?;
    Ok(meta)
}

pub fn remove_runtime(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let meta = get_runtime(name, env, cwd)?;
    let path = runtime_meta_path(&meta.name, env, cwd)?;
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(meta)
}
