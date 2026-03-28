use std::io::{self, Write};

use super::{Cli, start::StartOnboardingMode};
use crate::cli::start::StartRequest;
use crate::store::validate_name;

impl Cli {
    pub(super) fn handle_setup_command(&self, args: Vec<String>) -> Result<i32, String> {
        Self::assert_no_extra_args(&args)?;

        self.stdout_line("OpenClaw setup");
        self.stdout_line("");
        self.stdout_line("Choose how to run OpenClaw:");
        self.stdout_line("  1. Latest stable release");
        self.stdout_line("  2. Beta release");
        self.stdout_line("  3. Exact release version");
        self.stdout_line("  4. Local command");
        self.stdout_line("");

        let name = loop {
            let raw = self.prompt_with_default("Env name", "default")?;
            match validate_name(&raw, "Environment name") {
                Ok(value) => break value,
                Err(error) => self.stderr_line(format!("ocm: {error}")),
            }
        };

        let mode = self.prompt_setup_mode()?;
        let (version, channel, command, cwd) = match mode {
            SetupMode::Stable => (None, Some("stable".to_string()), None, None),
            SetupMode::Beta => (None, Some("beta".to_string()), None, None),
            SetupMode::Version => (
                Some(self.prompt_required("OpenClaw version")?),
                None,
                None,
                None,
            ),
            SetupMode::LocalCommand => (
                None,
                None,
                Some(self.prompt_required("Command")?),
                Some(self.prompt_with_default(
                    "Working directory",
                    &self.cwd.display().to_string(),
                )?),
            ),
        };

        let request = StartRequest {
            name,
            root: None,
            gateway_port: None,
            protect: false,
            service_requested: self.prompt_yes_no("Install a persistent service?", false)?,
            onboarding_mode: if self.prompt_yes_no("Run onboarding now?", true)? {
                StartOnboardingMode::Always
            } else {
                StartOnboardingMode::Never
            },
            runtime_name: None,
            launcher_name: None,
            version,
            channel,
            command,
            cwd,
        };

        self.stdout_line("");
        self.run_start_request(request, false)
    }

    fn prompt_setup_mode(&self) -> Result<SetupMode, String> {
        loop {
            let raw = self.prompt_with_default("Mode [1-4]", "1")?;
            match raw.trim().to_ascii_lowercase().as_str() {
                "1" | "stable" | "latest" => return Ok(SetupMode::Stable),
                "2" | "beta" => return Ok(SetupMode::Beta),
                "3" | "version" => return Ok(SetupMode::Version),
                "4" | "local" | "command" | "dev" => return Ok(SetupMode::LocalCommand),
                _ => self.stderr_line("ocm: choose 1, 2, 3, or 4"),
            }
        }
    }

    fn prompt_required(&self, label: &str) -> Result<String, String> {
        loop {
            let value = self.prompt_line(label, None)?;
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
            self.stderr_line(format!("ocm: {label} is required"));
        }
    }

    fn prompt_with_default(&self, label: &str, default: &str) -> Result<String, String> {
        let value = self.prompt_line(label, Some(default))?;
        if value.trim().is_empty() {
            Ok(default.to_string())
        } else {
            Ok(value.trim().to_string())
        }
    }

    fn prompt_yes_no(&self, label: &str, default: bool) -> Result<bool, String> {
        let suffix = if default { "[Y/n]" } else { "[y/N]" };
        loop {
            let answer = self.prompt_line(&format!("{label} {suffix}"), None)?;
            match answer.trim().to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.stderr_line("ocm: answer yes or no"),
            }
        }
    }

    fn prompt_line(&self, label: &str, default: Option<&str>) -> Result<String, String> {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        match default {
            Some(default) => write!(handle, "{label} [{default}]: "),
            None => write!(handle, "{label}: "),
        }
        .map_err(|error| error.to_string())?;
        handle.flush().map_err(|error| error.to_string())?;

        let mut input = String::new();
        io::stdin()
            .read_line(&mut input)
            .map_err(|error| error.to_string())?;
        Ok(input.trim_end_matches(['\n', '\r']).to_string())
    }
}

enum SetupMode {
    Stable,
    Beta,
    Version,
    LocalCommand,
}
