use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::paths::{derive_env_paths, validate_name};
use crate::shell::{build_openclaw_env, quote_posix, render_use_script, resolve_shell_name};
use crate::store::{
    add_version, create_environment, ensure_store, get_environment, get_version, list_environments,
    list_versions, now_utc, remove_environment, remove_version, save_environment,
    select_prune_candidates, summarize_env,
};
use crate::types::{AddVersionOptions, CreateEnvironmentOptions, EnvMeta, EnvSummary};

const VERSION: &str = "0.1.0";

pub struct Cli {
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

impl Cli {
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
            "OpenClaw Manager (ocm)\n\nUsage:\n  {cmd} help\n  {cmd} --version\n  {cmd} env create <name> [--root <path>] [--port <port>] [--version <name>] [--protect]\n  {cmd} env list [--json]\n  {cmd} env show <name> [--json]\n  {cmd} env use <name> [--shell zsh|bash|sh|fish]\n  {cmd} env exec <name> -- <command...>\n  {cmd} env run <name> [--version <name>] -- <openclaw args...>\n  {cmd} env set-version <name> <version|none>\n  {cmd} env protect <name> <on|off>\n  {cmd} env remove <name> [--force]\n  {cmd} env prune [--older-than <days>] [--yes] [--json]\n  {cmd} version add <name> --command \"<launcher>\" [--cwd <path>] [--description <text>]\n  {cmd} version list [--json]\n  {cmd} version show <name> [--json]\n  {cmd} version remove <name>\n\nExamples:\n  {cmd} version add stable --command openclaw\n  {cmd} env create refactor-a --version stable --port 19789\n  eval \"$({cmd} env use refactor-a)\"\n  {cmd} env run refactor-a -- onboard\n  {cmd} env exec refactor-a -- openclaw gateway run --port 19789\n"
        )
    }

    fn parse_positive_u32(raw: &str, label: &str) -> Result<u32, String> {
        let value = raw
            .trim()
            .parse::<u32>()
            .map_err(|_| format!("{label} must be a positive integer"))?;
        if value == 0 {
            return Err(format!("{label} must be a positive integer"));
        }
        Ok(value)
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

    fn run_direct(
        &self,
        command: &str,
        args: &[String],
        env: &BTreeMap<String, String>,
        cwd: &Path,
    ) -> Result<i32, String> {
        let status = Command::new(command)
            .args(args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .env_clear()
            .envs(env)
            .current_dir(cwd)
            .status()
            .map_err(|error| format!("failed to run \"{command}\": {error}"))?;
        Ok(status.code().unwrap_or(1))
    }

    fn run_shell(
        &self,
        command: &str,
        env: &BTreeMap<String, String>,
        cwd: &Path,
    ) -> Result<i32, String> {
        if cfg!(windows) {
            self.run_direct("cmd", &["/C".to_string(), command.to_string()], env, cwd)
        } else {
            self.run_direct("sh", &["-lc".to_string(), command.to_string()], env, cwd)
        }
    }

    fn touch_environment(&self, name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, &self.env, &self.cwd)?;
        meta.last_used_at = Some(now_utc());
        save_environment(meta, &self.env, &self.cwd)
    }

    fn handle_env_create(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, protect) = Self::consume_flag(args, "--protect");
        let (args, root) = Self::consume_option(args, "--root")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) if !raw.trim().is_empty() => Some(Self::parse_positive_u32(raw, "--port")?),
            _ => None,
        };
        let (args, version_name) = Self::consume_option(args, "--version")?;

        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        if let Some(version_name) = version_name
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            get_version(version_name, &self.env, &self.cwd)?;
        }

        let meta = create_environment(
            CreateEnvironmentOptions {
                name: name.clone(),
                root,
                gateway_port,
                default_version: version_name.filter(|value| !value.trim().is_empty()),
                protected: protect,
            },
            &self.env,
            &self.cwd,
        )?;

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
        if let Some(version) = summary.default_version.as_deref() {
            self.stdout_line(format!("  version: {version}"));
        }
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    fn handle_env_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let envs = list_environments(&self.env, &self.cwd)?;
        let summaries = envs.iter().map(summarize_env).collect::<Vec<_>>();
        if json_flag {
            self.print_json(&summaries)?;
            return Ok(0);
        }
        if summaries.is_empty() {
            self.stdout_line("No environments.");
            return Ok(0);
        }
        for summary in summaries {
            let mut bits = vec![summary.name, summary.root];
            if let Some(version) = summary.default_version {
                bits.push(format!("version={version}"));
            }
            if let Some(port) = summary.gateway_port {
                bits.push(format!("port={port}"));
            }
            if summary.protected {
                bits.push("protected".to_string());
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    fn handle_env_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = get_environment(name, &self.env, &self.cwd)?;
        let summary = summarize_env(&meta);
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        let mut lines = BTreeMap::new();
        lines.insert("name".to_string(), summary.name);
        lines.insert("root".to_string(), summary.root);
        lines.insert("openclawHome".to_string(), summary.openclaw_home);
        lines.insert("stateDir".to_string(), summary.state_dir);
        lines.insert("configPath".to_string(), summary.config_path);
        lines.insert("workspaceDir".to_string(), summary.workspace_dir);
        lines.insert("protected".to_string(), summary.protected.to_string());
        lines.insert(
            "createdAt".to_string(),
            summary
                .created_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
        );
        if let Some(port) = summary.gateway_port {
            lines.insert("gatewayPort".to_string(), port.to_string());
        }
        if let Some(version) = summary.default_version {
            lines.insert("defaultVersion".to_string(), version);
        }
        if let Some(last_used_at) = summary.last_used_at {
            lines.insert(
                "lastUsedAt".to_string(),
                last_used_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .map_err(|error| error.to_string())?,
            );
        }

        for (key, value) in lines {
            self.stdout_line(format!("{key}: {value}"));
        }
        Ok(0)
    }

    fn handle_env_use(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, shell_name) = Self::consume_option(args, "--shell")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.touch_environment(name)?;
        let shell = resolve_shell_name(shell_name.as_deref(), &self.env);
        print!("{}", render_use_script(&meta, &shell));
        Ok(0)
    }

    fn handle_env_exec(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_command_separator(&before, "env exec requires -- before the command")?;
        if after.is_empty() {
            return Err("env exec requires a command after --".to_string());
        }

        let meta = self.touch_environment(name)?;
        self.run_direct(
            &after[0],
            &after[1..],
            &build_openclaw_env(&meta, &self.env),
            &self.cwd,
        )
    }

    fn handle_env_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let (before, version_override) = Self::consume_option(before, "--version")?;
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_command_separator(&before, "env run requires -- before OpenClaw arguments")?;

        let meta = self.touch_environment(name)?;
        let version_name = version_override
            .filter(|value| !value.trim().is_empty())
            .or_else(|| meta.default_version.clone())
            .ok_or_else(|| {
                format!(
                    "environment \"{}\" has no default version; use env set-version or pass --version",
                    meta.name
                )
            })?;

        let version = get_version(&version_name, &self.env, &self.cwd)?;
        let mut command = version.command.clone();
        if !after.is_empty() {
            let quoted = after.iter().map(|arg| quote_posix(arg)).collect::<Vec<_>>();
            command.push(' ');
            command.push_str(&quoted.join(" "));
        }

        let run_dir = version
            .cwd
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.cwd.clone());
        self.run_shell(&command, &build_openclaw_env(&meta, &self.env), &run_dir)
    }

    fn handle_env_set_version(&self, args: Vec<String>) -> Result<i32, String> {
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env set-version <env> <version|none>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let version_name = &args[1];
        Self::assert_no_extra_args(&args[2..])?;

        let mut meta = get_environment(name, &self.env, &self.cwd)?;
        if version_name.eq_ignore_ascii_case("none") {
            meta.default_version = None;
        } else {
            let validated = validate_name(version_name, "Version name")?;
            get_version(&validated, &self.env, &self.cwd)?;
            meta.default_version = Some(validated);
        }

        let meta = save_environment(meta, &self.env, &self.cwd)?;
        let default_version = meta.default_version.unwrap_or_else(|| "none".to_string());
        self.stdout_line(format!(
            "Updated env {}: defaultVersion={default_version}",
            meta.name
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

        let mut meta = get_environment(name, &self.env, &self.cwd)?;
        meta.protected = value == "on";
        let meta = save_environment(meta, &self.env, &self.cwd)?;
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

        let meta = remove_environment(name, force, &self.env, &self.cwd)?;
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
            Some(raw) if !raw.trim().is_empty() => {
                Self::parse_positive_u32(raw, "--older-than")? as i64
            }
            _ => 14,
        };

        let envs = list_environments(&self.env, &self.cwd)?;
        let candidates = select_prune_candidates(&envs, older_than_days);

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
        for meta in candidates {
            let removed_meta = remove_environment(&meta.name, false, &self.env, &self.cwd)?;
            removed.push(summarize_env(&removed_meta));
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

    fn handle_version_add(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, command) = Self::consume_option(args, "--command")?;
        let (args, cwd) = Self::consume_option(args, "--cwd")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("version name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = add_version(
            AddVersionOptions {
                name: name.clone(),
                command: command.unwrap_or_default(),
                cwd,
                description,
            },
            &self.env,
            &self.cwd,
        )?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Added version {}", meta.name));
        self.stdout_line(format!("  command: {}", meta.command));
        if let Some(cwd) = meta.cwd.as_deref() {
            self.stdout_line(format!("  cwd: {cwd}"));
        }
        Ok(0)
    }

    fn handle_version_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let versions = list_versions(&self.env, &self.cwd)?;
        if json_flag {
            self.print_json(&versions)?;
            return Ok(0);
        }
        if versions.is_empty() {
            self.stdout_line("No versions.");
            return Ok(0);
        }
        for meta in versions {
            let mut bits = vec![meta.name, meta.command];
            if let Some(cwd) = meta.cwd {
                bits.push(format!("cwd={cwd}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    fn handle_version_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("version name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = get_version(name, &self.env, &self.cwd)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        let mut lines = BTreeMap::new();
        lines.insert("kind".to_string(), meta.kind.clone());
        lines.insert("name".to_string(), meta.name.clone());
        lines.insert("command".to_string(), meta.command.clone());
        lines.insert(
            "createdAt".to_string(),
            meta.created_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
        );
        lines.insert(
            "updatedAt".to_string(),
            meta.updated_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
        );
        if let Some(cwd) = meta.cwd {
            lines.insert("cwd".to_string(), cwd);
        }
        if let Some(description) = meta.description {
            lines.insert("description".to_string(), description);
        }
        for (key, value) in lines {
            self.stdout_line(format!("{key}: {value}"));
        }
        Ok(0)
    }

    fn handle_version_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let Some(name) = args.first() else {
            return Err("version name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = remove_version(name, &self.env, &self.cwd)?;
        self.stdout_line(format!("Removed version {}", meta.name));
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
            "env" => match action.as_str() {
                "create" => self.handle_env_create(rest),
                "list" => self.handle_env_list(rest),
                "show" => self.handle_env_show(rest),
                "use" => self.handle_env_use(rest),
                "exec" => self.handle_env_exec(rest),
                "run" => self.handle_env_run(rest),
                "set-version" => self.handle_env_set_version(rest),
                "protect" => self.handle_env_protect(rest),
                "remove" | "rm" => self.handle_env_remove(rest),
                "prune" => self.handle_env_prune(rest),
                _ => Err(format!("unknown env command: {action}")),
            },
            "version" => match action.as_str() {
                "add" => self.handle_version_add(rest),
                "list" => self.handle_version_list(rest),
                "show" => self.handle_version_show(rest),
                "remove" | "rm" => self.handle_version_remove(rest),
                _ => Err(format!("unknown version command: {action}")),
            },
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
