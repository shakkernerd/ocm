use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use dialoguer::{
    Input, Select,
    console::{Style, style},
    theme::ColorfulTheme,
};
use serde_json::Value;
use time::OffsetDateTime;

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
        let name_default = self.setup_name_default(mode, local_defaults.as_ref());
        let name = loop {
            let raw = self.prompt_with_default("Environment name", &name_default)?;
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
            service_requested: self.prompt_yes_no("Install a persistent service?", true)?,
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
            let items = ["Yes", "No"];
            let selection = Select::with_theme(&Self::setup_theme())
                .with_prompt(label)
                .default(if default { 0 } else { 1 })
                .items(items)
                .interact()
                .map_err(|error| error.to_string())?;
            return Ok(selection == 0);
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
            KeyValueRow::muted("Controls", "Use arrows, type, and press Enter"),
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

    fn setup_name_default(
        &self,
        mode: SetupMode,
        local_defaults: Option<&LocalSetupDefaults>,
    ) -> String {
        if !self.use_pretty_setup_prompts() {
            return match mode {
                SetupMode::Stable | SetupMode::Beta | SetupMode::Version => "default".to_string(),
                SetupMode::LocalCommand => {
                    if local_defaults.is_some() {
                        "dev".to_string()
                    } else {
                        "local".to_string()
                    }
                }
            };
        }

        match mode {
            SetupMode::Stable | SetupMode::Beta | SetupMode::Version => {
                self.suggest_generated_env_name()
            }
            SetupMode::LocalCommand => local_defaults
                .and_then(|defaults| self.preferred_checkout_env_name(defaults))
                .unwrap_or_else(|| self.suggest_generated_env_name()),
        }
    }

    fn preferred_checkout_env_name(&self, defaults: &LocalSetupDefaults) -> Option<String> {
        let file_name = defaults.cwd.file_name()?.to_string_lossy();
        let fragment = sanitize_name_fragment(&file_name)?;
        let preferred = if fragment == "openclaw" {
            "dev".to_string()
        } else {
            format!("{fragment}-dev")
        };
        Some(self.ensure_available_env_name(&preferred))
    }

    fn suggest_generated_env_name(&self) -> String {
        const NAME_POOL: &[&str] = &[
            "atlas-harbor",
            "aurora-trail",
            "cedar-signal",
            "cinder-lantern",
            "cobalt-raven",
            "copper-anchor",
            "drift-forge",
            "ember-meadow",
            "glimmer-otter",
            "harbor-mint",
            "iris-brook",
            "juniper-comet",
            "kindle-bay",
            "linen-peak",
            "maple-signal",
            "meadow-lark",
            "midnight-fern",
            "north-hollow",
            "orchid-trail",
            "quiet-marble",
            "river-ember",
            "saffron-brook",
            "silver-cove",
            "solstice-harbor",
            "spring-fable",
            "summit-lantern",
            "tidal-forge",
            "topaz-raven",
            "violet-anchor",
            "willow-comet",
            "winter-grove",
            "zephyr-mesa",
        ];

        let now = OffsetDateTime::now_utc().unix_timestamp_nanos();
        let mut seed = (now as u64) ^ ((now >> 64) as u64) ^ (u64::from(std::process::id()) << 16);
        seed ^= seed >> 30;
        seed = seed.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        seed ^= seed >> 27;
        seed = seed.wrapping_mul(0x94d0_49bb_1331_11eb);
        seed ^= seed >> 31;

        let start = (seed as usize) % NAME_POOL.len();
        let step = 11usize;
        for attempt in 0..NAME_POOL.len() {
            let candidate = NAME_POOL[(start + attempt * step) % NAME_POOL.len()];
            if self
                .environment_service()
                .find(candidate)
                .ok()
                .flatten()
                .is_none()
            {
                return candidate.to_string();
            }
        }

        self.ensure_available_env_name("openclaw-env")
    }

    fn ensure_available_env_name(&self, preferred: &str) -> String {
        if self
            .environment_service()
            .find(preferred)
            .ok()
            .flatten()
            .is_none()
        {
            return preferred.to_string();
        }

        for suffix in 2..1000 {
            let candidate = format!("{preferred}-{suffix}");
            if self
                .environment_service()
                .find(&candidate)
                .ok()
                .flatten()
                .is_none()
            {
                return candidate;
            }
        }

        preferred.to_string()
    }
}

#[derive(Clone, Copy)]
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

fn sanitize_name_fragment(value: &str) -> Option<String> {
    let mut out = String::new();
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if matches!(ch, '-' | '_' | '.') {
            Some(ch)
        } else {
            None
        };

        match normalized {
            Some(ch) => out.push(ch),
            None if !out.ends_with('-') => out.push('-'),
            None => {}
        }
    }

    let trimmed = out.trim_matches(['-', '.', '_']).to_string();
    if trimmed.is_empty() {
        return None;
    }
    if !trimmed
        .chars()
        .next()
        .map(|ch| ch.is_ascii_alphanumeric())
        .unwrap_or(false)
    {
        return None;
    }
    Some(trimmed)
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

#[cfg(test)]
mod tests {
    use super::sanitize_name_fragment;

    #[test]
    fn sanitize_name_fragment_normalizes_checkout_names() {
        assert_eq!(
            sanitize_name_fragment("OpenClaw Dev Repo").as_deref(),
            Some("openclaw-dev-repo")
        );
        assert_eq!(
            sanitize_name_fragment("demo_repo-1").as_deref(),
            Some("demo_repo-1")
        );
    }

    #[test]
    fn sanitize_name_fragment_rejects_empty_results() {
        assert_eq!(sanitize_name_fragment("!!!"), None);
    }
}
