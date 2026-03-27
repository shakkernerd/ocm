mod env;
mod init;
mod launcher;
mod render;
mod runtime;
mod service;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

use serde::Serialize;

use crate::env::EnvironmentService;
use crate::launcher::LauncherService;
use crate::runtime::RuntimeService;
use crate::service::ServiceService;
use crate::store::ensure_store;

const VERSION: &str = "0.1.0";

pub struct Cli {
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

impl Cli {
    fn launcher_service(&self) -> LauncherService<'_> {
        LauncherService::new(&self.env, &self.cwd)
    }

    fn environment_service(&self) -> EnvironmentService<'_> {
        EnvironmentService::new(&self.env, &self.cwd)
    }

    fn runtime_service(&self) -> RuntimeService<'_> {
        RuntimeService::new(&self.env, &self.cwd)
    }

    fn service_service(&self) -> ServiceService<'_> {
        ServiceService::new(&self.env, &self.cwd)
    }

    fn stdout_line(&self, line: impl AsRef<str>) {
        println!("{}", line.as_ref());
    }

    fn stdout_lines<I, S>(&self, lines: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for line in lines {
            self.stdout_line(line);
        }
    }

    fn stderr_line(&self, line: impl AsRef<str>) {
        eprintln!("{}", line.as_ref());
    }

    fn print_json<T: Serialize>(&self, value: &T) -> Result<(), String> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        serde_json::to_writer_pretty(&mut handle, value).map_err(|error| error.to_string())?;
        writeln!(handle).map_err(|error| error.to_string())
    }

    fn stdout_text(&self, text: &str) -> Result<(), String> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle
            .write_all(text.as_bytes())
            .map_err(|error| error.to_string())
    }

    fn command_example(&self) -> String {
        self.env
            .get("OCM_SELF")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "ocm".to_string())
    }

    fn render_help(&self) -> String {
        let cmd = self.command_example();
        format!(
            "OpenClaw Manager (ocm)\n\nUsage:\n  {cmd} help\n  {cmd} --version\n  {cmd} init [zsh|bash|sh|fish]\n  {cmd} env create <name> [--root <path>] [--port <port>] [--runtime <name>] [--launcher <name>] [--protect]\n  {cmd} env clone <source> <target> [--root <path>] [--json]\n  {cmd} env export <name> [--output <path>] [--json]\n  {cmd} env import <archive> [--name <name>] [--root <path>] [--json]\n  {cmd} env snapshot create <name> [--label <label>] [--json]\n  {cmd} env snapshot show <name> <snapshot> [--json]\n  {cmd} env snapshot list <name> [--json]\n  {cmd} env snapshot list --all [--json]\n  {cmd} env snapshot restore <name> <snapshot> [--json]\n  {cmd} env snapshot remove <name> <snapshot> [--json]\n  {cmd} env snapshot prune (<name> | --all) [--keep <count>] [--older-than <days>] [--yes] [--json]\n  {cmd} env list [--json]\n  {cmd} env show <name> [--json]\n  {cmd} env status <name> [--json]\n  {cmd} env doctor <name> [--json]\n  {cmd} env cleanup (<name> | --all) [--yes] [--json]\n  {cmd} env repair-marker <name> [--json]\n  {cmd} env use <name> [--shell zsh|bash|sh|fish]\n  {cmd} env exec <name> -- <command...>\n  {cmd} env resolve <name> [--runtime <name> | --launcher <name>] [--json] [-- <openclaw args...>]\n  {cmd} env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>\n  {cmd} env set-runtime <name> <runtime|none>\n  {cmd} env set-launcher <name> <launcher|none>\n  {cmd} env protect <name> <on|off>\n  {cmd} env remove <name> [--force]\n  {cmd} env prune [--older-than <days>] [--yes] [--json]\n  {cmd} launcher add <name> --command \"<launcher>\" [--cwd <path>] [--description <text>]\n  {cmd} launcher list [--json]\n  {cmd} launcher show <name> [--json]\n  {cmd} launcher remove <name>\n  {cmd} runtime add <name> --path <binary> [--description <text>]\n  {cmd} runtime install <name> (--path <binary> | --url <url> | --manifest-url <url> (--version <version> | --channel <channel>)) [--description <text>] [--force]\n  {cmd} runtime update (<name> | --all) [--version <version> | --channel <channel>] [--json]\n  {cmd} runtime releases --manifest-url <url> [--version <version> | --channel <channel>] [--json]\n  {cmd} runtime list [--json]\n  {cmd} runtime show <name> [--json]\n  {cmd} runtime verify (<name> | --all) [--json]\n  {cmd} runtime which <name> [--json]\n  {cmd} runtime remove <name>\n  {cmd} service adopt-global <env> [--dry-run] [--json]\n  {cmd} service install <env> [--json]\n  {cmd} service list [--json]\n  {cmd} service status <env> [--json]\n  {cmd} service status --all [--json]\n  {cmd} service logs <env> [--stderr] [--tail <count>] [--json]\n  {cmd} service start <env> [--json]\n  {cmd} service stop <env> [--json]\n  {cmd} service restart <env> [--json]\n  {cmd} service uninstall <env> [--json]\n\nExamples:\n  {cmd} init\n  {cmd} init zsh\n  {cmd} init bash\n  {cmd} init fish\n  {cmd} launcher add stable --command openclaw\n  {cmd} runtime add stable --path /path/to/openclaw\n  {cmd} runtime install managed-stable --path ./target/debug/openclaw\n  {cmd} runtime install nightly --url https://example.test/openclaw-nightly\n  {cmd} runtime install nightly --url https://example.test/openclaw-nightly --force\n  {cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --version 0.2.0\n  {cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --channel stable\n  {cmd} runtime update stable\n  {cmd} runtime update stable --version 0.3.0\n  {cmd} runtime update --all\n  {cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --channel stable\n  {cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --version 0.2.0 --json\n  {cmd} runtime verify nightly --json\n  {cmd} runtime verify --all\n  {cmd} runtime which nightly --json\n  {cmd} service adopt-global refactor-a --dry-run --json\n  {cmd} service install refactor-a --json\n  {cmd} service list\n  {cmd} service status refactor-a --json\n  {cmd} service status --all\n  {cmd} service logs refactor-a\n  {cmd} service logs refactor-a --stderr --tail 50\n  {cmd} service start refactor-a\n  {cmd} service restart refactor-a --json\n  {cmd} service stop refactor-a\n  {cmd} service uninstall refactor-a\n  {cmd} env create refactor-a --runtime stable --launcher stable --port 19789\n  {cmd} env clone refactor-a refactor-b\n  {cmd} env export refactor-a --output ./backups/refactor-a.ocm-env.tar\n  {cmd} env import ./backups/refactor-a.ocm-env.tar --name refactor-b\n  {cmd} env snapshot create refactor-a --label before-upgrade\n  {cmd} env snapshot show refactor-a 1742922000-123456789\n  {cmd} env snapshot list refactor-a\n  {cmd} env snapshot list --all --json\n  {cmd} env snapshot restore refactor-a 1742922000-123456789\n  {cmd} env snapshot remove refactor-a 1742922000-123456789\n  {cmd} env snapshot prune refactor-a --keep 5 --yes\n  {cmd} env snapshot prune --all --older-than 30 --json\n  {cmd} env status refactor-a --json\n  {cmd} env doctor refactor-a --json\n  {cmd} env cleanup refactor-a --json\n  {cmd} env cleanup refactor-a --yes\n  {cmd} env cleanup --all --yes\n  {cmd} env repair-marker refactor-a --json\n  {cmd} env resolve refactor-a --json\n  eval \"$({cmd} env use refactor-a)\"\n  {cmd} env run refactor-a -- onboard\n  {cmd} env exec refactor-a -- openclaw gateway run --port 19789\n"
        )
    }

    fn parse_positive_u32(raw: &str, label: &str) -> Result<u32, String> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(format!("{label} requires a value"));
        }

        let value = raw
            .parse::<u32>()
            .map_err(|_| format!("{label} must be a positive integer"))?;
        if value == 0 {
            return Err(format!("{label} must be a positive integer"));
        }
        Ok(value)
    }

    fn require_option_value(value: Option<String>, name: &str) -> Result<Option<String>, String> {
        match value {
            Some(value) if value.trim().is_empty() => Err(format!("{name} requires a value")),
            Some(value) => Ok(Some(value.trim().to_string())),
            None => Ok(None),
        }
    }

    fn split_on_double_dash(args: &[String]) -> (Vec<String>, Vec<String>) {
        for (index, arg) in args.iter().enumerate() {
            if arg == "--" {
                return (args[..index].to_vec(), args[index + 1..].to_vec());
            }
        }
        (args.to_vec(), Vec::new())
    }

    fn consume_flag(args: Vec<String>, name: &str) -> (Vec<String>, bool) {
        let mut out = Vec::with_capacity(args.len());
        let mut found = false;
        for arg in args {
            if !found && arg == name {
                found = true;
                continue;
            }
            out.push(arg);
        }
        (out, found)
    }

    fn consume_option(
        args: Vec<String>,
        name: &str,
    ) -> Result<(Vec<String>, Option<String>), String> {
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
                let mut out = Vec::with_capacity(args.len().saturating_sub(1));
                out.extend(args[..index].iter().cloned());
                out.extend(args[index + 1..].iter().cloned());
                return Ok((out, Some(value.to_string())));
            }
            if arg == name {
                if index + 1 >= args.len() {
                    return Err(format!("{name} requires a value"));
                }
                let mut out = Vec::with_capacity(args.len().saturating_sub(2));
                out.extend(args[..index].iter().cloned());
                out.extend(args[index + 2..].iter().cloned());
                return Ok((out, Some(args[index + 1].clone())));
            }
            index += 1;
        }
        Ok((args, None))
    }

    fn assert_no_extra_args(args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            Ok(())
        } else {
            Err(format!("unexpected arguments: {}", args.join(" ")))
        }
    }

    fn assert_command_separator(before: &[String], message: &str) -> Result<(), String> {
        if before.len() > 1 {
            return Err(message.to_string());
        }
        Self::assert_no_extra_args(&before[1..]).map_err(|_| message.to_string())?;
        Ok(())
    }

    pub fn run(&self, args: Vec<String>) -> i32 {
        if args.is_empty() || matches!(args[0].as_str(), "help" | "--help" | "-h") {
            print!("{}", self.render_help());
            return 0;
        }
        if matches!(args[0].as_str(), "--version" | "-v") {
            self.stdout_line(VERSION);
            return 0;
        }

        if let Err(error) = ensure_store(&self.env, &self.cwd) {
            self.stderr_line(format!("ocm: {error}"));
            self.stderr_line(format!(
                "Run \"{} help\" for usage.",
                self.command_example()
            ));
            return 1;
        }

        let group = args.first().cloned().unwrap_or_default();
        let action = args.get(1).cloned().unwrap_or_default();
        let rest = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            Vec::new()
        };

        let result = match group.as_str() {
            "init" => self.handle_init_command(&action, rest),
            "env" => self.dispatch_env_command(action.as_str(), rest),
            "launcher" => self.dispatch_launcher_command(action.as_str(), rest),
            "runtime" => self.dispatch_runtime_command(action.as_str(), rest),
            "service" => self.dispatch_service_command(action.as_str(), rest),
            _ => Err(format!("unknown command group: {group}")),
        };

        match result {
            Ok(code) => code,
            Err(error) => {
                self.stderr_line(format!("ocm: {error}"));
                self.stderr_line(format!(
                    "Run \"{} help\" for usage.",
                    self.command_example()
                ));
                1
            }
        }
    }
}
