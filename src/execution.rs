use std::path::{Path, PathBuf};

use crate::shell::quote_posix;
use crate::types::{EnvMeta, LauncherMeta};

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

pub fn resolve_launcher_name(
    env_meta: &EnvMeta,
    launcher_override: Option<String>,
) -> Result<String, String> {
    launcher_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| env_meta.default_launcher.clone())
        .ok_or_else(|| {
            format!(
                "environment \"{}\" has no default launcher; use env set-launcher or pass --launcher",
                env_meta.name
            )
        })
}

pub fn build_launcher_command(launcher: &LauncherMeta, args: &[String]) -> String {
    if args.is_empty() {
        return launcher.command.clone();
    }

    let mut command = launcher.command.clone();
    let quoted = args.iter().map(|arg| quote_posix(arg)).collect::<Vec<_>>();
    command.push(' ');
    command.push_str(&quoted.join(" "));
    command
}

pub fn resolve_launcher_run_dir(launcher: &LauncherMeta, fallback_cwd: &Path) -> PathBuf {
    launcher
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback_cwd.to_path_buf())
}

pub fn resolve_runtime_run_dir(fallback_cwd: &Path) -> PathBuf {
    fallback_cwd.to_path_buf()
}
