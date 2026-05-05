mod common;
mod envs;
mod gateway_ports;
mod launchers;
mod layout;
mod openclaw_config;
mod openclaw_state;
mod runtimes;
mod snapshots;

use std::collections::BTreeMap;
use std::path::Path;

use time::OffsetDateTime;

use crate::env::EnvMeta;
use crate::env::EnvSummary;
pub(crate) use common::{copy_dir_recursive, ensure_dir, read_json, write_json};
pub use envs::{
    clone_environment, create_environment, export_environment, get_environment, import_environment,
    list_environments, remove_environment, save_environment,
};
pub(crate) use gateway_ports::{
    openclaw_port_family_available, openclaw_port_family_range, resolve_effective_gateway_ports,
    resolve_env_gateway_port,
};
pub use launchers::{add_launcher, get_launcher, list_launchers, remove_launcher};
pub use layout::{
    EnvPaths, StorePaths, clean_path, default_env_root, derive_env_paths, display_path,
    env_registry_path, launcher_meta_path, resolve_absolute_path, resolve_ocm_home,
    resolve_store_paths, resolve_user_home, runtime_install_files_dir, runtime_install_root,
    runtime_meta_path, snapshot_archive_path, snapshot_env_dir, snapshot_meta_path,
    supervisor_logs_dir, supervisor_runtime_path, supervisor_state_path, validate_name,
};
pub(crate) use openclaw_config::{
    OpenClawConfigAudit, audit_openclaw_config, clear_skip_bootstrap_for_openclaw_onboarding,
    ensure_minimum_local_openclaw_config, repair_openclaw_config,
    rewrite_openclaw_config_for_target,
};
pub(crate) use openclaw_state::{
    OpenClawStateAudit, audit_openclaw_state, clear_nonportable_runtime_state,
    openclaw_env_archive_options, prepare_migrated_runtime_state, repair_openclaw_runtime_state,
};
pub(crate) use runtimes::install_runtime_from_local_openclaw_build;
pub(crate) use runtimes::install_runtime_from_selected_official_openclaw_release;
pub(crate) use runtimes::{BuildLocalRuntimeOptions, InstallContext, RuntimeReleaseDetails};
pub use runtimes::{
    add_runtime, get_runtime, get_runtime_verified, install_runtime,
    install_runtime_from_official_openclaw_release, install_runtime_from_release,
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
    ensure_dir(&stores.supervisor_dir)?;
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
        service_enabled: meta.service_enabled,
        service_running: meta.service_running,
        default_runtime: meta.default_runtime.clone(),
        default_launcher: meta.default_launcher.clone(),
        dev_repo_root: meta.dev.as_ref().map(|dev| dev.repo_root.clone()),
        dev_worktree_root: meta.dev.as_ref().map(|dev| dev.worktree_root.clone()),
        protected: meta.protected,
        created_at: meta.created_at,
        last_used_at: meta.last_used_at,
    }
}
