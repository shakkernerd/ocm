use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::env::{
    CloneEnvironmentOptions, CreateEnvironmentOptions, EnvExportSummary, EnvImportSummary, EnvMeta,
    ExportEnvironmentOptions, ImportEnvironmentOptions,
};
use crate::infra::archive::{
    ArchivedEnvMeta, EnvArchiveMetadata, extract_env_archive, write_env_archive_with_options,
};
use crate::openclaw_repo::remove_openclaw_worktree;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::common::{copy_dir_recursive, ensure_dir, path_exists, read_json, write_json};
use super::gateway_ports::{
    DEFAULT_GATEWAY_PORT, choose_available_gateway_port, resolve_effective_gateway_ports,
    resolve_env_gateway_port,
};
use super::layout::{
    clean_path, default_env_root, derive_env_paths, display_path, env_registry_path,
    resolve_absolute_path, resolve_store_paths, validate_name,
};
use super::now_utc;
use super::{
    clear_nonportable_runtime_state, openclaw_env_archive_options,
    rewrite_openclaw_config_for_target,
};

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
    registry: &mut EnvRegistry,
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

struct EnvRegistryLock {
    file: File,
}

pub(crate) struct EnvironmentOperationLock {
    file: File,
}

impl Drop for EnvRegistryLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

impl Drop for EnvironmentOperationLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

pub(crate) fn lock_environment_operation(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvironmentOperationLock, String> {
    // Every env binding, service, snapshot, and guarded-destroy mutation shares
    // this lock. Bypassing it can make an accepted destroy token stale.
    let safe_name = validate_name(name, "Environment name")?;
    let lock_dir = resolve_store_paths(env, cwd)?
        .home
        .join("locks")
        .join("environments");
    ensure_dir(&lock_dir)?;
    let lock_path = lock_dir.join(format!("{safe_name}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            format!(
                "failed to open environment operation lock {}: {error}",
                display_path(&lock_path)
            )
        })?;
    file.lock_exclusive().map_err(|error| {
        format!(
            "failed to lock environment operation {}: {error}",
            display_path(&lock_path)
        )
    })?;
    Ok(EnvironmentOperationLock { file })
}

fn lock_env_registry(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvRegistryLock, String> {
    // Keep load/allocate/write under one cross-process lock. Locking only the
    // final rename loses concurrent entries and can assign duplicate ports.
    let registry_path = env_registry_path(env, cwd)?;
    let parent = registry_path
        .parent()
        .ok_or_else(|| "environment registry has no parent directory".to_string())?;
    ensure_dir(parent)?;
    let lock_path = registry_path.with_extension("lock");
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| {
            format!(
                "failed to open environment registry lock {}: {error}",
                display_path(&lock_path)
            )
        })?;
    file.lock_exclusive().map_err(|error| {
        format!(
            "failed to lock environment registry {}: {error}",
            display_path(&lock_path)
        )
    })?;
    Ok(EnvRegistryLock { file })
}

fn normalize_environment(mut meta: EnvMeta) -> Result<EnvMeta, String> {
    meta.name = validate_name(&meta.name, "Environment name")?;
    meta.kind = "ocm-env".to_string();
    meta.root = display_path(&clean_path(Path::new(&meta.root)));
    meta.updated_at = now_utc();
    Ok(meta)
}

fn upsert_environment(
    registry: &mut EnvRegistry,
    meta: EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let mut meta = normalize_environment(meta)?;
    let existing_launcher =
        find_environment(registry, &meta.name).and_then(|existing| existing.default_launcher);
    if meta.default_launcher != existing_launcher
        && let Some(launcher_name) = meta.default_launcher.as_deref()
    {
        let launcher = super::launchers::get_launcher(launcher_name, env, cwd)?;
        meta.default_launcher = Some(launcher.name);
    }
    registry.envs.retain(|entry| entry.name != meta.name);
    registry.envs.push(meta.clone());
    Ok(meta)
}

fn find_environment(registry: &EnvRegistry, name: &str) -> Option<EnvMeta> {
    registry.envs.iter().find(|meta| meta.name == name).cloned()
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
    let _lock = lock_env_registry(env, cwd)?;
    let mut registry = load_env_registry(env, cwd)?;
    meta = upsert_environment(&mut registry, meta, env, cwd)?;
    write_env_registry(&mut registry, env, cwd)?;

    Ok(meta)
}

pub fn create_environment(
    options: CreateEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let name = validate_name(&options.name, "Environment name")?;
    let _lock = lock_env_registry(env, cwd)?;
    let mut registry = load_env_registry(env, cwd)?;
    if find_environment(&registry, &name).is_some() {
        return Err(format!("environment \"{name}\" already exists"));
    }
    let default_launcher = options
        .default_launcher
        .as_deref()
        .map(|launcher_name| super::launchers::get_launcher(launcher_name, env, cwd))
        .transpose()?
        .map(|launcher| launcher.name);

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

    let gateway_port_auto_assigned = options.gateway_port.is_none();
    let gateway_port = options.gateway_port.or_else(|| {
        Some(choose_available_gateway_port(
            DEFAULT_GATEWAY_PORT,
            &registry.envs,
            env,
        ))
    });
    let created_at = now_utc();
    let meta = EnvMeta {
        kind: "ocm-env".to_string(),
        name,
        root: display_path(&paths.root),
        gateway_port,
        gateway_port_auto_assigned,
        service_enabled: options.service_enabled,
        service_running: options.service_running,
        default_runtime: options.default_runtime,
        default_launcher,
        dev: options.dev,
        protected: options.protected,
        created_at,
        updated_at: created_at,
        last_used_at: None,
    };
    let meta = upsert_environment(&mut registry, meta, env, cwd)?;
    write_env_registry(&mut registry, env, cwd)?;
    Ok(meta)
}

pub fn clone_environment(
    options: CloneEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let source_name = validate_name(&options.source_name, "Environment name")?;
    let name = validate_name(&options.name, "Environment name")?;
    let _lock = lock_env_registry(env, cwd)?;
    let mut registry = load_env_registry(env, cwd)?;
    let source = find_environment(&registry, &source_name)
        .ok_or_else(|| format!("environment \"{source_name}\" does not exist"))?;
    if find_environment(&registry, &name).is_some() {
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
        let gateway_port = choose_cloned_gateway_port(&source, &registry.envs, env);
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
            gateway_port_auto_assigned: false,
            service_enabled: false,
            service_running: false,
            default_runtime: source.default_runtime,
            default_launcher: source.default_launcher,
            dev: None,
            protected: source.protected,
            created_at,
            updated_at: created_at,
            last_used_at: None,
        };
        let meta = upsert_environment(&mut registry, meta, env, cwd)?;
        write_env_registry(&mut registry, env, cwd)?;
        Ok(meta)
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&target_paths.root);
    }

    result
}

fn choose_cloned_gateway_port(
    source: &EnvMeta,
    envs: &[EnvMeta],
    env: &BTreeMap<String, String>,
) -> u32 {
    let effective_ports = resolve_effective_gateway_ports(envs, env);
    let preferred = effective_ports
        .get(&source.name)
        .copied()
        .or_else(|| resolve_env_gateway_port(source))
        .unwrap_or(DEFAULT_GATEWAY_PORT);

    choose_available_gateway_port(preferred, envs, env)
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
            gateway_port_auto_assigned: meta.gateway_port_auto_assigned,
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

    let result = write_env_archive_with_options(
        &metadata,
        &env_paths.root,
        &output_path,
        openclaw_env_archive_options(),
    );
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
        let _lock = lock_env_registry(env, cwd)?;
        let mut registry = load_env_registry(env, cwd)?;
        if find_environment(&registry, &name).is_some() {
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
            let preferred_gateway_port = extracted
                .metadata
                .env
                .gateway_port
                .unwrap_or(DEFAULT_GATEWAY_PORT);
            let gateway_port =
                choose_available_gateway_port(preferred_gateway_port, &registry.envs, env);
            copy_dir_recursive(&extracted.root_dir, &target_paths.root)?;
            rewrite_openclaw_config_for_target(
                &target_paths,
                extracted.metadata.env.source_root.as_deref().map(Path::new),
                Some(gateway_port),
            )?;
            clear_nonportable_runtime_state(&target_paths)?;

            let created_at = now_utc();
            let meta = EnvMeta {
                kind: "ocm-env".to_string(),
                name: name.clone(),
                root: display_path(&target_paths.root),
                gateway_port: Some(gateway_port),
                gateway_port_auto_assigned: false,
                service_enabled: false,
                service_running: false,
                default_runtime: extracted.metadata.env.default_runtime.clone(),
                default_launcher: extracted.metadata.env.default_launcher.clone(),
                dev: None,
                protected: extracted.metadata.env.protected,
                created_at,
                updated_at: created_at,
                last_used_at: None,
            };
            let meta = upsert_environment(&mut registry, meta, env, cwd)?;
            write_env_registry(&mut registry, env, cwd)?;
            Ok(meta)
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
    let safe_name = validate_name(name, "Environment name")?;
    let _lock = lock_env_registry(env, cwd)?;
    let mut registry = load_env_registry(env, cwd)?;
    let meta = find_environment(&registry, &safe_name)
        .ok_or_else(|| format!("environment \"{safe_name}\" does not exist"))?;
    if meta.protected && !force {
        return Err(format!(
            "environment \"{}\" is protected; re-run with --force",
            meta.name
        ));
    }

    let paths = derive_env_paths(Path::new(&meta.root));

    if let Some(dev) = meta.dev.as_ref() {
        remove_openclaw_worktree(Path::new(&dev.repo_root), Path::new(&dev.worktree_root))?;
    }

    if path_exists(&paths.root) {
        fs::remove_dir_all(&paths.root).map_err(|error| error.to_string())?;
    }

    registry.envs.retain(|entry| entry.name != meta.name);
    write_env_registry(&mut registry, env, cwd)?;

    Ok(meta)
}

pub(super) fn with_locked_environments<T>(
    env: &BTreeMap<String, String>,
    cwd: &Path,
    action: impl FnOnce(&[EnvMeta]) -> Result<T, String>,
) -> Result<T, String> {
    let _lock = lock_env_registry(env, cwd)?;
    let registry = load_env_registry(env, cwd)?;
    action(&registry.envs)
}

fn import_staging_dir() -> PathBuf {
    let id = NEXT_IMPORT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("ocm-env-imports")
        .join(format!("{}-{id}", std::process::id()))
}
