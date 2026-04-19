use super::LauncherMeta;
use std::path::{Path, PathBuf};

use crate::env::EnvMeta;
use crate::infra::shell::quote_posix;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DirectLauncherCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
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

pub(crate) fn resolve_direct_launcher_command(
    launcher: &LauncherMeta,
    openclaw_args: &[String],
    _fallback_cwd: &Path,
) -> Option<DirectLauncherCommand> {
    let tokens = tokenize_simple_command(&launcher.command)?;

    Some(DirectLauncherCommand {
        program: tokens.first()?.clone(),
        args: tokens
            .into_iter()
            .skip(1)
            .chain(openclaw_args.iter().cloned())
            .collect(),
    })
}

fn tokenize_simple_command(command: &str) -> Option<Vec<String>> {
    let trimmed = command.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_control) {
        return None;
    }
    if trimmed.contains([
        '\'', '"', '`', '$', '|', '&', ';', '<', '>', '(', ')', '{', '}',
    ]) {
        return None;
    }

    let tokens = trimmed
        .split_whitespace()
        .map(str::to_string)
        .collect::<Vec<_>>();
    let first = tokens.first()?;
    if first.contains('=') {
        return None;
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::{resolve_direct_launcher_command, tokenize_simple_command};
    use crate::launcher::LauncherMeta;
    use std::path::Path;
    use time::OffsetDateTime;

    fn sample_launcher(command: &str, cwd: Option<&str>) -> LauncherMeta {
        LauncherMeta {
            kind: "ocm-launcher".to_string(),
            name: "dev".to_string(),
            command: command.to_string(),
            cwd: cwd.map(str::to_string),
            description: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn tokenize_simple_command_rejects_shell_syntax() {
        assert!(tokenize_simple_command("FOO=bar openclaw").is_none());
        assert!(tokenize_simple_command("pnpm openclaw | tee log").is_none());
        assert!(tokenize_simple_command("openclaw 'gateway run'").is_none());
    }

    #[test]
    fn resolve_direct_launcher_command_uses_tokens_for_simple_commands() {
        let launcher = sample_launcher("openclaw --profile dev", None);
        let command = resolve_direct_launcher_command(
            &launcher,
            &["gateway".to_string(), "run".to_string()],
            Path::new("/tmp/fallback"),
        )
        .unwrap();

        assert_eq!(command.program, "openclaw");
        assert_eq!(command.args, vec!["--profile", "dev", "gateway", "run"]);
    }

    #[test]
    fn resolve_direct_launcher_command_keeps_package_manager_launchers_as_is() {
        let launcher = sample_launcher("pnpm openclaw --profile dev", None);
        let command = resolve_direct_launcher_command(
            &launcher,
            &[
                "gateway".to_string(),
                "run".to_string(),
                "--port".to_string(),
                "18900".to_string(),
            ],
            Path::new("/tmp/fallback"),
        )
        .unwrap();

        assert_eq!(command.program, "pnpm");
        assert_eq!(
            command.args,
            vec![
                "openclaw",
                "--profile",
                "dev",
                "gateway",
                "run",
                "--port",
                "18900",
            ]
        );
    }
}
