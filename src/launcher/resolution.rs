use super::LauncherMeta;
use std::path::{Path, PathBuf};

use crate::infra::shell::quote_posix;
use crate::types::EnvMeta;

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
