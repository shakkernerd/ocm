use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};

use super::Cli;
use super::render::RenderProfile;
use crate::env::{CreateEnvironmentOptions, EnvDevMeta, EnvMeta};
use crate::infra::process::run_direct;
use crate::infra::shell::build_openclaw_env;
use crate::infra::terminal::{Cell, KeyValueRow, Tone, paint, render_key_value_card, render_table};
use crate::openclaw_repo::{
    detect_openclaw_checkout, discover_openclaw_checkout, ensure_openclaw_worktree,
};
use crate::service::service_backend_support_error;
use crate::store::{
    derive_env_paths, display_path, ensure_minimum_local_openclaw_config, ensure_store, read_json,
    resolve_absolute_path, validate_name, write_json,
};

const DEV_PREFERENCES_KIND: &str = "ocm-dev-preferences";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DevPreferences {
    kind: String,
    preferred_repo_root: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevStatusSummary {
    env_name: String,
    root: String,
    repo_root: String,
    worktree_root: String,
    gateway_port: u32,
    gateway_url: String,
    config_path: String,
    workspace_dir: String,
    service_enabled: bool,
    service_running: bool,
    logs_command: String,
    status_command: String,
}

impl Cli {
    pub(super) fn handle_dev_command(&self, args: Vec<String>) -> Result<i32, String> {
        match args.first().map(String::as_str).unwrap_or("") {
            "" | "help" | "--help" | "-h" => self.dispatch_help_command(vec!["dev".to_string()]),
            "status" => self.handle_dev_status(args[1..].to_vec()),
            _ => self.handle_dev_run(args),
        }
    }

    fn handle_dev_status(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "dev status")?;
        let target = args.first().cloned();
        Self::assert_no_extra_args(&args[target.is_some() as usize..])?;

        let envs = self.environment_service().list()?;
        let mut summaries = envs
            .into_iter()
            .filter_map(|meta| self.build_dev_status_summary(meta).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        summaries.sort_by(|left, right| left.env_name.cmp(&right.env_name));

        if let Some(target) = target {
            let target = validate_name(&target, "Environment name")?;
            let summary = summaries
                .into_iter()
                .find(|summary| summary.env_name == target)
                .ok_or_else(|| format!("environment \"{target}\" is not a dev env"))?;
            if json_flag {
                self.print_json(&summary)?;
            } else {
                self.stdout_lines(render_dev_status(&summary, profile));
            }
            return Ok(0);
        }

        if json_flag {
            self.print_json(&summaries)?;
            return Ok(0);
        }

        if summaries.is_empty() {
            self.stdout_line("No dev envs.");
            return Ok(0);
        }

        self.stdout_lines(render_dev_status_list(&summaries, profile));
        Ok(0)
    }

    fn handle_dev_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, force) = Self::consume_flag(args, "--force");
        let (args, service_requested) = Self::consume_flag(args, "--service");
        let (args, watch) = Self::consume_flag(args, "--watch");
        let (args, onboard) = Self::consume_flag(args, "--onboard");
        let (args, repo_root) = Self::consume_option(args, "--repo")?;
        let repo_root = Self::require_option_value(repo_root, "--repo")?;
        let (args, root) = Self::consume_option(args, "--root")?;
        let root = Self::require_option_value(root, "--root")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--port")?),
            None => None,
        };
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;
        if force && !watch {
            return Err("dev accepts --force only with --watch".to_string());
        }
        if watch && service_requested {
            return Err("dev cannot combine --watch with --service".to_string());
        }
        if service_requested && let Some(error) = service_backend_support_error(&self.env) {
            return Err(error);
        }
        let name = validate_name(name, "Environment name")?;

        let (meta, created) = self.ensure_dev_env(&name, repo_root, root, gateway_port)?;
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let stderr_profile = self.dev_stderr_profile();
        let watch_takes_over_service = watch && force && meta.service_running;
        if !service_requested && meta.service_running && !watch_takes_over_service {
            return Err(format!(
                "dev env {} is already running in the background; stop it first with {} service stop {}, inspect it with {} logs {} --all-streams --follow, or rerun with --watch --force to take it over temporarily",
                meta.name,
                self.command_example(),
                meta.name,
                self.command_example(),
                meta.name
            ));
        }
        self.stderr_lines(render_dev_run_summary(
            &meta,
            created,
            service_requested,
            watch,
            onboard,
            stderr_profile,
        ));

        let install_code = self.ensure_dev_dependencies(&meta)?;
        if install_code != 0 {
            return Ok(install_code);
        }

        if onboard {
            self.stderr_lines(render_dev_run_step(
                "Onboarding",
                format!("Running local onboarding in {}", dev.worktree_root),
                stderr_profile,
            ));
            let code = self.run_dev_onboard(&meta)?;
            if code != 0 {
                return Ok(code);
            }
        }

        if service_requested {
            self.stderr_lines(render_dev_run_step(
                "Service",
                format!(
                    "Installing and starting {} in the OCM background service",
                    meta.name
                ),
                stderr_profile,
            ));
            self.service_service().install(&meta.name)?;
            self.service_service().start(&meta.name)?;
            self.stdout_lines(render_dev_service_started(
                &meta,
                &self.command_example(),
                self.dev_stdout_profile(),
            ));
            return Ok(0);
        }

        if watch {
            if watch_takes_over_service {
                self.stderr_lines(render_dev_run_step(
                    "Takeover",
                    format!(
                        "Stopping background service for {} while watch takes over; OCM will restore it when watch exits",
                        meta.name
                    ),
                    stderr_profile,
                ));
                self.service_service().stop(&meta.name)?;
            }
            self.stderr_lines(render_dev_run_step(
                "Watch",
                format!(
                    "Watching {} on port {}",
                    dev.worktree_root,
                    meta.gateway_port.unwrap_or_default()
                ),
                stderr_profile,
            ));
            let code = self.run_dev_gateway_watch(&meta)?;
            if watch_takes_over_service {
                self.stderr_lines(render_dev_run_step(
                    "Restore",
                    format!("Starting background service for {}", meta.name),
                    stderr_profile,
                ));
                self.service_service().start(&meta.name)?;
                self.stdout_lines(render_dev_service_restored(
                    &meta,
                    &self.command_example(),
                    self.dev_stdout_profile(),
                ));
            }
            return Ok(code);
        }

        self.stderr_lines(render_dev_run_step(
            "Gateway",
            format!(
                "Starting {} on port {} from {}",
                meta.name,
                meta.gateway_port.unwrap_or_default(),
                dev.worktree_root
            ),
            stderr_profile,
        ));
        self.run_dev_gateway(&meta)
    }

    fn ensure_dev_env(
        &self,
        name: &str,
        repo_root: Option<String>,
        root: Option<String>,
        gateway_port: Option<u32>,
    ) -> Result<(EnvMeta, bool), String> {
        if let Some(existing) = self.environment_service().find(name)? {
            if gateway_port.is_some() {
                return Err(format!(
                    "dev cannot change the port for existing env {}; use a new env name or keep the current port",
                    existing.name
                ));
            }

            let dev = existing.dev.as_ref().ok_or_else(|| {
                format!(
                    "environment \"{}\" is not a dev env; use a new env name for `ocm dev`",
                    existing.name
                )
            })?;
            let existing_repo = PathBuf::from(&dev.repo_root);
            if let Some(repo_root) = repo_root {
                let requested = resolve_absolute_path(&repo_root, &self.env, &self.cwd)?;
                if requested != existing_repo {
                    return Err(format!(
                        "dev cannot change the repo for existing env {}; current repo is {}",
                        existing.name, dev.repo_root
                    ));
                }
            }
            if let Some(root) = root {
                let requested = resolve_absolute_path(&root, &self.env, &self.cwd)?;
                let current = PathBuf::from(&existing.root);
                if requested != current {
                    return Err(format!(
                        "dev cannot change the root for existing env {}; current root is {}",
                        existing.name, existing.root
                    ));
                }
            }

            ensure_openclaw_worktree(&existing_repo, &existing.name)?;
            let meta = self
                .environment_service()
                .apply_effective_gateway_port(existing)?;
            self.save_preferred_dev_repo(&existing_repo)?;
            self.bootstrap_dev_env(&meta)?;
            return Ok((meta, false));
        }

        let repo_root = self.resolve_dev_repo_root(repo_root)?;
        let repo_root = detect_openclaw_checkout(&repo_root).ok_or_else(|| {
            format!(
                "OpenClaw checkout not found at {}",
                display_path(&repo_root)
            )
        })?;
        self.save_preferred_dev_repo(&repo_root)?;
        let worktree_root = ensure_openclaw_worktree(&repo_root, name)?;

        let created = self.environment_service().create(CreateEnvironmentOptions {
            name: name.to_string(),
            root,
            gateway_port,
            service_enabled: false,
            service_running: false,
            default_runtime: None,
            default_launcher: None,
            dev: Some(EnvDevMeta {
                repo_root: display_path(&repo_root),
                worktree_root: display_path(&worktree_root),
            }),
            protected: false,
        });
        let created = match created {
            Ok(meta) => meta,
            Err(error) => {
                let _ = crate::openclaw_repo::remove_openclaw_worktree(&repo_root, &worktree_root);
                return Err(error);
            }
        };

        let created = self
            .environment_service()
            .apply_effective_gateway_port(created)?;
        if let Err(error) = self.bootstrap_dev_env(&created) {
            let _ = self.environment_service().remove(&created.name, true);
            return Err(error);
        }

        Ok((created, true))
    }

    fn resolve_dev_repo_root(&self, repo_root: Option<String>) -> Result<PathBuf, String> {
        if let Some(repo_root) = repo_root {
            return resolve_absolute_path(&repo_root, &self.env, &self.cwd);
        }

        if let Some(repo_root) = discover_openclaw_checkout(&self.cwd) {
            return Ok(repo_root);
        }

        if let Some(repo_root) = self.load_preferred_dev_repo()? {
            if let Some(repo_root) = detect_openclaw_checkout(&repo_root) {
                return Ok(repo_root);
            }
        }

        let repo_root = self.prompt_dev_repo_root()?;
        resolve_absolute_path(&repo_root, &self.env, &self.cwd)
    }

    fn prompt_dev_repo_root(&self) -> Result<String, String> {
        loop {
            let value = self.prompt_required("OpenClaw repo path").map_err(|_| {
                "OpenClaw repo path is required; pass --repo /path/to/openclaw".to_string()
            })?;
            let repo_root = resolve_absolute_path(&value, &self.env, &self.cwd)?;
            if detect_openclaw_checkout(&repo_root).is_some() {
                return Ok(display_path(&repo_root));
            }
            self.stderr_line(format!(
                "ocm: OpenClaw checkout not found at {}",
                display_path(&repo_root)
            ));
        }
    }

    fn load_preferred_dev_repo(&self) -> Result<Option<PathBuf>, String> {
        let path = self.dev_preferences_path()?;
        if !path.exists() {
            return Ok(None);
        }

        let prefs = read_json::<DevPreferences>(&path)?;
        Ok(prefs.preferred_repo_root.map(PathBuf::from))
    }

    fn save_preferred_dev_repo(&self, repo_root: &Path) -> Result<(), String> {
        let path = self.dev_preferences_path()?;
        let prefs = DevPreferences {
            kind: DEV_PREFERENCES_KIND.to_string(),
            preferred_repo_root: Some(display_path(repo_root)),
        };
        write_json(&path, &prefs)
    }

    fn dev_preferences_path(&self) -> Result<PathBuf, String> {
        let stores = ensure_store(&self.env, &self.cwd)?;
        Ok(stores.home.join("dev.json"))
    }

    fn bootstrap_dev_env(&self, meta: &EnvMeta) -> Result<(), String> {
        let paths = derive_env_paths(Path::new(&meta.root));
        let (gateway_port, _) = self
            .environment_service()
            .resolve_effective_gateway_port(meta)?;
        ensure_minimum_local_openclaw_config(&paths, gateway_port)
    }

    fn ensure_dev_dependencies(&self, meta: &EnvMeta) -> Result<i32, String> {
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let worktree_root = Path::new(&dev.worktree_root);
        let pnpm_store = worktree_root.join("node_modules").join(".pnpm");
        let tsx_bin = worktree_root.join("node_modules").join(".bin").join("tsx");
        if pnpm_store.exists() && tsx_bin.exists() {
            return Ok(0);
        }

        self.stderr_lines(render_dev_run_step(
            "Dependencies",
            format!("Installing dependencies in {}", dev.worktree_root),
            self.dev_stderr_profile(),
        ));
        run_direct(
            "pnpm",
            &["install".to_string()],
            &build_openclaw_env(meta, &self.env),
            worktree_root,
        )
    }

    fn dev_stderr_profile(&self) -> RenderProfile {
        let color_mode = self.color_mode();
        let pretty_enabled =
            self.stderr_is_terminal() || matches!(color_mode, super::ColorMode::Always);
        if pretty_enabled {
            RenderProfile::pretty(
                self.color_output_enabled_for(self.stderr_is_terminal(), color_mode),
            )
        } else {
            RenderProfile::raw()
        }
    }

    fn dev_stdout_profile(&self) -> RenderProfile {
        let color_mode = self.color_mode();
        let pretty_enabled =
            self.stdout_is_terminal() || matches!(color_mode, super::ColorMode::Always);
        if pretty_enabled {
            RenderProfile::pretty(
                self.color_output_enabled_for(self.stdout_is_terminal(), color_mode),
            )
        } else {
            RenderProfile::raw()
        }
    }

    fn run_dev_onboard(&self, meta: &EnvMeta) -> Result<i32, String> {
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let args = vec![
            "openclaw".to_string(),
            "onboard".to_string(),
            "--mode".to_string(),
            "local".to_string(),
            "--no-install-daemon".to_string(),
        ];
        run_direct(
            "pnpm",
            &args,
            &build_openclaw_env(meta, &self.env),
            Path::new(&dev.worktree_root),
        )
    }

    fn run_dev_gateway(&self, meta: &EnvMeta) -> Result<i32, String> {
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let args = vec![
            "openclaw".to_string(),
            "gateway".to_string(),
            "run".to_string(),
            "--port".to_string(),
            meta.gateway_port.unwrap_or_default().to_string(),
        ];
        run_direct(
            "pnpm",
            &args,
            &build_openclaw_env(meta, &self.env),
            Path::new(&dev.worktree_root),
        )
    }

    fn run_dev_gateway_watch(&self, meta: &EnvMeta) -> Result<i32, String> {
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let args = vec![
            "scripts/watch-node.mjs".to_string(),
            "gateway".to_string(),
            "run".to_string(),
            "--port".to_string(),
            meta.gateway_port.unwrap_or_default().to_string(),
        ];
        let stop_requested = Arc::new(AtomicBool::new(false));
        let signal_flag = Arc::clone(&stop_requested);
        ctrlc::set_handler(move || {
            signal_flag.store(true, Ordering::SeqCst);
        })
        .map_err(|error| format!("failed to install dev watch signal handler: {error}"))?;

        let status = Command::new("node")
            .args(&args)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .env_clear()
            .envs(build_openclaw_env(meta, &self.env))
            .current_dir(Path::new(&dev.worktree_root))
            .status()
            .map_err(|error| format!("failed to run \"node\": {error}"))?;

        Ok(match status.code() {
            Some(code) => code,
            None if stop_requested.load(Ordering::SeqCst) => 130,
            None => 1,
        })
    }

    fn build_dev_status_summary(&self, meta: EnvMeta) -> Result<Option<DevStatusSummary>, String> {
        let Some(dev) = meta.dev.as_ref() else {
            return Ok(None);
        };
        let (gateway_port, _) = self
            .environment_service()
            .resolve_effective_gateway_port(&meta)?;
        let paths = derive_env_paths(Path::new(&meta.root));
        let env_name = meta.name.clone();
        Ok(Some(DevStatusSummary {
            env_name: env_name.clone(),
            root: meta.root,
            repo_root: dev.repo_root.clone(),
            worktree_root: dev.worktree_root.clone(),
            gateway_port,
            gateway_url: dev_gateway_url(gateway_port),
            config_path: display_path(&paths.config_path),
            workspace_dir: display_path(&paths.workspace_dir),
            service_enabled: meta.service_enabled,
            service_running: meta.service_running,
            logs_command: format!(
                "{} logs {} --all-streams --follow",
                self.command_example(),
                env_name
            ),
            status_command: format!("{} service status {}", self.command_example(), env_name),
        }))
    }
}

fn render_dev_status(summary: &DevStatusSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return vec![
            format!("env={}", summary.env_name),
            format!("port={}", summary.gateway_port),
            format!("repo={}", summary.repo_root),
            format!("worktree={}", summary.worktree_root),
            format!("root={}", summary.root),
            format!("url={}", summary.gateway_url),
            format!("config={}", summary.config_path),
            format!("workspace={}", summary.workspace_dir),
            format!("status={}", summary.status_command),
            format!("logs={}", summary.logs_command),
        ];
    }

    let mut lines = vec![paint(
        &format!("Dev env {}", summary.env_name),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Status",
        &[
            KeyValueRow::accent("Port", summary.gateway_port.to_string()),
            KeyValueRow::plain("URL", summary.gateway_url.clone()),
            KeyValueRow::plain(
                "Service",
                if summary.service_running {
                    "running".to_string()
                } else if summary.service_enabled {
                    "enabled".to_string()
                } else {
                    "disabled".to_string()
                },
            ),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Source",
        &[
            KeyValueRow::plain("Repo", summary.repo_root.clone()),
            KeyValueRow::plain("Worktree", summary.worktree_root.clone()),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Next",
        &[
            KeyValueRow::plain("Status", summary.status_command.clone()),
            KeyValueRow::plain("Logs", summary.logs_command.clone()),
        ],
        profile.color,
    ));
    lines
}

fn render_dev_run_summary(
    meta: &EnvMeta,
    created: bool,
    service_requested: bool,
    watch: bool,
    onboard: bool,
    profile: RenderProfile,
) -> Vec<String> {
    let Some(dev) = meta.dev.as_ref() else {
        return Vec::new();
    };
    if !profile.pretty {
        return vec![
            format!(
                "{} dev env {}",
                if created { "prepared" } else { "using" },
                meta.name
            ),
            format!("port={}", meta.gateway_port.unwrap_or_default()),
            format!("repo={}", dev.repo_root),
            format!("worktree={}", dev.worktree_root),
            format!(
                "mode={}",
                if service_requested {
                    "service"
                } else if watch {
                    "watch"
                } else {
                    "run"
                }
            ),
            format!("onboard={onboard}"),
        ];
    }

    let mut lines = vec![paint(
        &format!("Dev env {}", meta.name),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Environment",
        &[
            KeyValueRow::new(
                "State",
                if created { "prepared" } else { "reusing" },
                Tone::Accent,
            ),
            KeyValueRow::accent("Port", meta.gateway_port.unwrap_or_default().to_string()),
            KeyValueRow::plain("Root", meta.root.clone()),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Source",
        &[
            KeyValueRow::plain("Repo", dev.repo_root.clone()),
            KeyValueRow::plain("Worktree", dev.worktree_root.clone()),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Launch",
        &[
            KeyValueRow::plain(
                "Mode",
                if service_requested {
                    "service"
                } else if watch {
                    "watch"
                } else {
                    "run"
                },
            ),
            KeyValueRow::plain("Onboard first", onboard.to_string()),
        ],
        profile.color,
    ));
    lines
}

fn render_dev_service_started(
    meta: &EnvMeta,
    command_example: &str,
    profile: RenderProfile,
) -> Vec<String> {
    let Some(dev) = meta.dev.as_ref() else {
        return Vec::new();
    };

    if !profile.pretty {
        return vec![
            format!("service started for {}", meta.name),
            format!("port={}", meta.gateway_port.unwrap_or_default()),
            format!(
                "url={}",
                dev_gateway_url(meta.gateway_port.unwrap_or_default())
            ),
            format!("repo={}", dev.repo_root),
            format!("worktree={}", dev.worktree_root),
            format!("status={} service status {}", command_example, meta.name),
            format!(
                "logs={} logs {} --all-streams --follow",
                command_example, meta.name
            ),
        ];
    }

    let mut lines = vec![paint(
        &format!("Dev service {}", meta.name),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Service",
        &[
            KeyValueRow::success("State", "running"),
            KeyValueRow::accent("Port", meta.gateway_port.unwrap_or_default().to_string()),
            KeyValueRow::plain(
                "URL",
                dev_gateway_url(meta.gateway_port.unwrap_or_default()),
            ),
            KeyValueRow::plain("Env", meta.name.clone()),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Source",
        &[
            KeyValueRow::plain("Repo", dev.repo_root.clone()),
            KeyValueRow::plain("Worktree", dev.worktree_root.clone()),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Next",
        &[
            KeyValueRow::plain(
                "Status",
                format!("{command_example} service status {}", meta.name),
            ),
            KeyValueRow::plain(
                "Logs",
                format!(
                    "{command_example} logs {} --all-streams --follow",
                    meta.name
                ),
            ),
            KeyValueRow::plain(
                "Stop",
                format!("{command_example} service stop {}", meta.name),
            ),
        ],
        profile.color,
    ));
    lines
}

fn render_dev_service_restored(
    meta: &EnvMeta,
    command_example: &str,
    profile: RenderProfile,
) -> Vec<String> {
    let Some(dev) = meta.dev.as_ref() else {
        return Vec::new();
    };

    if !profile.pretty {
        return vec![
            format!("service restored for {}", meta.name),
            format!("port={}", meta.gateway_port.unwrap_or_default()),
            format!(
                "url={}",
                dev_gateway_url(meta.gateway_port.unwrap_or_default())
            ),
            format!("repo={}", dev.repo_root),
            format!("worktree={}", dev.worktree_root),
            format!("status={} service status {}", command_example, meta.name),
            format!(
                "logs={} logs {} --all-streams --follow",
                command_example, meta.name
            ),
        ];
    }

    let mut lines = vec![paint(
        &format!("Dev service {}", meta.name),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Service",
        &[
            KeyValueRow::success("State", "restored"),
            KeyValueRow::accent("Port", meta.gateway_port.unwrap_or_default().to_string()),
            KeyValueRow::plain(
                "URL",
                dev_gateway_url(meta.gateway_port.unwrap_or_default()),
            ),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Next",
        &[
            KeyValueRow::plain(
                "Status",
                format!("{command_example} service status {}", meta.name),
            ),
            KeyValueRow::plain(
                "Logs",
                format!(
                    "{command_example} logs {} --all-streams --follow",
                    meta.name
                ),
            ),
        ],
        profile.color,
    ));
    lines
}

fn dev_gateway_url(port: u32) -> String {
    format!("http://127.0.0.1:{port}")
}

fn render_dev_run_step(title: &str, detail: String, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return vec![detail];
    }

    render_key_value_card(title, &[KeyValueRow::accent("Step", detail)], profile.color)
}

fn render_dev_status_list(summaries: &[DevStatusSummary], profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        let mut lines = Vec::new();
        for (index, summary) in summaries.iter().enumerate() {
            if index > 0 {
                lines.push(String::new());
            }
            lines.extend(render_dev_status(summary, profile));
        }
        return lines;
    }

    render_table(
        &["Env", "Port", "Repo", "Worktree", "Service"],
        &summaries
            .iter()
            .map(|summary| {
                vec![
                    Cell::accent(summary.env_name.clone()),
                    Cell::right(summary.gateway_port.to_string(), Tone::Accent),
                    Cell::plain(summary.repo_root.clone()),
                    Cell::plain(summary.worktree_root.clone()),
                    Cell::new(
                        if summary.service_running {
                            "running"
                        } else if summary.service_enabled {
                            "enabled"
                        } else {
                            "disabled"
                        },
                        crate::infra::terminal::Align::Left,
                        if summary.service_running {
                            Tone::Success
                        } else if summary.service_enabled {
                            Tone::Warning
                        } else {
                            Tone::Muted
                        },
                    ),
                ]
            })
            .collect::<Vec<_>>(),
        profile.color,
    )
}

#[cfg(test)]
mod tests {
    use super::{DevStatusSummary, RenderProfile, render_dev_status};

    fn sample_summary() -> DevStatusSummary {
        DevStatusSummary {
            env_name: "demo".to_string(),
            root: "/tmp/demo".to_string(),
            repo_root: "/repo/openclaw".to_string(),
            worktree_root: "/repo/openclaw/.worktrees/demo".to_string(),
            gateway_port: 18789,
            gateway_url: "http://127.0.0.1:18789".to_string(),
            config_path: "/tmp/demo/.openclaw/openclaw.json".to_string(),
            workspace_dir: "/tmp/demo/.openclaw/workspace".to_string(),
            service_enabled: true,
            service_running: true,
            logs_command: "ocm logs demo --all-streams --follow".to_string(),
            status_command: "ocm service status demo".to_string(),
        }
    }

    #[test]
    fn dev_status_pretty_stays_compact_when_healthy() {
        let lines = render_dev_status(&sample_summary(), RenderProfile::pretty(false));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("http://127.0.0.1:18789"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("ocm logs demo --all-streams --follow"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("/tmp/demo/.openclaw/openclaw.json"))
        );
        assert!(
            !lines
                .iter()
                .any(|line| line.contains("/tmp/demo/.openclaw/workspace"))
        );
        assert!(!lines.iter().any(|line| line.contains("Service enabled")));
    }
}
