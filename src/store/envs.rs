use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::env::{
    CloneEnvironmentOptions, CreateEnvironmentOptions, EnvExportSummary, EnvImportSummary, EnvMeta,
    ExportEnvironmentOptions, ImportEnvironmentOptions,
};
use crate::infra::archive::{
    ArchivedEnvMeta, EnvArchiveMetadata, extract_env_archive, write_env_archive,
};
use serde::{Deserialize, Serialize};

use super::common::{copy_dir_recursive, ensure_dir, path_exists, read_json, write_json};
use super::gateway_ports::{
    DEFAULT_GATEWAY_PORT, choose_available_gateway_port, resolve_effective_gateway_ports,
    resolve_env_gateway_port,
};
use super::layout::{
    clean_path, default_env_root, derive_env_paths, display_path, env_registry_path,
    resolve_absolute_path, validate_name,
};
use super::now_utc;
use super::{clear_nonportable_runtime_state, rewrite_openclaw_config_for_target};

static NEXT_IMPORT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EnvRegistry {
    kind: String,
    envs: Vec<EnvMeta>,
}

fn empty_env_registry() -> EnvRegistry {
    EnvRegistry {
        kind: "ocm-env-registry".to_string(),
        envs: Vec::new(),
    }
}

fn load_env_registry(env: &BTreeMap<String, String>, cwd: &Path) -> Result<EnvRegistry, String> {
    let path = env_registry_path(env, cwd)?;
    if !path_exists(&path) {
        return Ok(empty_env_registry());
    }

    let mut registry: EnvRegistry = read_json(&path)?;
    registry.kind = "ocm-env-registry".to_string();
    registry
        .envs
        .sort_by(|left, right| left.name.cmp(&right.name));
    Ok(registry)
}

fn write_env_registry(
    mut registry: EnvRegistry,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(), String> {
    registry.kind = "ocm-env-registry".to_string();
    registry
        .envs
        .sort_by(|left, right| left.name.cmp(&right.name));
    let path = env_registry_path(env, cwd)?;
    write_json(&path, &registry)
}

fn environment_exists(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<bool, String> {
    Ok(load_env_registry(env, cwd)?
        .envs
        .iter()
        .any(|meta| meta.name == name))
}

pub fn list_environments(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<EnvMeta>, String> {
    Ok(load_env_registry(env, cwd)?.envs)
}

pub fn get_environment(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let safe_name = validate_name(name, "Environment name")?;
    load_env_registry(env, cwd)?
        .envs
        .into_iter()
        .find(|meta| meta.name == safe_name)
        .ok_or_else(|| format!("environment \"{safe_name}\" does not exist"))
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

    let mut registry = load_env_registry(env, cwd)?;
    registry.envs.retain(|entry| entry.name != meta.name);
    registry.envs.push(meta.clone());
    write_env_registry(registry, env, cwd)?;

    Ok(meta)
}

pub fn create_environment(
    options: CreateEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let name = validate_name(&options.name, "Environment name")?;
    if environment_exists(&name, env, cwd)? {
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
    let meta = EnvMeta {
        kind: "ocm-env".to_string(),
        name,
        root: display_path(&paths.root),
        gateway_port: options.gateway_port,
        service_enabled: options.service_enabled,
        service_running: options.service_running,
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
    if environment_exists(&name, env, cwd)? {
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
    let result = (|| {
        copy_dir_recursive(&source_paths.root, &target_paths.root)?;
        let created_at = now_utc();
        let gateway_port = choose_cloned_gateway_port(&source, env, cwd)?;
        rewrite_openclaw_config_for_target(
            &target_paths,
            Some(&source_paths.root),
            Some(gateway_port),
        )?;
        clear_nonportable_runtime_state(&target_paths)?;

        let meta = EnvMeta {
            kind: "ocm-env".to_string(),
            name,
            root: display_path(&target_paths.root),
            gateway_port: Some(gateway_port),
            service_enabled: source.service_enabled,
            service_running: source.service_running,
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
        let _ = fs::remove_dir_all(&target_paths.root);
    }

    result
}

fn choose_cloned_gateway_port(
    source: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<u32, String> {
    let envs = list_environments(env, cwd)?;
    let effective_ports = resolve_effective_gateway_ports(&envs, env);
    let preferred = effective_ports
        .get(&source.name)
        .copied()
        .or_else(|| resolve_env_gateway_port(source))
        .unwrap_or(DEFAULT_GATEWAY_PORT);

    Ok(choose_available_gateway_port(preferred, &envs, env))
}

pub fn export_environment(
    options: ExportEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvExportSummary, String> {
    let meta = get_environment(&options.name, env, cwd)?;
    let env_paths = derive_env_paths(Path::new(&meta.root));
    if !path_exists(&env_paths.root) {
        return Err(format!(
            "environment root does not exist: {}",
            display_path(&env_paths.root)
        ));
    }

    let output_path = if let Some(output) = options.output.as_deref() {
        resolve_absolute_path(output, env, cwd)?
    } else {
        clean_path(&cwd.join(format!("{}.ocm-env.tar", meta.name)))
    };
    if path_exists(&output_path) {
        return Err(format!(
            "export archive already exists: {}",
            display_path(&output_path)
        ));
    }

    let metadata = EnvArchiveMetadata {
        kind: "ocm-env-archive".to_string(),
        format_version: 1,
        exported_at: now_utc(),
        env: ArchivedEnvMeta {
            name: meta.name.clone(),
            source_root: Some(meta.root.clone()),
            gateway_port: meta.gateway_port,
            service_enabled: meta.service_enabled,
            service_running: meta.service_running,
            default_runtime: meta.default_runtime.clone(),
            default_launcher: meta.default_launcher.clone(),
            protected: meta.protected,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
            last_used_at: meta.last_used_at,
        },
    };

    let result = write_env_archive(&metadata, &env_paths.root, &output_path);
    if result.is_err() {
        let _ = fs::remove_file(&output_path);
    }
    result?;

    Ok(EnvExportSummary {
        name: meta.name,
        root: meta.root,
        archive_path: display_path(&output_path),
        default_runtime: meta.default_runtime,
        default_launcher: meta.default_launcher,
        protected: meta.protected,
    })
}

pub fn import_environment(
    options: ImportEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvImportSummary, String> {
    let archive_path = resolve_absolute_path(&options.archive, env, cwd)?;
    let staging_dir = import_staging_dir();
    if path_exists(&staging_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
    }

    let result = (|| {
        let extracted = extract_env_archive::<EnvArchiveMetadata>(&archive_path, &staging_dir)?;
        if extracted.metadata.kind != "ocm-env-archive" {
            return Err(format!(
                "unsupported archive kind: {}",
                extracted.metadata.kind
            ));
        }
        if extracted.metadata.format_version != 1 {
            return Err(format!(
                "unsupported archive format version: {}",
                extracted.metadata.format_version
            ));
        }

        let source_name = extracted.metadata.env.name.clone();
        let name = if let Some(name) = options.name.as_deref() {
            validate_name(name, "Environment name")?
        } else {
            validate_name(&source_name, "Environment name")?
        };
        if environment_exists(&name, env, cwd)? {
            return Err(format!("environment \"{name}\" already exists"));
        }

        let root = if let Some(root) = options.root.as_deref() {
            resolve_absolute_path(root, env, cwd)?
        } else {
            default_env_root(&name, env, cwd)?
        };
        let target_paths = derive_env_paths(&root);
        if path_exists(&target_paths.root) {
            let mut entries =
                fs::read_dir(&target_paths.root).map_err(|error| error.to_string())?;
            if entries.next().is_some() {
                return Err(format!(
                    "root already exists and is not empty: {}",
                    display_path(&target_paths.root)
                ));
            }
        }

        if !path_exists(&extracted.root_dir) {
            return Err("archive is missing root/".to_string());
        }
        let imported = (|| {
            copy_dir_recursive(&extracted.root_dir, &target_paths.root)?;
            rewrite_openclaw_config_for_target(
                &target_paths,
                extracted.metadata.env.source_root.as_deref().map(Path::new),
                extracted.metadata.env.gateway_port,
            )?;
            clear_nonportable_runtime_state(&target_paths)?;

            let created_at = now_utc();
            let meta = EnvMeta {
                kind: "ocm-env".to_string(),
                name: name.clone(),
                root: display_path(&target_paths.root),
                gateway_port: extracted.metadata.env.gateway_port,
                service_enabled: extracted.metadata.env.service_enabled,
                service_running: extracted.metadata.env.service_running,
                default_runtime: extracted.metadata.env.default_runtime.clone(),
                default_launcher: extracted.metadata.env.default_launcher.clone(),
                protected: extracted.metadata.env.protected,
                created_at,
                updated_at: created_at,
                last_used_at: None,
            };
            save_environment(meta, env, cwd)
        })();

        match imported {
            Ok(meta) => Ok(EnvImportSummary {
                name: meta.name.clone(),
                source_name,
                root: meta.root.clone(),
                archive_path: display_path(&archive_path),
                default_runtime: meta.default_runtime.clone(),
                default_launcher: meta.default_launcher.clone(),
                protected: meta.protected,
            }),
            Err(error) => {
                let _ = fs::remove_dir_all(&target_paths.root);
                Err(error)
            }
        }
    })();

    let _ = fs::remove_dir_all(&staging_dir);
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

    if root_exists {
        fs::remove_dir_all(&paths.root).map_err(|error| error.to_string())?;
    }

    let mut registry = load_env_registry(env, cwd)?;
    registry.envs.retain(|entry| entry.name != meta.name);
    write_env_registry(registry, env, cwd)?;

    Ok(meta)
}

fn import_staging_dir() -> PathBuf {
    let id = NEXT_IMPORT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("ocm-env-imports")
        .join(format!("{}-{id}", std::process::id()))
}
