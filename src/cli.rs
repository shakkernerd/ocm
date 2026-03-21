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
            .map_err(|error| error.to_string())?;
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
}
