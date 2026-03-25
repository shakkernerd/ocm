use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::archive::{ArchivedEnvMeta, EnvArchiveManifest, write_env_archive};
use crate::paths::{
    derive_env_paths, display_path, snapshot_archive_path, snapshot_env_dir, snapshot_meta_path,
    validate_name,
};
use crate::types::{CreateEnvSnapshotOptions, EnvSnapshotMeta, EnvSnapshotSummary};

use super::common::{load_json_files, path_exists, read_json, write_json};
use super::{get_environment, now_utc};

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

    let manifest = EnvArchiveManifest {
        kind: "ocm-env-archive".to_string(),
        format_version: 1,
        exported_at: created_at,
        env: ArchivedEnvMeta {
            name: meta.name.clone(),
            gateway_port: meta.gateway_port,
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
        default_runtime: meta.default_runtime.clone(),
        default_launcher: meta.default_launcher.clone(),
        protected: meta.protected,
        created_at,
    };

    let result = (|| {
        write_env_archive(&manifest, &env_paths.root, &archive_path)?;
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
        default_runtime: meta.default_runtime.clone(),
        default_launcher: meta.default_launcher.clone(),
        protected: meta.protected,
        created_at: meta.created_at,
    }
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
