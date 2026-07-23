use super::{EnvMeta, EnvironmentService};
use crate::store::{
    get_environment, save_environment, save_environment_with_validated_launcher,
    save_environment_with_validated_runtime,
};
use crate::supervisor::sync_supervisor_if_present;

impl<'a> EnvironmentService<'a> {
    pub fn set_launcher(&self, name: &str, launcher_name: &str) -> Result<EnvMeta, String> {
        let _lock = self.lock_operation(name)?;
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if launcher_name.eq_ignore_ascii_case("none") {
            meta.default_launcher = None;
        } else {
            meta.default_launcher = Some(launcher_name.to_string());
            meta.default_runtime = None;
        }
        let meta = save_environment_with_validated_launcher(meta, self.env, self.cwd)?;
        sync_supervisor_if_present(self.env, self.cwd)?;
        Ok(meta)
    }

    pub fn set_runtime(&self, name: &str, runtime_name: &str) -> Result<EnvMeta, String> {
        let _lock = self.lock_operation(name)?;
        self.set_runtime_locked(name, runtime_name)
    }

    pub(crate) fn set_runtime_locked(
        &self,
        name: &str,
        runtime_name: &str,
    ) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if runtime_name.eq_ignore_ascii_case("none") {
            meta.default_runtime = None;
        } else {
            meta.default_runtime = Some(runtime_name.to_string());
            meta.default_launcher = None;
        }
        let meta = if meta.default_runtime.is_some() {
            save_environment_with_validated_runtime(meta, self.env, self.cwd)?
        } else {
            save_environment(meta, self.env, self.cwd)?
        };
        sync_supervisor_if_present(self.env, self.cwd)?;
        Ok(meta)
    }
}
