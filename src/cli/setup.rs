use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dialoguer::{
    Confirm, Input, Select,
    console::{Style, style},
    theme::ColorfulTheme,
};
use serde_json::Value;

use super::{Cli, start::StartOnboardingMode};
use crate::cli::start::StartRequest;
use crate::infra::terminal::{KeyValueRow, Tone, paint, render_key_value_card};
use crate::store::validate_name;

impl Cli {
    pub(super) fn handle_setup_command(&self, args: Vec<String>) -> Result<i32, String> {
        Self::assert_no_extra_args(&args)?;

        let local_defaults = self.detect_local_setup_defaults();
        self.stdout_lines(self.setup_intro_lines(local_defaults.as_ref()));

        let mode = self.prompt_setup_mode()?;
        let name_default = match mode {
            SetupMode::Stable | SetupMode::Beta | SetupMode::Version => "default",
            SetupMode::LocalCommand => {
                if local_defaults.is_some() {
                    "dev"
                } else {
                    "local"
                }
            }
        };
        let name = loop {
            let raw = self.prompt_with_default("Environment name", name_default)?;
            match validate_name(&raw, "Environment name") {
                Ok(value) => break value,
                Err(error) => self.stderr_line(format!("ocm: {error}")),
            }
        };

        let (version, channel, command, cwd) = match mode {
            SetupMode::Stable => (None, Some("stable".to_string()), None, None),
            SetupMode::Beta => (None, Some("beta".to_string()), None, None),
            SetupMode::Version => (
                Some(self.prompt_required("Release version")?),
                None,
                None,
                None,
            ),
            SetupMode::LocalCommand => (
                None,
                None,
                Some(
                    self.prompt_with_default(
                        "Local command",
                        local_defaults
                            .as_ref()
                            .map(|defaults| defaults.command.as_str())
                            .unwrap_or("openclaw"),
                    )?,
                ),
                Some(
                    self.prompt_with_default(
                        "Project directory",
                        &local_defaults
                            .as_ref()
                            .map(|defaults| defaults.cwd.display().to_string())
                            .unwrap_or_else(|| self.cwd.display().to_string()),
                    )?,
                ),
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
        if self.use_pretty_setup_prompts() {
            let items = [
                "Latest stable release (recommended)",
                "Beta release",
                "Specific release version",
                "Local command or checkout",
            ];
            let selection = Select::with_theme(&Self::setup_theme())
                .with_prompt("How should OCM run OpenClaw?")
                .default(0)
                .items(items)
                .interact()
                .map_err(|error| error.to_string())?;
            return Ok(match selection {
                0 => SetupMode::Stable,
                1 => SetupMode::Beta,
                2 => SetupMode::Version,
                3 => SetupMode::LocalCommand,
                _ => unreachable!("dialoguer selection must stay within the provided items"),
            });
        }

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
        if self.use_pretty_setup_prompts() {
            loop {
                let value = Input::<String>::with_theme(&Self::setup_theme())
                    .with_prompt(label)
                    .interact_text()
                    .map(|value| value.trim().to_string())
                    .map_err(|error| error.to_string())?;
                if !value.is_empty() {
                    return Ok(value);
                }
                self.stderr_line(format!("ocm: {label} is required"));
            }
        }

        loop {
            let value = self.prompt_line(label, None)?;
            if !value.trim().is_empty() {
                return Ok(value.trim().to_string());
            }
            self.stderr_line(format!("ocm: {label} is required"));
        }
    }

    fn prompt_with_default(&self, label: &str, default: &str) -> Result<String, String> {
        if self.use_pretty_setup_prompts() {
            return Input::<String>::with_theme(&Self::setup_theme())
                .with_prompt(label)
                .default(default.to_string())
                .interact_text()
                .map(|value| value.trim().to_string())
                .map_err(|error| error.to_string());
        }

        let value = self.prompt_line(label, Some(default))?;
        if value.trim().is_empty() {
            Ok(default.to_string())
        } else {
            Ok(value.trim().to_string())
        }
    }

    fn prompt_yes_no(&self, label: &str, default: bool) -> Result<bool, String> {
        if self.use_pretty_setup_prompts() {
            return Confirm::with_theme(&Self::setup_theme())
                .with_prompt(label)
                .default(default)
                .interact()
                .map_err(|error| error.to_string());
        }

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

    fn detect_local_setup_defaults(&self) -> Option<LocalSetupDefaults> {
        self.cwd
            .ancestors()
            .take(6)
            .find_map(detect_openclaw_checkout)
            .map(|cwd| LocalSetupDefaults {
                command: "pnpm openclaw".to_string(),
                cwd,
            })
    }

    fn use_pretty_setup_prompts(&self) -> bool {
        self.stdin_is_terminal() && self.stdout_is_terminal()
    }

    fn setup_intro_lines(&self, local_defaults: Option<&LocalSetupDefaults>) -> Vec<String> {
        if !self.use_pretty_setup_prompts() {
            let mut lines = vec![
                "OpenClaw setup".to_string(),
                String::new(),
                "Choose how you want to run OpenClaw:".to_string(),
                "  1. Latest stable release (recommended)".to_string(),
                "  2. Beta release".to_string(),
                "  3. Specific release version".to_string(),
                "  4. Local command or checkout".to_string(),
            ];
            if let Some(defaults) = local_defaults {
                lines.push(String::new());
                lines.push(format!(
                    "Detected local OpenClaw checkout: {}",
                    defaults.cwd.display()
                ));
            }
            lines.push(String::new());
            return lines;
        }

        let color = self.color_output_enabled_for(self.stdout_is_terminal(), self.color_mode());
        let mut lines = vec![paint("OpenClaw setup", Tone::Strong, color), String::new()];
        let mut rows = vec![
            KeyValueRow::accent("Recommended", "Latest stable release"),
            KeyValueRow::plain(
                "Also available",
                "Beta release, exact version, or a local checkout",
            ),
            KeyValueRow::muted(
                "What happens",
                "OCM creates one env, binds it, and can start onboarding for you",
            ),
        ];
        if let Some(defaults) = local_defaults {
            rows.push(KeyValueRow::muted(
                "Detected checkout",
                defaults.cwd.display().to_string(),
            ));
        }
        lines.extend(render_key_value_card("Choose a setup path", &rows, color));
        lines.push(String::new());
        lines
    }

    fn setup_theme() -> ColorfulTheme {
        let mut theme = ColorfulTheme::default();
        theme.prompt_style = Style::new().bold().cyan();
        theme.values_style = Style::new().yellow();
        theme.active_item_style = Style::new().bold().cyan();
        theme.prompt_prefix = style("◆".to_string()).cyan();
        theme.success_prefix = style("✓".to_string()).green();
        theme.success_suffix = style("·".to_string()).dim();
        theme.error_prefix = style("✗".to_string()).red();
        theme
    }
}

enum SetupMode {
    Stable,
    Beta,
    Version,
    LocalCommand,
}

struct LocalSetupDefaults {
    command: String,
    cwd: PathBuf,
}

fn detect_openclaw_checkout(path: &Path) -> Option<PathBuf> {
    let package_json = path.join("package.json");
    let scripts_dir = path.join("scripts");
    if !package_json.exists() || !scripts_dir.join("run-node.mjs").exists() {
        return None;
    }

    let contents = fs::read_to_string(package_json).ok()?;
    let package: Value = serde_json::from_str(&contents).ok()?;
    if package.get("name").and_then(Value::as_str) == Some("openclaw") {
        Some(path.to_path_buf())
    } else {
        None
    }
}
