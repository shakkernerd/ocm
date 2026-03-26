use super::EnvironmentService;
use crate::store::{
    clone_environment, create_environment, export_environment, get_environment, get_launcher,
    get_runtime_verified, import_environment, list_environments, now_utc, remove_environment,
    save_environment, select_prune_candidates,
};
use crate::types::{
    CloneEnvironmentOptions, CreateEnvironmentOptions, EnvExportSummary, EnvImportSummary, EnvMeta,
    ExportEnvironmentOptions, ImportEnvironmentOptions,
};

impl<'a> EnvironmentService<'a> {
    pub fn create(&self, options: CreateEnvironmentOptions) -> Result<EnvMeta, String> {
        if let Some(runtime_name) = options.default_runtime.as_deref() {
            get_runtime_verified(runtime_name, self.env, self.cwd)?;
        }
        if let Some(launcher_name) = options.default_launcher.as_deref() {
            get_launcher(launcher_name, self.env, self.cwd)?;
        }
        create_environment(options, self.env, self.cwd)
    }

    pub fn clone(&self, options: CloneEnvironmentOptions) -> Result<EnvMeta, String> {
        clone_environment(options, self.env, self.cwd)
    }

    pub fn export(&self, options: ExportEnvironmentOptions) -> Result<EnvExportSummary, String> {
        export_environment(options, self.env, self.cwd)
    }

    pub fn import(&self, options: ImportEnvironmentOptions) -> Result<EnvImportSummary, String> {
        import_environment(options, self.env, self.cwd)
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
}
