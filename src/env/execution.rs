use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{EnvMeta, EnvironmentService};
use crate::launcher::{build_launcher_command, resolve_launcher_run_dir};
use crate::store::{get_launcher, get_runtime_verified};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub forwarded_args: Vec<String>,
    pub run_dir: String,
}

#[derive(Debug)]
pub enum ExecutionBinding {
    Launcher(String),
    Runtime(String),
}

pub fn resolve_execution_binding(
    env_meta: &EnvMeta,
    runtime_override: Option<String>,
    launcher_override: Option<String>,
) -> Result<ExecutionBinding, String> {
    let runtime_override = runtime_override.filter(|value| !value.trim().is_empty());
    let launcher_override = launcher_override.filter(|value| !value.trim().is_empty());

    if runtime_override.is_some() && launcher_override.is_some() {
        return Err("env run accepts only one of --runtime or --launcher".to_string());
    }

    if let Some(runtime_name) = runtime_override {
        return Ok(ExecutionBinding::Runtime(runtime_name));
    }

    if let Some(launcher_name) = launcher_override {
        return Ok(ExecutionBinding::Launcher(launcher_name));
    }

    if let Some(runtime_name) = env_meta.default_runtime.clone() {
        return Ok(ExecutionBinding::Runtime(runtime_name));
    }

    if let Some(launcher_name) = env_meta.default_launcher.clone() {
        return Ok(ExecutionBinding::Launcher(launcher_name));
    }

    Err(format!(
        "environment \"{}\" has no default runtime or launcher; use env set-runtime, env set-launcher, or pass --runtime/--launcher",
        env_meta.name
    ))
}

pub fn resolve_runtime_run_dir(fallback_cwd: &Path) -> PathBuf {
    fallback_cwd.to_path_buf()
}

fn normalize_openclaw_args_for_env(args: &[String]) -> Result<Vec<String>, String> {
    if !matches!(args.first().map(String::as_str), Some("onboard")) {
        return Ok(args.to_vec());
    }

    if args.iter().any(|arg| arg == "--install-daemon") {
        return Err(
            "env onboarding cannot install the OpenClaw daemon because the daemon service is global; rerun with --no-install-daemon or --skip-daemon, or run onboard outside OCM".to_string(),
        );
    }

    if args
        .iter()
        .any(|arg| arg == "--no-install-daemon" || arg == "--skip-daemon")
    {
        return Ok(args.to_vec());
    }

    let mut normalized = args.to_vec();
    normalized.push("--no-install-daemon".to_string());
    Ok(normalized)
}

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
        let env = self.apply_effective_gateway_port(self.get(name)?)?;
        self.resolve_execution(env, runtime_override, launcher_override, args)
    }

    pub fn resolve_run(
        &self,
        name: &str,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        let env = self.apply_effective_gateway_port(self.touch(name)?)?;
        self.resolve_execution(env, runtime_override, launcher_override, args)
    }

    fn resolve_execution(
        &self,
        env: EnvMeta,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        let args = normalize_openclaw_args_for_env(args)?;
        match resolve_execution_binding(&env, runtime_override, launcher_override)? {
            ExecutionBinding::Launcher(launcher_name) => {
                let launcher = get_launcher(&launcher_name, self.env, self.cwd)?;
                Ok(ResolvedExecution::Launcher {
                    launcher_name,
                    command: build_launcher_command(&launcher, &args),
                    run_dir: resolve_launcher_run_dir(&launcher, self.cwd),
                    env,
                })
            }
            ExecutionBinding::Runtime(runtime_name) => {
                let runtime = get_runtime_verified(&runtime_name, self.env, self.cwd)?;
                Ok(ResolvedExecution::Runtime {
                    runtime_name,
                    binary_path: runtime.binary_path,
                    args,
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
