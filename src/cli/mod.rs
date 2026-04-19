mod dev;
mod doctor;
mod env;
mod help;
mod init;
mod internal;
mod launcher;
mod migrate;
mod release;
mod render;
mod runtime;
mod self_cmd;
mod service;
mod setup;
mod start;
mod upgrade;

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;

use crate::env::EnvironmentService;
use crate::launcher::LauncherService;
use crate::runtime::RuntimeService;
use crate::service::ServiceService;
use crate::store::ensure_store;
use crate::supervisor::SupervisorService;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const INTERNAL_COLOR_MODE_ENV: &str = "OCM_INTERNAL_COLOR_MODE";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "always" => Ok(Self::Always),
            "never" => Ok(Self::Never),
            _ => Err("--color must be one of auto, always, or never".to_string()),
        }
    }

    fn from_env(env: &BTreeMap<String, String>) -> Self {
        match env.get(INTERNAL_COLOR_MODE_ENV).map(String::as_str) {
            Some("always") => Self::Always,
            Some("never") => Self::Never,
            _ => Self::Auto,
        }
    }

    fn as_env_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

pub struct Cli {
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

impl Cli {
    fn with_color_mode(&self, mode: ColorMode) -> Self {
        let mut env = self.env.clone();
        env.insert(
            INTERNAL_COLOR_MODE_ENV.to_string(),
            mode.as_env_value().to_string(),
        );
        Self {
            env,
            cwd: self.cwd.clone(),
        }
    }

    fn color_mode(&self) -> ColorMode {
        ColorMode::from_env(&self.env)
    }

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

    fn supervisor_service(&self) -> SupervisorService<'_> {
        SupervisorService::new(&self.env, &self.cwd)
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

    fn stdout_is_terminal(&self) -> bool {
        io::stdout().is_terminal()
    }

    fn stdin_is_terminal(&self) -> bool {
        io::stdin().is_terminal()
    }

    fn stderr_is_terminal(&self) -> bool {
        io::stderr().is_terminal()
    }

    fn color_output_enabled_for(&self, is_terminal: bool, mode: ColorMode) -> bool {
        match mode {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => {
                is_terminal
                    && !self.env.contains_key("NO_COLOR")
                    && self
                        .env
                        .get("TERM")
                        .map(|value| value != "dumb")
                        .unwrap_or(true)
            }
        }
    }

    fn progress_output_enabled(&self) -> bool {
        self.stderr_is_terminal()
            && self
                .env
                .get("TERM")
                .map(|value| value != "dumb")
                .unwrap_or(true)
    }

    fn progress_color_enabled(&self) -> bool {
        self.color_output_enabled_for(self.stderr_is_terminal(), self.color_mode())
    }

    fn with_progress<T, F>(&self, message: impl Into<String>, work: F) -> Result<T, String>
    where
        F: FnOnce() -> Result<T, String>,
    {
        if !self.progress_output_enabled() {
            return work();
        }

        let bar = ProgressBar::new_spinner();
        let template = if self.progress_color_enabled() {
            "{spinner:.cyan} {msg}"
        } else {
            "{spinner} {msg}"
        };
        let style = ProgressStyle::with_template(template)
            .map_err(|error| error.to_string())?
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);
        bar.set_style(style);
        bar.set_message(message.into());
        bar.enable_steady_tick(Duration::from_millis(90));
        bar.tick();

        let result = work();
        bar.finish_and_clear();
        result
    }

    fn consume_human_output_flags(
        &self,
        args: Vec<String>,
        command: &str,
    ) -> Result<(Vec<String>, bool, render::RenderProfile), String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, raw_flag) = Self::consume_flag(args, "--raw");
        let (args, color_mode) = Self::consume_color_option(args)?;
        if json_flag && raw_flag {
            return Err(format!("{command} accepts only one of --json or --raw"));
        }

        let color_mode = color_mode.unwrap_or_else(|| self.color_mode());
        let pretty_enabled = self.stdout_is_terminal() || matches!(color_mode, ColorMode::Always);
        let profile = if raw_flag || !pretty_enabled {
            render::RenderProfile::raw()
        } else {
            render::RenderProfile::pretty(self.color_output_enabled_for(true, color_mode))
        };
        Ok((args, json_flag, profile))
    }

    fn consume_color_option(args: Vec<String>) -> Result<(Vec<String>, Option<ColorMode>), String> {
        let (args, color_raw) = Self::consume_option(args, "--color")?;
        let color_raw = Self::require_option_value(color_raw, "--color")?;
        let color_mode = color_raw.as_deref().map(ColorMode::parse).transpose()?;
        Ok((args, color_mode))
    }

    fn consume_leading_color_option(
        args: Vec<String>,
    ) -> Result<(Vec<String>, Option<ColorMode>), String> {
        let Some(first) = args.first() else {
            return Ok((args, None));
        };

        if let Some(value) = first.strip_prefix("--color=") {
            if value.trim().is_empty() {
                return Err("--color requires a value".to_string());
            }
            let color_mode = ColorMode::parse(value)?;
            return Ok((args[1..].to_vec(), Some(color_mode)));
        }

        if first == "--color" {
            let Some(value) = args.get(1) else {
                return Err("--color requires a value".to_string());
            };
            let color_mode = ColorMode::parse(value)?;
            let mut remaining = Vec::with_capacity(args.len().saturating_sub(2));
            remaining.extend(args[2..].iter().cloned());
            return Ok((remaining, Some(color_mode)));
        }

        Ok((args, None))
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

    fn active_env_name(&self) -> Result<String, String> {
        self.env
            .get("OCM_ACTIVE_ENV")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .ok_or_else(|| {
                let command = self.command_example();
                format!(
                    "no active environment; run eval \"$({command} env use <name>)\" or use \"{command} env run <name> -- ...\""
                )
            })
    }

    fn handle_active_env_run_shorthand(&self, openclaw_args: Vec<String>) -> Result<i32, String> {
        let mut args = vec![self.active_env_name()?, "--".to_string()];
        args.extend(openclaw_args);
        self.handle_env_run(args)
    }

    fn explicit_env_name_from_shorthand(token: &str) -> Option<Result<String, String>> {
        let name = token.strip_prefix('@')?;
        if name.trim().is_empty() {
            Some(Err("env shorthand requires a target like @mira".to_string()))
        } else {
            Some(Ok(name.to_string()))
        }
    }

    fn handle_named_env_run_shorthand(
        &self,
        name: String,
        args: Vec<String>,
    ) -> Result<i32, String> {
        if !args.iter().any(|arg| arg == "--") {
            return Err("env shorthand requires -- before OpenClaw arguments".to_string());
        }

        let mut run_args = vec![name];
        run_args.extend(args);
        self.handle_env_run(run_args)
    }

    pub fn run(&self, args: Vec<String>) -> i32 {
        let (args, color_mode) = match Self::consume_leading_color_option(args) {
            Ok(result) => result,
            Err(error) => {
                self.stderr_line(format!("ocm: {error}"));
                self.stderr_line(format!(
                    "Run \"{} help\" for usage.",
                    self.command_example()
                ));
                return 1;
            }
        };
        let cli = color_mode.map(|mode| self.with_color_mode(mode));
        let cli = cli.as_ref().unwrap_or(self);

        if let Some(result) = cli.help_result_for_invocation(&args) {
            return match result {
                Ok(code) => code,
                Err(error) => {
                    cli.stderr_line(format!("ocm: {error}"));
                    cli.stderr_line(format!("Run \"{} help\" for usage.", cli.command_example()));
                    1
                }
            };
        }

        if matches!(args[0].as_str(), "--version" | "-v") {
            cli.stdout_line(VERSION);
            return 0;
        }

        if args[0] == "--" {
            return match cli.handle_active_env_run_shorthand(args[1..].to_vec()) {
                Ok(code) => code,
                Err(error) => {
                    cli.stderr_line(format!("ocm: {error}"));
                    cli.stderr_line(format!("Run \"{} help\" for usage.", cli.command_example()));
                    1
                }
            };
        }

        if let Some(target) = Self::explicit_env_name_from_shorthand(&args[0]) {
            return match target
                .and_then(|name| cli.handle_named_env_run_shorthand(name, args[1..].to_vec()))
            {
                Ok(code) => code,
                Err(error) => {
                    cli.stderr_line(format!("ocm: {error}"));
                    cli.stderr_line(format!("Run \"{} help\" for usage.", cli.command_example()));
                    1
                }
            };
        }

        let group = args.first().cloned().unwrap_or_default();
        let action = args.get(1).cloned().unwrap_or_default();
        let rest = if args.len() > 2 {
            args[2..].to_vec()
        } else {
            Vec::new()
        };

        if group == "self" {
            return match cli.dispatch_self_command(action.as_str(), rest) {
                Ok(code) => code,
                Err(error) => {
                    cli.stderr_line(format!("ocm: {error}"));
                    cli.stderr_line(format!("Run \"{} help\" for usage.", cli.command_example()));
                    1
                }
            };
        }

        if let Err(error) = ensure_store(&cli.env, &cli.cwd) {
            cli.stderr_line(format!("ocm: {error}"));
            cli.stderr_line(format!("Run \"{} help\" for usage.", cli.command_example()));
            return 1;
        }

        let result = match group.as_str() {
            "help" => cli.dispatch_help_command(rest),
            "init" => cli.handle_init_command(&action, rest),
            "setup" => cli.handle_setup_command(args[1..].to_vec()),
            "dev" => cli.handle_dev_command(args[1..].to_vec()),
            "start" => cli.handle_start_command(args[1..].to_vec()),
            "upgrade" => cli.handle_upgrade_command(args[1..].to_vec()),
            "doctor" => cli.dispatch_doctor_command(action.as_str(), rest),
            "env" => cli.dispatch_env_command(action.as_str(), rest),
            "migrate" => cli.handle_migrate_command(args[1..].to_vec()),
            "adopt" => cli.dispatch_adopt_command(action.as_str(), rest),
            "release" => cli.dispatch_release_command(action.as_str(), rest),
            "launcher" => cli.dispatch_launcher_command(action.as_str(), rest),
            "runtime" => cli.dispatch_runtime_command(action.as_str(), rest),
            "__daemon" => cli.dispatch_internal_command(action.as_str(), rest),
            "service" => cli.dispatch_service_command(action.as_str(), rest),
            _ => Err(format!("unknown command group: {group}")),
        };

        match result {
            Ok(code) => code,
            Err(error) => {
                cli.stderr_line(format!("ocm: {error}"));
                cli.stderr_line(format!("Run \"{} help\" for usage.", cli.command_example()));
                1
            }
        }
    }
}
