use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::paths::{derive_env_paths, validate_name};
use crate::runner::{run_direct, run_shell};
use crate::services::{EnvironmentService, LauncherService, RuntimeService};
use crate::shell::{build_openclaw_env, render_init_script, render_use_script, resolve_shell_name};
use crate::store::{ensure_store, summarize_env};
use crate::types::{
    AddLauncherOptions, AddRuntimeOptions, CloneEnvironmentOptions, CreateEnvironmentOptions,
    EnvSummary, ExportEnvironmentOptions, ImportEnvironmentOptions,
    InstallRuntimeFromReleaseOptions, InstallRuntimeFromUrlOptions, InstallRuntimeOptions,
    UpdateRuntimeFromReleaseOptions,
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
            "OpenClaw Manager (ocm)\n\nUsage:\n  {cmd} help\n  {cmd} --version\n  {cmd} init [zsh|bash|sh|fish]\n  {cmd} env create <name> [--root <path>] [--port <port>] [--runtime <name>] [--launcher <name>] [--protect]\n  {cmd} env clone <source> <target> [--root <path>] [--json]\n  {cmd} env export <name> [--output <path>] [--json]\n  {cmd} env import <archive> [--name <name>] [--root <path>] [--json]\n  {cmd} env list [--json]\n  {cmd} env show <name> [--json]\n  {cmd} env status <name> [--json]\n  {cmd} env use <name> [--shell zsh|bash|sh|fish]\n  {cmd} env exec <name> -- <command...>\n  {cmd} env resolve <name> [--runtime <name> | --launcher <name>] [--json] [-- <openclaw args...>]\n  {cmd} env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>\n  {cmd} env set-runtime <name> <runtime|none>\n  {cmd} env set-launcher <name> <launcher|none>\n  {cmd} env protect <name> <on|off>\n  {cmd} env remove <name> [--force]\n  {cmd} env prune [--older-than <days>] [--yes] [--json]\n  {cmd} launcher add <name> --command \"<launcher>\" [--cwd <path>] [--description <text>]\n  {cmd} launcher list [--json]\n  {cmd} launcher show <name> [--json]\n  {cmd} launcher remove <name>\n  {cmd} runtime add <name> --path <binary> [--description <text>]\n  {cmd} runtime install <name> (--path <binary> | --url <url> | --manifest-url <url> (--version <version> | --channel <channel>)) [--description <text>] [--force]\n  {cmd} runtime update (<name> | --all) [--version <version> | --channel <channel>] [--json]\n  {cmd} runtime releases --manifest-url <url> [--json]\n  {cmd} runtime list [--json]\n  {cmd} runtime show <name> [--json]\n  {cmd} runtime verify (<name> | --all) [--json]\n  {cmd} runtime which <name> [--json]\n  {cmd} runtime remove <name>\n\nExamples:\n  {cmd} init\n  {cmd} init zsh\n  {cmd} init bash\n  {cmd} init fish\n  {cmd} launcher add stable --command openclaw\n  {cmd} runtime add stable --path /path/to/openclaw\n  {cmd} runtime install managed-stable --path ./target/debug/openclaw\n  {cmd} runtime install nightly --url https://example.test/openclaw-nightly\n  {cmd} runtime install nightly --url https://example.test/openclaw-nightly --force\n  {cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --version 0.2.0\n  {cmd} runtime install stable --manifest-url https://example.test/openclaw-releases.json --channel stable\n  {cmd} runtime update stable\n  {cmd} runtime update stable --version 0.3.0\n  {cmd} runtime update --all\n  {cmd} runtime releases --manifest-url https://example.test/openclaw-releases.json --json\n  {cmd} runtime verify nightly --json\n  {cmd} runtime verify --all\n  {cmd} runtime which nightly --json\n  {cmd} env create refactor-a --runtime stable --launcher stable --port 19789\n  {cmd} env clone refactor-a refactor-b\n  {cmd} env export refactor-a --output ./backups/refactor-a.ocm-env.tar\n  {cmd} env import ./backups/refactor-a.ocm-env.tar --name refactor-b\n  {cmd} env status refactor-a --json\n  {cmd} env resolve refactor-a --json\n  eval \"$({cmd} env use refactor-a)\"\n  {cmd} env run refactor-a -- onboard\n  {cmd} env exec refactor-a -- openclaw gateway run --port 19789\n"
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

    fn handle_env_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let envs = self.environment_service().list()?;
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
            if let Some(runtime) = summary.default_runtime {
                bits.push(format!("runtime={runtime}"));
            }
            if let Some(launcher) = summary.default_launcher {
                bits.push(format!("launcher={launcher}"));
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

        let meta = self.environment_service().get(name)?;
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
        if let Some(runtime) = summary.default_runtime {
            lines.insert("defaultRuntime".to_string(), runtime);
        }
        if let Some(launcher) = summary.default_launcher {
            lines.insert("defaultLauncher".to_string(), launcher);
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

        let meta = self.environment_service().touch(name)?;
        let shell = resolve_shell_name(shell_name.as_deref(), &self.env);
        print!("{}", render_use_script(&meta, &shell));
        Ok(0)
    }

    fn handle_env_status(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let status = self.environment_service().status(name)?;
        if json_flag {
            self.print_json(&status)?;
            return Ok(0);
        }
        self.stdout_line(format!("envName: {}", status.env_name));
        self.stdout_line(format!("root: {}", status.root));
        if let Some(runtime) = status.default_runtime {
            self.stdout_line(format!("defaultRuntime: {runtime}"));
        }
        if let Some(launcher) = status.default_launcher {
            self.stdout_line(format!("defaultLauncher: {launcher}"));
        }
        if let Some(kind) = status.resolved_kind {
            self.stdout_line(format!("resolvedKind: {kind}"));
        }
        if let Some(name) = status.resolved_name {
            self.stdout_line(format!("resolvedName: {name}"));
        }
        if let Some(binary_path) = status.binary_path {
            self.stdout_line(format!("binaryPath: {binary_path}"));
        }
        if let Some(command) = status.command {
            self.stdout_line(format!("command: {command}"));
        }
        if let Some(run_dir) = status.run_dir {
            self.stdout_line(format!("runDir: {run_dir}"));
        }
        if let Some(source_kind) = status.runtime_source_kind {
            self.stdout_line(format!("runtimeSourceKind: {source_kind}"));
        }
        if let Some(release_version) = status.runtime_release_version {
            self.stdout_line(format!("runtimeReleaseVersion: {release_version}"));
        }
        if let Some(release_channel) = status.runtime_release_channel {
            self.stdout_line(format!("runtimeReleaseChannel: {release_channel}"));
        }
        if let Some(runtime_health) = status.runtime_health {
            self.stdout_line(format!("runtimeHealth: {runtime_health}"));
        }
        if let Some(issue) = status.issue {
            self.stdout_line(format!("issue: {issue}"));
        }
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

        let meta = self.environment_service().touch(name)?;
        run_direct(
            &after[0],
            &after[1..],
            &build_openclaw_env(&meta, &self.env),
            &self.cwd,
        )
    }

    fn handle_env_resolve(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let (before, json_flag) = Self::consume_flag(before, "--json");
        let (before, runtime_override) = Self::consume_option(before, "--runtime")?;
        let runtime_override = Self::require_option_value(runtime_override, "--runtime")?;
        let (before, launcher_override) = Self::consume_option(before, "--launcher")?;
        let launcher_override = Self::require_option_value(launcher_override, "--launcher")?;
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&before[1..])?;

        let summary = self
            .environment_service()
            .resolve(name, runtime_override, launcher_override, &after)?
            .into_summary();

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_line(format!("envName: {}", summary.env_name));
        self.stdout_line(format!("bindingKind: {}", summary.binding_kind));
        self.stdout_line(format!("bindingName: {}", summary.binding_name));
        if let Some(command) = summary.command {
            self.stdout_line(format!("command: {command}"));
        }
        if let Some(binary_path) = summary.binary_path {
            self.stdout_line(format!("binaryPath: {binary_path}"));
        }
        if !summary.forwarded_args.is_empty() {
            self.stdout_line(format!(
                "forwardedArgs: {}",
                summary.forwarded_args.join(" ")
            ));
        }
        self.stdout_line(format!("runDir: {}", summary.run_dir));
        Ok(0)
    }

    fn handle_env_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let (before, runtime_override) = Self::consume_option(before, "--runtime")?;
        let runtime_override = Self::require_option_value(runtime_override, "--runtime")?;
        let (before, launcher_override) = Self::consume_option(before, "--launcher")?;
        let launcher_override = Self::require_option_value(launcher_override, "--launcher")?;
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_command_separator(&before, "env run requires -- before OpenClaw arguments")?;

        let resolved = self.environment_service().resolve_run(
            name,
            runtime_override,
            launcher_override,
            &after,
        )?;
        match resolved {
            crate::services::ResolvedExecution::Launcher {
                env,
                command,
                run_dir,
                ..
            } => run_shell(&command, &build_openclaw_env(&env, &self.env), &run_dir),
            crate::services::ResolvedExecution::Runtime {
                env,
                binary_path,
                args,
                run_dir,
                ..
            } => run_direct(
                &binary_path,
                &args,
                &build_openclaw_env(&env, &self.env),
                &run_dir,
            ),
        }
    }

    fn handle_env_set_runtime(&self, args: Vec<String>) -> Result<i32, String> {
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env set-runtime <env> <runtime|none>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let runtime_name = &args[1];
        Self::assert_no_extra_args(&args[2..])?;

        let validated = if runtime_name.eq_ignore_ascii_case("none") {
            runtime_name.to_string()
        } else {
            validate_name(runtime_name, "Runtime name")?
        };
        let meta = self.environment_service().set_runtime(name, &validated)?;
        let default_runtime = meta.default_runtime.unwrap_or_else(|| "none".to_string());
        self.stdout_line(format!(
            "Updated env {}: defaultRuntime={default_runtime}",
            meta.name
        ));
        Ok(0)
    }

    fn handle_env_set_launcher(&self, args: Vec<String>) -> Result<i32, String> {
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env set-launcher <env> <launcher|none>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let launcher_name = &args[1];
        Self::assert_no_extra_args(&args[2..])?;

        let validated = if launcher_name.eq_ignore_ascii_case("none") {
            launcher_name.to_string()
        } else {
            validate_name(launcher_name, "Launcher name")?
        };
        let meta = self.environment_service().set_launcher(name, &validated)?;
        let default_launcher = meta.default_launcher.unwrap_or_else(|| "none".to_string());
        self.stdout_line(format!(
            "Updated env {}: defaultLauncher={default_launcher}",
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

    fn handle_launcher_add(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, command) = Self::consume_option(args, "--command")?;
        let command = Self::require_option_value(command, "--command")?;
        let (args, cwd) = Self::consume_option(args, "--cwd")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().add(AddLauncherOptions {
            name: name.clone(),
            command: command.unwrap_or_default(),
            cwd,
            description,
        })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Added launcher {}", meta.name));
        self.stdout_line(format!("  command: {}", meta.command));
        if let Some(cwd) = meta.cwd.as_deref() {
            self.stdout_line(format!("  cwd: {cwd}"));
        }
        Ok(0)
    }

    fn handle_launcher_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let launchers = self.launcher_service().list()?;
        if json_flag {
            self.print_json(&launchers)?;
            return Ok(0);
        }
        if launchers.is_empty() {
            self.stdout_line("No launchers.");
            return Ok(0);
        }
        for meta in launchers {
            let mut bits = vec![meta.name, meta.command];
            if let Some(cwd) = meta.cwd {
                bits.push(format!("cwd={cwd}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    fn handle_launcher_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().show(name)?;
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

    fn handle_launcher_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().remove(name)?;
        self.stdout_line(format!("Removed launcher {}", meta.name));
        Ok(0)
    }

    fn dispatch_launcher_command(&self, action: &str, rest: Vec<String>) -> Result<i32, String> {
        match action {
            "add" => self.handle_launcher_add(rest),
            "list" => self.handle_launcher_list(rest),
            "show" => self.handle_launcher_show(rest),
            "remove" | "rm" => self.handle_launcher_remove(rest),
            _ => Err(format!("unknown launcher command: {action}")),
        }
    }

    fn handle_runtime_add(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, path) = Self::consume_option(args, "--path")?;
        let path = Self::require_option_value(path, "--path")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.runtime_service().add(AddRuntimeOptions {
            name: name.clone(),
            path: path.unwrap_or_default(),
            description,
        })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Added runtime {}", meta.name));
        self.stdout_line(format!("  binary path: {}", meta.binary_path));
        Ok(0)
    }

    fn handle_runtime_install(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, force) = Self::consume_flag(args, "--force");
        let (args, path) = Self::consume_option(args, "--path")?;
        let path = Self::require_option_value(path, "--path")?;
        let (args, url) = Self::consume_option(args, "--url")?;
        let url = Self::require_option_value(url, "--url")?;
        let (args, manifest_url) = Self::consume_option(args, "--manifest-url")?;
        let manifest_url = Self::require_option_value(manifest_url, "--manifest-url")?;
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let source_count = usize::from(path.is_some())
            + usize::from(url.is_some())
            + usize::from(manifest_url.is_some());
        if source_count > 1 {
            return Err(
                "runtime install accepts only one of --path, --url, or --manifest-url".to_string(),
            );
        }
        if manifest_url.is_none() {
            if version.is_some() {
                return Err(
                    "runtime install only supports --version with --manifest-url".to_string(),
                );
            }
            if channel.is_some() {
                return Err(
                    "runtime install only supports --channel with --manifest-url".to_string(),
                );
            }
        }

        let meta = match (path, url, manifest_url) {
            (Some(path), None, None) => self.runtime_service().install(InstallRuntimeOptions {
                name: name.clone(),
                path,
                description,
                force,
            })?,
            (None, Some(url), None) => {
                self.runtime_service()
                    .install_from_url(InstallRuntimeFromUrlOptions {
                        name: name.clone(),
                        url,
                        description,
                        force,
                    })?
            }
            (None, None, Some(manifest_url)) => {
                if version.is_some() && channel.is_some() {
                    return Err(
                        "runtime install with --manifest-url accepts only one of --version or --channel"
                            .to_string(),
                    );
                }
                if version.is_none() && channel.is_none() {
                    return Err(
                        "runtime install with --manifest-url requires --version or --channel"
                            .to_string(),
                    );
                }
                self.runtime_service()
                    .install_from_release(InstallRuntimeFromReleaseOptions {
                        name: name.clone(),
                        manifest_url,
                        version,
                        channel,
                        description,
                        force,
                    })?
            }
            (None, None, None) => {
                return Err("runtime install requires --path, --url, or --manifest-url".to_string());
            }
            _ => unreachable!("source_count guards conflicting runtime install sources"),
        };

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Installed runtime {}", meta.name));
        self.stdout_line(format!("  binary path: {}", meta.binary_path));
        if let Some(install_root) = meta.install_root.as_deref() {
            self.stdout_line(format!("  install root: {install_root}"));
        }
        Ok(0)
    }

    fn handle_runtime_releases(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, manifest_url) = Self::consume_option(args, "--manifest-url")?;
        let manifest_url = Self::require_option_value(manifest_url, "--manifest-url")?;
        Self::assert_no_extra_args(&args)?;

        let releases = self
            .runtime_service()
            .releases_from_manifest(manifest_url.as_deref().unwrap_or_default())?;
        if json_flag {
            self.print_json(&releases)?;
            return Ok(0);
        }
        if releases.is_empty() {
            self.stdout_line("No runtime releases.");
            return Ok(0);
        }
        for release in releases {
            let mut bits = vec![release.version, release.url];
            if let Some(channel) = release.channel {
                bits.push(format!("channel={channel}"));
            }
            if let Some(sha256) = release.sha256 {
                bits.push(format!("sha256={sha256}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    fn handle_runtime_update(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, all_flag) = Self::consume_flag(args, "--all");
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        if all_flag {
            Self::assert_no_extra_args(&args)?;
            let summaries = self
                .runtime_service()
                .update_all_from_release(version, channel)?;
            let code = if summaries.iter().any(|summary| summary.outcome == "failed") {
                1
            } else {
                0
            };

            if json_flag {
                self.print_json(&summaries)?;
                return Ok(code);
            }

            if summaries.is_empty() {
                self.stdout_line("No runtimes.");
                return Ok(code);
            }

            for summary in summaries {
                let mut bits = vec![
                    summary.name,
                    format!("outcome={}", summary.outcome),
                    format!("source={}", summary.source_kind),
                ];
                if let Some(binary_path) = summary.binary_path {
                    bits.push(binary_path);
                }
                if let Some(release_version) = summary.release_version {
                    bits.push(format!("release={release_version}"));
                }
                if let Some(release_channel) = summary.release_channel {
                    bits.push(format!("channel={release_channel}"));
                }
                if let Some(issue) = summary.issue {
                    bits.push(format!("issue={issue}"));
                }
                self.stdout_line(bits.join("  "));
            }
            return Ok(code);
        }
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self
            .runtime_service()
            .update_from_release(UpdateRuntimeFromReleaseOptions {
                name: name.clone(),
                version,
                channel,
            })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Updated runtime {}", meta.name));
        self.stdout_line(format!("  binary path: {}", meta.binary_path));
        if let Some(install_root) = meta.install_root.as_deref() {
            self.stdout_line(format!("  install root: {install_root}"));
        }
        Ok(0)
    }

    fn handle_runtime_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let runtimes = self.runtime_service().list()?;
        if json_flag {
            self.print_json(&runtimes)?;
            return Ok(0);
        }
        if runtimes.is_empty() {
            self.stdout_line("No runtimes.");
            return Ok(0);
        }
        for meta in runtimes {
            let mut bits = vec![
                meta.name,
                meta.binary_path,
                format!("source={}", meta.source_kind.as_str()),
            ];
            if let Some(release_version) = meta.release_version {
                bits.push(format!("release={release_version}"));
            }
            if let Some(release_channel) = meta.release_channel {
                bits.push(format!("channel={release_channel}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    fn handle_runtime_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.runtime_service().show(name)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        let mut lines = BTreeMap::new();
        lines.insert("kind".to_string(), meta.kind.clone());
        lines.insert("name".to_string(), meta.name.clone());
        lines.insert("binaryPath".to_string(), meta.binary_path.clone());
        lines.insert(
            "sourceKind".to_string(),
            meta.source_kind.as_str().to_string(),
        );
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
        if let Some(description) = meta.description {
            lines.insert("description".to_string(), description);
        }
        if let Some(source_path) = meta.source_path {
            lines.insert("sourcePath".to_string(), source_path);
        }
        if let Some(source_url) = meta.source_url {
            lines.insert("sourceUrl".to_string(), source_url);
        }
        if let Some(source_manifest_url) = meta.source_manifest_url {
            lines.insert("sourceManifestUrl".to_string(), source_manifest_url);
        }
        if let Some(source_sha256) = meta.source_sha256 {
            lines.insert("sourceSha256".to_string(), source_sha256);
        }
        if let Some(release_version) = meta.release_version {
            lines.insert("releaseVersion".to_string(), release_version);
        }
        if let Some(release_channel) = meta.release_channel {
            lines.insert("releaseChannel".to_string(), release_channel);
        }
        if let Some(release_selector_kind) = meta.release_selector_kind {
            lines.insert(
                "releaseSelectorKind".to_string(),
                release_selector_kind.as_str().to_string(),
            );
        }
        if let Some(release_selector_value) = meta.release_selector_value {
            lines.insert("releaseSelectorValue".to_string(), release_selector_value);
        }
        if let Some(install_root) = meta.install_root {
            lines.insert("installRoot".to_string(), install_root);
        }
        for (key, value) in lines {
            self.stdout_line(format!("{key}: {value}"));
        }
        Ok(0)
    }

    fn handle_runtime_verify(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, all_flag) = Self::consume_flag(args, "--all");
        if all_flag {
            Self::assert_no_extra_args(&args)?;
            let summaries = self.runtime_service().verify_all()?;
            let code = if summaries.iter().all(|summary| summary.healthy) {
                0
            } else {
                1
            };

            if json_flag {
                self.print_json(&summaries)?;
                return Ok(code);
            }

            if summaries.is_empty() {
                self.stdout_line("No runtimes.");
                return Ok(code);
            }

            for summary in summaries {
                let mut bits = vec![
                    summary.name,
                    summary.binary_path,
                    format!("source={}", summary.source_kind),
                    format!("healthy={}", summary.healthy),
                ];
                if let Some(issue) = summary.issue {
                    bits.push(format!("issue={issue}"));
                }
                self.stdout_line(bits.join("  "));
            }
            return Ok(code);
        }

        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.runtime_service().verify(name)?;
        let code = if summary.healthy { 0 } else { 1 };

        if json_flag {
            self.print_json(&summary)?;
            return Ok(code);
        }

        self.stdout_line(format!("name: {}", summary.name));
        self.stdout_line(format!("binaryPath: {}", summary.binary_path));
        self.stdout_line(format!("sourceKind: {}", summary.source_kind));
        self.stdout_line(format!("healthy: {}", summary.healthy));
        if let Some(source_path) = summary.source_path {
            self.stdout_line(format!("sourcePath: {source_path}"));
        }
        if let Some(source_url) = summary.source_url {
            self.stdout_line(format!("sourceUrl: {source_url}"));
        }
        if let Some(source_manifest_url) = summary.source_manifest_url {
            self.stdout_line(format!("sourceManifestUrl: {source_manifest_url}"));
        }
        if let Some(source_sha256) = summary.source_sha256 {
            self.stdout_line(format!("sourceSha256: {source_sha256}"));
        }
        if let Some(release_version) = summary.release_version {
            self.stdout_line(format!("releaseVersion: {release_version}"));
        }
        if let Some(release_channel) = summary.release_channel {
            self.stdout_line(format!("releaseChannel: {release_channel}"));
        }
        if let Some(install_root) = summary.install_root {
            self.stdout_line(format!("installRoot: {install_root}"));
        }
        if let Some(issue) = summary.issue {
            self.stdout_line(format!("issue: {issue}"));
        }
        Ok(code)
    }

    fn handle_runtime_which(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.runtime_service().which(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }
        self.stdout_line(summary.binary_path);
        Ok(0)
    }

    fn handle_runtime_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.runtime_service().remove(name)?;
        self.stdout_line(format!("Removed runtime {}", meta.name));
        Ok(0)
    }

    fn dispatch_runtime_command(&self, action: &str, rest: Vec<String>) -> Result<i32, String> {
        match action {
            "add" => self.handle_runtime_add(rest),
            "install" => self.handle_runtime_install(rest),
            "update" => self.handle_runtime_update(rest),
            "releases" => self.handle_runtime_releases(rest),
            "list" => self.handle_runtime_list(rest),
            "show" => self.handle_runtime_show(rest),
            "verify" => self.handle_runtime_verify(rest),
            "which" => self.handle_runtime_which(rest),
            "remove" | "rm" => self.handle_runtime_remove(rest),
            _ => Err(format!("unknown runtime command: {action}")),
        }
    }

    fn handle_init_command(&self, shell: &str, args: Vec<String>) -> Result<i32, String> {
        let shell = if shell.is_empty() {
            resolve_shell_name(None, &self.env)
        } else {
            shell.to_string()
        };
        if !matches!(shell.as_str(), "bash" | "fish" | "sh" | "zsh") {
            return Err(format!("unsupported init shell: {shell}"));
        }
        Self::assert_no_extra_args(&args)?;
        print!("{}", render_init_script(&self.command_example(), &shell)?);
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
                "list" => self.handle_env_list(rest),
                "show" => self.handle_env_show(rest),
                "status" => self.handle_env_status(rest),
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
