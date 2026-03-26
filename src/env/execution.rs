use std::path::PathBuf;

use super::EnvironmentService;
use crate::execution::{
    ExecutionBinding, build_launcher_command, resolve_execution_binding, resolve_launcher_run_dir,
    resolve_runtime_run_dir,
};
use crate::store::{get_launcher, get_runtime_verified};
use crate::types::{EnvMeta, ExecutionSummary};

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

impl<'a> EnvironmentService<'a> {
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
