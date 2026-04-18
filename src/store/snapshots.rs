use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::env::{
    CreateEnvSnapshotOptions, EnvMarker, EnvMeta, EnvSnapshotRemoveSummary,
    EnvSnapshotRestoreSummary, EnvSnapshotSummary, RemoveEnvSnapshotOptions,
    RestoreEnvSnapshotOptions, default_service_enabled, default_service_running,
};
use crate::infra::archive::{
    ArchivedEnvMeta, EnvArchiveMetadata, extract_env_archive, write_env_archive,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::common::{copy_dir_recursive, load_json_files, path_exists, read_json, write_json};
use super::layout::{
    derive_env_paths, display_path, snapshot_archive_path, snapshot_env_dir, snapshot_meta_path,
    validate_name,
};
use super::{
    audit_openclaw_state, clear_nonportable_runtime_state, get_environment, list_environments,
    now_utc, rewrite_openclaw_config_for_target, save_environment,
};

static NEXT_RESTORE_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotMeta {
    pub kind: String,
    pub id: String,
    pub env_name: String,
    #[serde(default)]
    pub label: Option<String>,
    pub archive_path: String,
    pub source_root: String,
    pub gateway_port: Option<u32>,
    #[serde(default = "default_service_enabled")]
    pub service_enabled: bool,
    #[serde(default = "default_service_running")]
    pub service_running: bool,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub fn create_env_snapshot(
    options: CreateEnvSnapshotOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvSnapshotMeta, String> {
    let env_name = validate_name(&options.env_name, "Environment name")?;
    let meta = get_environment(&env_name, env, cwd)?;
    let env_paths = derive_env_paths(Path::new(&meta.root));
    if !path_exists(&env_paths.root) {
        return Err(format!(
            "environment root does not exist: {}",
            display_path(&env_paths.root)
        ));
    }
    if !path_exists(&env_paths.marker_path) {
        return Err(format!(
            "refusing to snapshot {} without {}",
            display_path(&env_paths.root),
            env_paths
                .marker_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(".ocm-env.json")
        ));
    }

    let created_at = now_utc();
    let snapshot_id = format!(
        "{}-{:09}",
        created_at.unix_timestamp(),
        created_at.nanosecond()
    );
    let archive_path = snapshot_archive_path(&env_name, &snapshot_id, env, cwd)?;
    let meta_path = snapshot_meta_path(&env_name, &snapshot_id, env, cwd)?;

    let metadata = EnvArchiveMetadata {
        kind: "ocm-env-archive".to_string(),
        format_version: 1,
        exported_at: created_at,
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

    let snapshot = EnvSnapshotMeta {
        kind: "ocm-env-snapshot".to_string(),
        id: snapshot_id,
        env_name: meta.name.clone(),
        label: options.label,
        archive_path: display_path(&archive_path),
        source_root: meta.root.clone(),
        gateway_port: meta.gateway_port,
        service_enabled: meta.service_enabled,
        service_running: meta.service_running,
        default_runtime: meta.default_runtime.clone(),
        default_launcher: meta.default_launcher.clone(),
        protected: meta.protected,
        created_at,
    };

    let result = (|| {
        write_env_archive(&metadata, &env_paths.root, &archive_path)?;
        write_json(&meta_path, &snapshot)?;
        Ok(snapshot)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&archive_path);
        let _ = fs::remove_file(&meta_path);
    }

    result
}

pub fn get_env_snapshot(
    env_name: &str,
    snapshot_id: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvSnapshotMeta, String> {
    let safe_env_name = validate_name(env_name, "Environment name")?;
    let safe_snapshot_id = snapshot_id.trim();
    if safe_snapshot_id.is_empty() {
        return Err("snapshot id is required".to_string());
    }

    let path = snapshot_meta_path(&safe_env_name, safe_snapshot_id, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!(
            "snapshot \"{}\" does not exist for environment \"{}\"",
            safe_snapshot_id, safe_env_name
        ));
    }
    read_json(&path)
}

pub fn summarize_snapshot(meta: &EnvSnapshotMeta) -> EnvSnapshotSummary {
    EnvSnapshotSummary {
        id: meta.id.clone(),
        env_name: meta.env_name.clone(),
        label: meta.label.clone(),
        archive_path: meta.archive_path.clone(),
        source_root: meta.source_root.clone(),
        gateway_port: meta.gateway_port,
        service_enabled: meta.service_enabled,
        service_running: meta.service_running,
        default_runtime: meta.default_runtime.clone(),
        default_launcher: meta.default_launcher.clone(),
        protected: meta.protected,
        created_at: meta.created_at,
    }
}

pub fn restore_env_snapshot(
    options: RestoreEnvSnapshotOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvSnapshotRestoreSummary, String> {
    let env_name = validate_name(&options.env_name, "Environment name")?;
    let snapshot = get_env_snapshot(&env_name, &options.snapshot_id, env, cwd)?;
    let current = get_environment(&env_name, env, cwd)?;
    let current_paths = derive_env_paths(Path::new(&current.root));
    let root_exists = path_exists(&current_paths.root);
    let marker_exists = path_exists(&current_paths.marker_path);
    if root_exists && !marker_exists {
        let marker_name = current_paths
            .marker_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(".ocm-env.json");
        return Err(format!(
            "refusing to restore {} without {}",
            display_path(&current_paths.root),
            marker_name
        ));
    }

    let staging_dir = restore_staging_dir();
    let backup_root = restore_backup_root(&current_paths.root);
    if path_exists(&staging_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
    }

    let result = (|| {
        let extracted = extract_env_archive::<EnvArchiveMetadata>(
            Path::new(&snapshot.archive_path),
            &staging_dir,
        )?;
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
        if !path_exists(&extracted.root_dir.join(".ocm-env.json")) {
            return Err("snapshot archive is missing .ocm-env.json".to_string());
        }

        let mut renamed = false;
        if root_exists {
            fs::rename(&current_paths.root, &backup_root).map_err(|error| error.to_string())?;
            renamed = true;
        }

        let restore_result = (|| {
            copy_dir_recursive(&extracted.root_dir, &current_paths.root)?;
            rewrite_openclaw_config_for_target(
                &current_paths,
                Some(Path::new(&snapshot.source_root)),
                extracted.metadata.env.gateway_port,
            )?;
            let marker = EnvMarker {
                kind: "ocm-env-marker".to_string(),
                name: env_name.clone(),
                created_at: now_utc(),
            };
            write_json(&current_paths.marker_path, &marker)?;

            let restored = EnvMeta {
                kind: "ocm-env".to_string(),
                name: current.name.clone(),
                root: current.root.clone(),
                gateway_port: extracted.metadata.env.gateway_port,
                service_enabled: extracted.metadata.env.service_enabled,
                service_running: extracted.metadata.env.service_running,
                default_runtime: extracted.metadata.env.default_runtime.clone(),
                default_launcher: extracted.metadata.env.default_launcher.clone(),
                protected: extracted.metadata.env.protected,
                created_at: current.created_at,
                updated_at: current.updated_at,
                last_used_at: current.last_used_at,
            };
            let known_envs = list_environments(env, cwd)?;
            let audit = audit_openclaw_state(&restored, &known_envs);
            if audit.repair_runtime_state {
                clear_nonportable_runtime_state(&current_paths)?;
            }
            save_environment(restored, env, cwd)
        })();

        match restore_result {
            Ok(meta) => {
                if renamed {
                    let _ = fs::remove_dir_all(&backup_root);
                }
                Ok(EnvSnapshotRestoreSummary {
                    env_name: meta.name,
                    snapshot_id: snapshot.id,
                    label: snapshot.label,
                    root: meta.root,
                    archive_path: snapshot.archive_path,
                    default_runtime: meta.default_runtime,
                    default_launcher: meta.default_launcher,
                    protected: meta.protected,
                })
            }
            Err(error) => {
                let _ = fs::remove_dir_all(&current_paths.root);
                if renamed {
                    let _ = fs::rename(&backup_root, &current_paths.root);
                }
                Err(error)
            }
        }
    })();

    let _ = fs::remove_dir_all(&staging_dir);
    result
}

pub fn remove_env_snapshot(
    options: RemoveEnvSnapshotOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvSnapshotRemoveSummary, String> {
    let snapshot = get_env_snapshot(&options.env_name, &options.snapshot_id, env, cwd)?;
    let meta_path = snapshot_meta_path(&snapshot.env_name, &snapshot.id, env, cwd)?;
    let archive_path = PathBuf::from(&snapshot.archive_path);

    if path_exists(&meta_path) {
        fs::remove_file(&meta_path).map_err(|error| error.to_string())?;
    }
    if path_exists(&archive_path) {
        fs::remove_file(&archive_path).map_err(|error| error.to_string())?;
    }

    remove_snapshot_parent_if_empty(&snapshot.env_name, env, cwd)?;

    Ok(EnvSnapshotRemoveSummary {
        env_name: snapshot.env_name,
        snapshot_id: snapshot.id,
        label: snapshot.label,
        archive_path: snapshot.archive_path,
    })
}

pub fn list_env_snapshots(
    env_name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<EnvSnapshotMeta>, String> {
    let safe_env_name = validate_name(env_name, "Environment name")?;
    let dir = snapshot_env_dir(&safe_env_name, env, cwd)?;
    let files = load_json_files(&dir)?;
    let mut out = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    sort_snapshots(&mut out);
    Ok(out)
}

pub fn list_all_env_snapshots(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<EnvSnapshotMeta>, String> {
    let stores = super::ensure_store(env, cwd)?;
    let mut out = Vec::new();
    let entries = fs::read_dir(&stores.snapshots_dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let files = load_json_files(&path)?;
        for file in files {
            out.push(read_json(&file)?);
        }
    }
    sort_snapshots(&mut out);
    Ok(out)
}

fn sort_snapshots(snapshots: &mut [EnvSnapshotMeta]) {
    snapshots.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn restore_staging_dir() -> PathBuf {
    let id = NEXT_RESTORE_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("ocm-snapshot-restores")
        .join(format!("{}-{id}", std::process::id()))
}

fn restore_backup_root(root: &Path) -> PathBuf {
    let id = NEXT_RESTORE_ID.fetch_add(1, Ordering::Relaxed);
    let backup_name = format!(
        ".{}-ocm-restore-{}-{id}",
        root.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("env"),
        std::process::id()
    );
    root.parent()
        .unwrap_or_else(|| Path::new("."))
        .join(backup_name)
}

fn remove_snapshot_parent_if_empty(
    env_name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(), String> {
    let dir = snapshot_env_dir(env_name, env, cwd)?;
    if !path_exists(&dir) {
        return Ok(());
    }

    let mut entries = fs::read_dir(&dir).map_err(|error| error.to_string())?;
    if entries.next().is_none() {
        fs::remove_dir(&dir).map_err(|error| error.to_string())?;
    }
    Ok(())
}
