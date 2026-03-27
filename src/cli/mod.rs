mod env;
mod help;
mod init;
mod launcher;
mod render;
mod runtime;
mod service;

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
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

    fn stdout_is_terminal(&self) -> bool {
        io::stdout().is_terminal()
    }

    fn color_output_enabled(&self) -> bool {
        self.stdout_is_terminal()
            && !self.env.contains_key("NO_COLOR")
            && self
                .env
                .get("TERM")
                .map(|value| value != "dumb")
                .unwrap_or(true)
    }

    fn consume_human_output_flags(
        &self,
        args: Vec<String>,
        command: &str,
    ) -> Result<(Vec<String>, bool, render::RenderProfile), String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, raw_flag) = Self::consume_flag(args, "--raw");
        if json_flag && raw_flag {
            return Err(format!("{command} accepts only one of --json or --raw"));
        }

        let profile = if raw_flag || !self.stdout_is_terminal() {
            render::RenderProfile::raw()
        } else {
            render::RenderProfile::pretty(self.color_output_enabled())
        };
        Ok((args, json_flag, profile))
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
        if let Some(result) = self.help_result_for_invocation(&args) {
            return match result {
                Ok(code) => code,
                Err(error) => {
                    self.stderr_line(format!("ocm: {error}"));
                    self.stderr_line(format!(
                        "Run \"{} help\" for usage.",
                        self.command_example()
                    ));
                    1
                }
            };
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
            "help" => self.dispatch_help_command(rest),
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
