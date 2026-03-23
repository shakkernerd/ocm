use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::execution::{build_launcher_command, resolve_launcher_name, resolve_launcher_run_dir};
use crate::store::{
    create_environment, get_environment, get_launcher, list_environments, now_utc,
    remove_environment, save_environment, select_prune_candidates,
};
use crate::types::{CreateEnvironmentOptions, EnvMeta};

pub struct ResolvedLauncherExecution {
    pub env: EnvMeta,
    pub command: String,
    pub run_dir: PathBuf,
}

pub struct EnvironmentService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> EnvironmentService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn create(&self, options: CreateEnvironmentOptions) -> Result<EnvMeta, String> {
        if let Some(launcher_name) = options.default_launcher.as_deref() {
            get_launcher(launcher_name, self.env, self.cwd)?;
        }
        create_environment(options, self.env, self.cwd)
    }

    pub fn list(&self) -> Result<Vec<EnvMeta>, String> {
        list_environments(self.env, self.cwd)
    }

    pub fn get(&self, name: &str) -> Result<EnvMeta, String> {
        get_environment(name, self.env, self.cwd)
    }

    pub fn touch(&self, name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        meta.last_used_at = Some(now_utc());
        save_environment(meta, self.env, self.cwd)
    }

    pub fn set_launcher(&self, name: &str, launcher_name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if launcher_name.eq_ignore_ascii_case("none") {
            meta.default_launcher = None;
        } else {
            get_launcher(launcher_name, self.env, self.cwd)?;
            meta.default_launcher = Some(launcher_name.to_string());
        }
        save_environment(meta, self.env, self.cwd)
    }

    pub fn set_protected(&self, name: &str, protected: bool) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        meta.protected = protected;
        save_environment(meta, self.env, self.cwd)
    }

    pub fn remove(&self, name: &str, force: bool) -> Result<EnvMeta, String> {
        remove_environment(name, force, self.env, self.cwd)
    }

    pub fn prune_candidates(&self, older_than_days: i64) -> Result<Vec<EnvMeta>, String> {
        let envs = list_environments(self.env, self.cwd)?;
        Ok(select_prune_candidates(&envs, older_than_days))
    }

    pub fn prune(&self, older_than_days: i64) -> Result<Vec<EnvMeta>, String> {
        let candidates = self.prune_candidates(older_than_days)?;
        let mut removed = Vec::with_capacity(candidates.len());
        for meta in candidates {
            removed.push(remove_environment(&meta.name, false, self.env, self.cwd)?);
        }
        Ok(removed)
    }

    pub fn resolve_run(
        &self,
        name: &str,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedLauncherExecution, String> {
        let env = self.touch(name)?;
        let launcher_name = resolve_launcher_name(&env, launcher_override)?;
        let launcher = get_launcher(&launcher_name, self.env, self.cwd)?;

        Ok(ResolvedLauncherExecution {
            command: build_launcher_command(&launcher, args),
            run_dir: resolve_launcher_run_dir(&launcher, self.cwd),
            env,
        })
    }
}
