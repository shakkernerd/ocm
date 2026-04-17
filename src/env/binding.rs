use super::{EnvMeta, EnvironmentService};
use crate::store::{get_environment, get_launcher, get_runtime_verified, save_environment};
use crate::supervisor::sync_supervisor_if_present;

impl<'a> EnvironmentService<'a> {
    pub fn set_launcher(&self, name: &str, launcher_name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if launcher_name.eq_ignore_ascii_case("none") {
            meta.default_launcher = None;
        } else {
            get_launcher(launcher_name, self.env, self.cwd)?;
            meta.default_launcher = Some(launcher_name.to_string());
            meta.default_runtime = None;
        }
        let meta = save_environment(meta, self.env, self.cwd)?;
        sync_supervisor_if_present(self.env, self.cwd)?;
        Ok(meta)
    }

    pub fn set_runtime(&self, name: &str, runtime_name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if runtime_name.eq_ignore_ascii_case("none") {
            meta.default_runtime = None;
        } else {
            get_runtime_verified(runtime_name, self.env, self.cwd)?;
            meta.default_runtime = Some(runtime_name.to_string());
            meta.default_launcher = None;
        }
        let meta = save_environment(meta, self.env, self.cwd)?;
        sync_supervisor_if_present(self.env, self.cwd)?;
        Ok(meta)
    }
}
