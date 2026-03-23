mod common;
mod environments;
mod launchers;
mod prune;

use std::collections::BTreeMap;
use std::path::Path;

use time::OffsetDateTime;

use crate::paths::{derive_env_paths, display_path, resolve_store_paths};
use crate::types::{EnvMeta, EnvSummary, StorePaths};
use common::ensure_dir;
pub use environments::{
    create_environment, get_environment, list_environments, remove_environment, save_environment,
};
pub use launchers::{add_launcher, get_launcher, list_launchers, remove_launcher};
pub use prune::select_prune_candidates;

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
