mod env;
mod init;
mod launcher;
mod runtime;

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths::derive_env_paths;
use crate::services::{EnvironmentService, LauncherService, RuntimeService};
use crate::store::{ensure_store, summarize_env};
use crate::types::{
    CloneEnvironmentOptions, CreateEnvironmentOptions, EnvSummary, ExportEnvironmentOptions,
    ImportEnvironmentOptions,
};

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

    fn stdout_line(&self, line: impl AsRef<str>) {
        println!("{}", line.as_ref());
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
            "OpenClaw Manager (ocm)\n\nUsage:\n  {cmd} help\n  {cmd} --version\n  {cmd} init [zsh|bash|sh|fish]\n  {cmd} env create <name> [--root <path>] [--port <port>] [--runtime <name>] [--launcher <name>] [--protect]\n  {cmd} env clone <source> <target> [--root <path>] [--json]\n  {cmd} env export <name> [--output <path>] [--json]\n  {cmd} env import <archive> [--name <name>] [--root <path>] [--json]\n  {cmd} env snapshot create <name> [--label <label>] [--json]\n  {cmd} env snapshot show <name> <snapshot> [--json]\n  {cmd} env snapshot list <name> [--json]\n  {cmd} env snapshot list --all [--json]\n  {cmd} env snapshot restore <name> <snapshot> [--json]\n  {cmd} env snapshot remove <name> <snapshot> [--json]\n  {cmd} env snapshot prune (<name> | --all) [--keep <count>] [--older-than <days>] [--yes] [--json]\n  {cmd} env list [--json]\n  {cmd} env show <name> [--json]\n  {cmd} env status <name> [--json]\n  {cmd} env doctor <name> [--json]\n  {cmd} env cleanup (<name> | --all) [--yes] [--json]\n  {cmd} env repair-marker <name> [--json]\n  {cmd} env use <name> [--shell zsh|bash|sh|fish]\n  {cmd} env exec <name> -- <command...>\n  {cmd} env resolve <name> [--runtime <name> | --launcher <name>] [--json] [-- <openclaw args...>]\n  {cmd} env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>\n  {cmd} env set-runtime <name> <runtime|none>\n  {cmd} env set-launcher <name> <launcher|none>\n  {cmd} env protect <name> <on|off>\n  {cmd} env remove <name> [--force]\n  {cmd} env prune [--older-than <days>] [--yes] [--json]\n  {cmd} launcher add <name> --command \"<launcher>\" [--cwd <path>] [--description <text>]\n  {cmd} launcher list [--json]\n  {cmd} launcher show <name> [--json]\n  {cmd} launcher remove <name>\n  {cmd} runtime add <name> --path <binary> [--description <text>]\n  {cmd} runtime install <name> (--path <binary> | --url <url> | --manifest-url <url> (--version <version> | --channel <channel>)) [--description <text>] [--force]\n  {cmd} runtime update (<name> | --all) [--version <version> | --channel <channel>] [--json]\n  {cmd} runtime releases --manifest-url <url> [--version <version> | --channel <channel>] [--json]\n  {cmd} runtime list [--json]\n  {cmd} runtime show <name> [--json]\n  {cmd} runtime verify (<name> | --all) [--json]\n  {cmd} runtime which <name> [--json]\n  {cmd} runtime remove <name>\n\nExamples:\n  {cmd} init\n  {cmd} init zsh\n  {cmd} init bash\n  {cmd} init fish\n  {cmd} launcher add stable --command openclaw\n  {cmd} runtime add stable --path /path/to/openclaw\n  {cmd} runtime install managed-stable --path ./target/debug/openclaw\n  {cmd} runtime install nightly --url https://example.test/openclaw-nightly\n  {cmd} runtime install nightly --url https://example.test/openclaw-nightly --force\n  {cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --version 0.2.0\n  {cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --channel stable\n  {cmd} runtime update stable\n  {cmd} runtime update stable --version 0.3.0\n  {cmd} runtime update --all\n  {cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --channel stable\n  {cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --version 0.2.0 --json\n  {cmd} runtime verify nightly --json\n  {cmd} runtime verify --all\n  {cmd} runtime which nightly --json\n  {cmd} env create refactor-a --runtime stable --launcher stable --port 19789\n  {cmd} env clone refactor-a refactor-b\n  {cmd} env export refactor-a --output ./backups/refactor-a.ocm-env.tar\n  {cmd} env import ./backups/refactor-a.ocm-env.tar --name refactor-b\n  {cmd} env snapshot create refactor-a --label before-upgrade\n  {cmd} env snapshot show refactor-a 1742922000-123456789\n  {cmd} env snapshot list refactor-a\n  {cmd} env snapshot list --all --json\n  {cmd} env snapshot restore refactor-a 1742922000-123456789\n  {cmd} env snapshot remove refactor-a 1742922000-123456789\n  {cmd} env snapshot prune refactor-a --keep 5 --yes\n  {cmd} env snapshot prune --all --older-than 30 --json\n  {cmd} env status refactor-a --json\n  {cmd} env doctor refactor-a --json\n  {cmd} env cleanup refactor-a --json\n  {cmd} env cleanup refactor-a --yes\n  {cmd} env cleanup --all --yes\n  {cmd} env repair-marker refactor-a --json\n  {cmd} env resolve refactor-a --json\n  eval \"$({cmd} env use refactor-a)\"\n  {cmd} env run refactor-a -- onboard\n  {cmd} env exec refactor-a -- openclaw gateway run --port 19789\n"
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

    fn handle_env_create(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, protect) = Self::consume_flag(args, "--protect");
        let (args, root) = Self::consume_option(args, "--root")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--port")?),
            _ => None,
        };
        let (args, runtime_name) = Self::consume_option(args, "--runtime")?;
        let runtime_name = Self::require_option_value(runtime_name, "--runtime")?;
        let (args, launcher_name) = Self::consume_option(args, "--launcher")?;
        let launcher_name = Self::require_option_value(launcher_name, "--launcher")?;

        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self
            .environment_service()
            .create(CreateEnvironmentOptions {
                name: name.clone(),
                root,
                gateway_port,
                default_runtime: runtime_name,
                default_launcher: launcher_name,
                protected: protect,
            })?;

        if json_flag {
            self.print_json(&summarize_env(&meta))?;
            return Ok(0);
        }

        let summary = summarize_env(&meta);
        self.stdout_line(format!("Created env {}", summary.name));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  openclaw home: {}", summary.openclaw_home));
        self.stdout_line(format!("  workspace: {}", summary.workspace_dir));
        if let Some(port) = summary.gateway_port {
            self.stdout_line(format!("  gateway port: {port}"));
        }
        if let Some(runtime) = summary.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    fn handle_env_clone(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, root) = Self::consume_option(args, "--root")?;
        let Some(source_name) = args.first() else {
            return Err("source environment name is required".to_string());
        };
        let Some(target_name) = args.get(1) else {
            return Err("target environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let meta = self.environment_service().clone(CloneEnvironmentOptions {
            source_name: source_name.clone(),
            name: target_name.clone(),
            root,
        })?;

        if json_flag {
            self.print_json(&summarize_env(&meta))?;
            return Ok(0);
        }

        let summary = summarize_env(&meta);
        self.stdout_line(format!("Cloned env {} from {}", summary.name, source_name));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  openclaw home: {}", summary.openclaw_home));
        self.stdout_line(format!("  workspace: {}", summary.workspace_dir));
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    fn handle_env_export(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, output) = Self::consume_option(args, "--output")?;
        let output = Self::require_option_value(output, "--output")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self
            .environment_service()
            .export(ExportEnvironmentOptions {
                name: name.clone(),
                output,
            })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_line(format!("Exported env {}", summary.name));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  archive: {}", summary.archive_path));
        if let Some(runtime) = summary.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        if summary.protected {
            self.stdout_line("  protected: true");
        }
        Ok(0)
    }

    fn handle_env_import(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, name) = Self::consume_option(args, "--name")?;
        let name = Self::require_option_value(name, "--name")?;
        let (args, root) = Self::consume_option(args, "--root")?;
        let root = Self::require_option_value(root, "--root")?;
        let Some(archive) = args.first() else {
            return Err("archive path is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self
            .environment_service()
            .import(ImportEnvironmentOptions {
                archive: archive.clone(),
                name,
                root,
            })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_line(format!(
            "Imported env {} from {}",
            summary.name, summary.source_name
        ));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  archive: {}", summary.archive_path));
        if let Some(runtime) = summary.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        if summary.protected {
            self.stdout_line("  protected: true");
        }
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    fn handle_env_protect(&self, args: Vec<String>) -> Result<i32, String> {
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env protect <env> <on|off>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let value = args[1].trim().to_ascii_lowercase();
        Self::assert_no_extra_args(&args[2..])?;
        if value != "on" && value != "off" {
            return Err("protection must be \"on\" or \"off\"".to_string());
        }

        let meta = self
            .environment_service()
            .set_protected(name, value == "on")?;
        self.stdout_line(format!(
            "Updated env {}: protected={}",
            meta.name, meta.protected
        ));
        Ok(0)
    }

    fn handle_env_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, force) = Self::consume_flag(args, "--force");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.environment_service().remove(name, force)?;
        self.stdout_line(format!("Removed env {}", meta.name));
        self.stdout_line(format!(
            "  root: {}",
            derive_env_paths(Path::new(&meta.root)).root.display()
        ));
        Ok(0)
    }

    fn handle_env_prune(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, yes) = Self::consume_flag(args, "--yes");
        let (args, older_than_raw) = Self::consume_option(args, "--older-than")?;
        Self::assert_no_extra_args(&args)?;

        let older_than_days = match older_than_raw.as_deref() {
            Some(raw) => Self::parse_positive_u32(raw, "--older-than")? as i64,
            _ => 14,
        };

        let candidates = self
            .environment_service()
            .prune_candidates(older_than_days)?;

        if !yes {
            if json_flag {
                let summaries = candidates.iter().map(summarize_env).collect::<Vec<_>>();
                self.print_json(&serde_json::json!({
                    "apply": false,
                    "olderThanDays": older_than_days,
                    "count": summaries.len(),
                    "candidates": summaries,
                }))?;
                return Ok(0);
            }

            self.stdout_line(format!(
                "Prune preview ({}d): {} candidate(s)",
                older_than_days,
                candidates.len()
            ));
            for meta in candidates {
                self.stdout_line(format!(
                    "  {}  {}",
                    meta.name,
                    derive_env_paths(Path::new(&meta.root)).root.display()
                ));
            }
            self.stdout_line("Re-run with --yes to remove them.");
            return Ok(0);
        }

        let mut removed = Vec::<EnvSummary>::new();
        let removed_meta = self.environment_service().prune(older_than_days)?;
        for meta in removed_meta {
            removed.push(summarize_env(&meta));
        }

        if json_flag {
            self.print_json(&serde_json::json!({
                "apply": true,
                "olderThanDays": older_than_days,
                "count": removed.len(),
                "removed": removed,
            }))?;
            return Ok(0);
        }

        self.stdout_line(format!("Pruned {} environment(s).", removed.len()));
        for summary in removed {
            self.stdout_line(format!("  {}  {}", summary.name, summary.root));
        }
        Ok(0)
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
            "env" => match action.as_str() {
                "create" => self.handle_env_create(rest),
                "clone" => self.handle_env_clone(rest),
                "export" => self.handle_env_export(rest),
                "import" => self.handle_env_import(rest),
                "snapshot" => self.dispatch_env_snapshot_command(rest),
                "list" => self.handle_env_list(rest),
                "show" => self.handle_env_show(rest),
                "status" => self.handle_env_status(rest),
                "doctor" => self.handle_env_doctor(rest),
                "cleanup" => self.handle_env_cleanup(rest),
                "repair-marker" => self.handle_env_repair_marker(rest),
                "use" => self.handle_env_use(rest),
                "exec" => self.handle_env_exec(rest),
                "resolve" => self.handle_env_resolve(rest),
                "run" => self.handle_env_run(rest),
                "set-runtime" => self.handle_env_set_runtime(rest),
                "set-launcher" => self.handle_env_set_launcher(rest),
                "protect" => self.handle_env_protect(rest),
                "remove" | "rm" => self.handle_env_remove(rest),
                "prune" => self.handle_env_prune(rest),
                _ => Err(format!("unknown env command: {action}")),
            },
            "launcher" => self.dispatch_launcher_command(action.as_str(), rest),
            "runtime" => self.dispatch_runtime_command(action.as_str(), rest),
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
