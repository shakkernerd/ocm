mod common;
mod envs;
mod launchers;
mod layout;
mod runtimes;
mod snapshots;

use std::collections::BTreeMap;
use std::path::Path;

use time::OffsetDateTime;

use crate::env::EnvMeta;
use crate::env::EnvSummary;
use common::ensure_dir;
pub use envs::{
    clone_environment, create_environment, export_environment, get_environment, import_environment,
    list_environments, remove_environment, repair_environment_marker, save_environment,
};
pub use launchers::{add_launcher, get_launcher, list_launchers, remove_launcher};
pub use layout::{
    EnvPaths, StorePaths, clean_path, default_env_root, derive_env_paths, display_path,
    env_meta_path, launcher_meta_path, resolve_absolute_path, resolve_ocm_home,
    resolve_store_paths, resolve_user_home, runtime_install_files_dir, runtime_install_root,
    runtime_meta_path, snapshot_archive_path, snapshot_env_dir, snapshot_meta_path, validate_name,
};
pub use runtimes::{
    add_runtime, get_runtime, get_runtime_verified, install_runtime, install_runtime_from_release,
    install_runtime_from_url, list_runtimes, remove_runtime, runtime_integrity_issue,
};
pub use snapshots::{
    create_env_snapshot, get_env_snapshot, list_all_env_snapshots, list_env_snapshots,
    remove_env_snapshot, restore_env_snapshot, summarize_snapshot,
};

pub fn now_utc() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

pub fn ensure_store(env: &BTreeMap<String, String>, cwd: &Path) -> Result<StorePaths, String> {
    let stores = resolve_store_paths(env, cwd)?;
    ensure_dir(&stores.home)?;
    ensure_dir(&stores.envs_dir)?;
    ensure_dir(&stores.launchers_dir)?;
    ensure_dir(&stores.runtimes_dir)?;
    ensure_dir(&stores.snapshots_dir)?;
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
        default_runtime: meta.default_runtime.clone(),
        default_launcher: meta.default_launcher.clone(),
        protected: meta.protected,
        created_at: meta.created_at,
        last_used_at: meta.last_used_at,
    }
}
