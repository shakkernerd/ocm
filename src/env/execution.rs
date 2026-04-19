use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{EnvMeta, EnvironmentService};
use crate::infra::shell::build_openclaw_env;
use crate::launcher::{
    build_launcher_command, resolve_direct_launcher_command, resolve_launcher_run_dir,
};
use crate::runtime::resolve_runtime_launch;
use crate::store::{get_launcher, get_runtime_verified};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSummary {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub runtime_source_kind: Option<String>,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub forwarded_args: Vec<String>,
    pub run_dir: String,
}

#[derive(Clone, Debug)]
pub struct GatewayProcessSpec {
    pub env_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub runtime_source_kind: Option<String>,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub args: Vec<String>,
    pub run_dir: PathBuf,
    pub process_env: BTreeMap<String, String>,
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

fn gateway_shell_program_arguments(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".to_string(), "/C".to_string(), command.to_string()]
    } else {
        vec![
            "/bin/sh".to_string(),
            "-lc".to_string(),
            command.to_string(),
        ]
    }
}

pub fn resolve_gateway_process_spec(
    env_meta: &EnvMeta,
    process_env: &BTreeMap<String, String>,
    cwd: &Path,
    bootstrap_managed_node: bool,
) -> Result<GatewayProcessSpec, String> {
    let port = env_meta.gateway_port.ok_or_else(|| {
        format!(
            "failed to resolve gateway port for env \"{}\"",
            env_meta.name
        )
    })?;
    let gateway_args = vec![
        "gateway".to_string(),
        "run".to_string(),
        "--port".to_string(),
        port.to_string(),
    ];

    match resolve_execution_binding(env_meta, None, None)? {
        ExecutionBinding::Launcher(binding_name) => {
            let launcher = get_launcher(&binding_name, process_env, cwd)?;
            let run_dir = resolve_launcher_run_dir(&launcher, Path::new(&env_meta.root));
            let direct = resolve_direct_launcher_command(&launcher, &gateway_args, &run_dir);
            Ok(GatewayProcessSpec {
                env_name: env_meta.name.clone(),
                binding_kind: "launcher".to_string(),
                binding_name,
                command: Some(build_launcher_command(&launcher, &gateway_args)),
                binary_path: direct.as_ref().map(|command| command.program.clone()),
                runtime_source_kind: None,
                runtime_release_version: None,
                runtime_release_channel: None,
                args: direct.map(|command| command.args).unwrap_or_default(),
                run_dir,
                process_env: build_openclaw_env(env_meta, process_env),
            })
        }
        ExecutionBinding::Runtime(binding_name) => {
            let runtime = get_runtime_verified(&binding_name, process_env, cwd)?;
            let launch = resolve_runtime_launch(
                &runtime,
                &gateway_args,
                process_env,
                cwd,
                bootstrap_managed_node,
            )?;
            Ok(GatewayProcessSpec {
                env_name: env_meta.name.clone(),
                binding_kind: "runtime".to_string(),
                binding_name,
                command: None,
                binary_path: Some(launch.program),
                runtime_source_kind: Some(runtime.source_kind.as_str().to_string()),
                runtime_release_version: runtime.release_version.clone(),
                runtime_release_channel: runtime.release_channel.clone(),
                args: launch.args,
                run_dir: Path::new(&env_meta.root).to_path_buf(),
                process_env: build_openclaw_env(env_meta, process_env),
            })
        }
    }
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
        runtime_source_kind: String,
        runtime_release_version: Option<String>,
        runtime_release_channel: Option<String>,
        forwarded_args: Vec<String>,
        program: String,
        program_args: Vec<String>,
        run_dir: PathBuf,
    },
}

impl GatewayProcessSpec {
    pub fn program_arguments(&self) -> Vec<String> {
        match (&self.binary_path, &self.command) {
            (Some(binary_path), _) => {
                let mut program_arguments = vec![binary_path.clone()];
                program_arguments.extend(self.args.iter().cloned());
                program_arguments
            }
            (None, Some(command)) => gateway_shell_program_arguments(command),
            (None, None) => Vec::new(),
        }
    }
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

    pub fn resolve_gateway_process(
        &self,
        name: &str,
        bootstrap_managed_node: bool,
    ) -> Result<GatewayProcessSpec, String> {
        let env = self.apply_effective_gateway_port(self.get(name)?)?;
        resolve_gateway_process_spec(&env, self.env, self.cwd, bootstrap_managed_node)
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
                let launch = resolve_runtime_launch(&runtime, &args, self.env, self.cwd, true)?;
                Ok(ResolvedExecution::Runtime {
                    runtime_name,
                    binary_path: launch.runtime_binary_path,
                    runtime_source_kind: runtime.source_kind.as_str().to_string(),
                    runtime_release_version: runtime.release_version.clone(),
                    runtime_release_channel: runtime.release_channel.clone(),
                    forwarded_args: args,
                    program: launch.program,
                    program_args: launch.args,
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
                runtime_source_kind: None,
                runtime_release_version: None,
                runtime_release_channel: None,
                forwarded_args: Vec::new(),
                run_dir: run_dir.display().to_string(),
            },
            Self::Runtime {
                env,
                runtime_name,
                binary_path,
                runtime_source_kind,
                runtime_release_version,
                runtime_release_channel,
                forwarded_args,
                run_dir,
                ..
            } => ExecutionSummary {
                env_name: env.name,
                binding_kind: "runtime".to_string(),
                binding_name: runtime_name,
                command: None,
                binary_path: Some(binary_path),
                runtime_source_kind: Some(runtime_source_kind),
                runtime_release_version,
                runtime_release_channel,
                forwarded_args,
                run_dir: run_dir.display().to_string(),
            },
        }
    }
}
