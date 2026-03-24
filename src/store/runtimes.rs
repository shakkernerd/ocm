use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::download::{artifact_file_name_from_url, download_to_file};
use crate::paths::{
    clean_path, display_path, resolve_absolute_path, runtime_install_files_dir,
    runtime_install_root, runtime_meta_path, validate_name,
};
use crate::types::{
    AddRuntimeOptions, InstallRuntimeFromUrlOptions, InstallRuntimeOptions, RuntimeMeta,
    RuntimeSourceKind,
};

use super::common::{ensure_dir, load_json_files, path_exists, read_json, write_json};
use super::now_utc;

fn trim_description(description: Option<String>) -> Option<String> {
    description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn build_installed_runtime_meta(
    name: String,
    binary_path: &Path,
    install_root: &Path,
    source_path: Option<&Path>,
    source_url: Option<String>,
    description: Option<String>,
) -> RuntimeMeta {
    let created_at = now_utc();
    RuntimeMeta {
        kind: "ocm-runtime".to_string(),
        name,
        binary_path: display_path(binary_path),
        source_kind: RuntimeSourceKind::Installed,
        source_path: source_path.map(display_path),
        source_url,
        install_root: Some(display_path(install_root)),
        description,
        created_at,
        updated_at: created_at,
    }
}

fn copy_installed_runtime_binary(source_path: &Path, binary_path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(source_path).map_err(|error| error.to_string())?;
    fs::copy(source_path, binary_path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        let permissions = metadata.permissions();
        fs::set_permissions(binary_path, permissions).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn install_runtime_at_path(
    name: String,
    meta_path: PathBuf,
    install_root: PathBuf,
    install_files: PathBuf,
    file_name: &Path,
    source_path: Option<&Path>,
    source_url: Option<String>,
    description: Option<String>,
) -> Result<RuntimeMeta, String> {
    if path_exists(&install_root) {
        return Err(format!(
            "runtime install root already exists: {}",
            display_path(&install_root)
        ));
    }

    let result = (|| {
        ensure_dir(&install_files)?;
        let binary_path = install_files.join(file_name);
        match (source_path, source_url.as_deref()) {
            (Some(source_path), _) => copy_installed_runtime_binary(source_path, &binary_path)?,
            (None, Some(source_url)) => {
                download_to_file(source_url, &binary_path)?;
                #[cfg(unix)]
                {
                    let mut permissions = fs::metadata(&binary_path)
                        .map_err(|error| error.to_string())?
                        .permissions();
                    permissions.set_mode(0o755);
                    fs::set_permissions(&binary_path, permissions)
                        .map_err(|error| error.to_string())?;
                }
            }
            (None, None) => return Err("runtime install requires a source path or URL".to_string()),
        }

        let meta = build_installed_runtime_meta(
            name,
            &binary_path,
            &install_root,
            source_path,
            source_url,
            description,
        );
        write_json(&meta_path, &meta)?;
        Ok(meta)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_dir_all(&install_root);
    }

    result
}

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

pub fn get_runtime_verified(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    verify_runtime_binary(get_runtime(name, env, cwd)?)
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

    let description = trim_description(options.description);

    let created_at = now_utc();
    let meta = RuntimeMeta {
        kind: "ocm-runtime".to_string(),
        name,
        binary_path: display_path(&binary_path),
        source_kind: RuntimeSourceKind::Registered,
        source_path: Some(display_path(&binary_path)),
        source_url: None,
        install_root: None,
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
    if let Some(install_root) = meta.install_root.as_deref() {
        let expected_root = runtime_install_root(&meta.name, env, cwd)?;
        if clean_path(Path::new(install_root)) == expected_root && path_exists(&expected_root) {
            fs::remove_dir_all(&expected_root).map_err(|error| error.to_string())?;
        }
    }
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(meta)
}

pub fn install_runtime(
    options: InstallRuntimeOptions,
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

    let source_path = resolve_absolute_path(raw_path, env, cwd)?;
    if !path_exists(&source_path) {
        return Err(format!(
            "runtime path does not exist: {}",
            display_path(&source_path)
        ));
    }

    let metadata = fs::metadata(&source_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!(
            "runtime path must be a file: {}",
            display_path(&source_path)
        ));
    }

    let file_name = source_path.file_name().ok_or_else(|| {
        format!(
            "runtime path must include a file name: {}",
            display_path(&source_path)
        )
    })?;
    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    install_runtime_at_path(
        name,
        meta_path,
        install_root,
        install_files,
        Path::new(file_name),
        Some(&source_path),
        None,
        trim_description(options.description),
    )
}

pub fn install_runtime_from_url(
    options: InstallRuntimeFromUrlOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = runtime_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("runtime \"{name}\" already exists"));
    }

    let file_name = artifact_file_name_from_url(&options.url)?;
    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    install_runtime_at_path(
        name,
        meta_path,
        install_root,
        install_files,
        Path::new(&file_name),
        None,
        Some(options.url),
        trim_description(options.description),
    )
}

pub fn verify_runtime_binary(meta: RuntimeMeta) -> Result<RuntimeMeta, String> {
    let binary_path = Path::new(&meta.binary_path);
    if !path_exists(binary_path) {
        return Err(format!(
            "runtime \"{}\" binary path does not exist: {}",
            meta.name,
            display_path(binary_path)
        ));
    }

    let metadata = fs::metadata(binary_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!(
            "runtime \"{}\" binary path is not a file: {}",
            meta.name,
            display_path(binary_path)
        ));
    }

    Ok(meta)
}
