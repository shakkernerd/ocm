use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::execution::{
    ExecutionBinding, build_launcher_command, resolve_execution_binding, resolve_launcher_run_dir,
    resolve_runtime_run_dir,
};
use crate::store::{
    clone_environment, create_environment, get_environment, get_launcher, get_runtime_verified,
    list_environments, now_utc, remove_environment, runtime_integrity_issue, save_environment,
    select_prune_candidates,
};
use crate::types::EnvStatusSummary;
use crate::types::{CloneEnvironmentOptions, CreateEnvironmentOptions, EnvMeta, ExecutionSummary};

pub enum ResolvedExecution {
    Launcher {
        env: EnvMeta,
        launcher_name: String,
        command: String,
        run_dir: PathBuf,
    },
    Runtime {
        env: EnvMeta,
        runtime_name: String,
        binary_path: String,
        args: Vec<String>,
        run_dir: PathBuf,
    },
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

    pub fn set_runtime(&self, name: &str, runtime_name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if runtime_name.eq_ignore_ascii_case("none") {
            meta.default_runtime = None;
        } else {
            get_runtime_verified(runtime_name, self.env, self.cwd)?;
            meta.default_runtime = Some(runtime_name.to_string());
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

    pub fn resolve(
        &self,
        name: &str,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        let env = self.get(name)?;
        self.resolve_execution(env, runtime_override, launcher_override, args)
    }

    pub fn resolve_run(
        &self,
        name: &str,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        let env = self.touch(name)?;
        self.resolve_execution(env, runtime_override, launcher_override, args)
    }

    fn resolve_execution(
        &self,
        env: EnvMeta,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        match resolve_execution_binding(&env, runtime_override, launcher_override)? {
            ExecutionBinding::Launcher(launcher_name) => {
                let launcher = get_launcher(&launcher_name, self.env, self.cwd)?;
                Ok(ResolvedExecution::Launcher {
                    launcher_name,
                    command: build_launcher_command(&launcher, args),
                    run_dir: resolve_launcher_run_dir(&launcher, self.cwd),
                    env,
                })
            }
            ExecutionBinding::Runtime(runtime_name) => {
                let runtime = get_runtime_verified(&runtime_name, self.env, self.cwd)?;
                Ok(ResolvedExecution::Runtime {
                    runtime_name,
                    binary_path: runtime.binary_path,
                    args: args.to_vec(),
                    run_dir: resolve_runtime_run_dir(self.cwd),
                    env,
                })
            }
        }
    }
}

impl ResolvedExecution {
    pub fn into_summary(self) -> ExecutionSummary {
        match self {
            Self::Launcher {
                env,
                launcher_name,
                command,
                run_dir,
            } => ExecutionSummary {
                env_name: env.name,
                binding_kind: "launcher".to_string(),
                binding_name: launcher_name,
                command: Some(command),
                binary_path: None,
                forwarded_args: Vec::new(),
                run_dir: run_dir.display().to_string(),
            },
            Self::Runtime {
                env,
                runtime_name,
                binary_path,
                args,
                run_dir,
            } => ExecutionSummary {
                env_name: env.name,
                binding_kind: "runtime".to_string(),
                binding_name: runtime_name,
                command: None,
                binary_path: Some(binary_path),
                forwarded_args: args,
                run_dir: run_dir.display().to_string(),
            },
        }
    }
}
