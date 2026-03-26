use super::{
    EnvStatusSummary, EnvironmentService, ExecutionBinding, resolve_execution_binding,
    resolve_runtime_run_dir,
};
use crate::launcher::resolve_launcher_run_dir;
use crate::store::{get_launcher, runtime_integrity_issue};

impl<'a> EnvironmentService<'a> {
    pub fn status(&self, name: &str) -> Result<EnvStatusSummary, String> {
        let env = self.get(name)?;
        let mut summary = EnvStatusSummary {
            env_name: env.name.clone(),
            root: env.root.clone(),
            default_runtime: env.default_runtime.clone(),
            default_launcher: env.default_launcher.clone(),
            resolved_kind: None,
            resolved_name: None,
            binary_path: None,
            command: None,
            run_dir: None,
            runtime_source_kind: None,
            runtime_release_version: None,
            runtime_release_channel: None,
            runtime_health: None,
            issue: None,
        };

        match resolve_execution_binding(&env, None, None) {
            Ok(ExecutionBinding::Runtime(runtime_name)) => {
                summary.resolved_kind = Some("runtime".to_string());
                summary.resolved_name = Some(runtime_name.clone());
                match crate::store::get_runtime(&runtime_name, self.env, self.cwd) {
                    Ok(runtime) => {
                        summary.binary_path = Some(runtime.binary_path.clone());
                        summary.run_dir =
                            Some(resolve_runtime_run_dir(self.cwd).display().to_string());
                        summary.runtime_source_kind =
                            Some(runtime.source_kind.as_str().to_string());
                        summary.runtime_release_version = runtime.release_version.clone();
                        summary.runtime_release_channel = runtime.release_channel.clone();
                        match runtime_integrity_issue(&runtime) {
                            None => summary.runtime_health = Some("ok".to_string()),
                            Some(error) => {
                                summary.runtime_health = Some("broken".to_string());
                                summary.issue =
                                    Some(format!("runtime \"{}\" {error}", runtime.name));
                            }
                        }
                    }
                    Err(error) => {
                        summary.runtime_health = Some("missing".to_string());
                        summary.issue = Some(error);
                    }
                }
            }
            Ok(ExecutionBinding::Launcher(launcher_name)) => {
                summary.resolved_kind = Some("launcher".to_string());
                summary.resolved_name = Some(launcher_name.clone());
                match get_launcher(&launcher_name, self.env, self.cwd) {
                    Ok(launcher) => {
                        summary.command = Some(launcher.command.clone());
                        summary.run_dir = Some(
                            resolve_launcher_run_dir(&launcher, self.cwd)
                                .display()
                                .to_string(),
                        );
                    }
                    Err(error) => summary.issue = Some(error),
                }
            }
            Err(error) => summary.issue = Some(error),
        }

        Ok(summary)
    }
}
