use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Cli;
use super::render::RenderProfile;
use crate::env::{
    CreateEnvironmentOptions, CreateSourceWatchOverrideOptions, EnvDevMeta, EnvMeta,
    SourceWatchLease,
};
use crate::infra::process::run_direct;
use crate::infra::shell::{build_openclaw_dev_source_env, build_openclaw_env};
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

        if let Some(existing) = self.environment_service().find(&name)?
            && existing.dev.is_none()
        {
            return self.handle_existing_env_source_watch(
                existing,
                repo_root,
                root,
                gateway_port,
                watch,
                force,
                onboard,
            );
        }

        let (meta, created) = self.ensure_dev_env(&name, repo_root, root, gateway_port)?;
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let stderr_profile = self.dev_stderr_profile();
        let watch_takes_over_service = watch && force && meta.service_running;
        if !service_requested && meta.service_running && !watch_takes_over_service {
            return Err(format!(
                "dev env {} is already running in the background; stop it first with {} service stop {}, inspect it with {} logs {} --follow, or rerun with --watch --force to take it over temporarily",
                meta.name,
                self.command_example(),
                meta.name,
                self.command_example(),
                meta.name
            ));
        }
        let source_watch_lease = if watch {
            Some(
                self.environment_service()
                    .acquire_source_watch_lease(&meta.name)?,
            )
        } else {
            None
        };
        self.stderr_lines(render_dev_run_summary(
            &meta,
            created,
            service_requested,
            watch,
            onboard,
            stderr_profile,
        ));
        self.stderr_lines(render_dev_external_plugin_warnings(
            &meta,
            Path::new(&dev.worktree_root),
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
                self.stop_service_for_source_watch(&meta.name)?;
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
            let watch_result = self.run_dev_gateway_watch(
                &meta,
                source_watch_lease
                    .as_ref()
                    .ok_or_else(|| "source watch lease is missing".to_string())?,
            );
            let restore_result = if watch_takes_over_service {
                self.stderr_lines(render_dev_run_step(
                    "Restore",
                    format!("Starting background service for {}", meta.name),
                    stderr_profile,
                ));
                self.service_service().start(&meta.name).map(|_| {
                    self.stdout_lines(render_dev_service_restored(
                        &meta,
                        &self.command_example(),
                        self.dev_stdout_profile(),
                    ));
                })
            } else {
                Ok(())
            };
            return combine_watch_and_restore_results(watch_result, restore_result, &meta.name);
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

    fn handle_existing_env_source_watch(
        &self,
        existing: EnvMeta,
        repo_root: Option<String>,
        root: Option<String>,
        gateway_port: Option<u32>,
        watch: bool,
        force: bool,
        onboard: bool,
    ) -> Result<i32, String> {
        if !watch || !force {
            return Err(format!(
                "environment \"{}\" is not a dev env; use a new env name for `ocm dev`, or rerun with --repo <path> --watch --force to take it over temporarily",
                existing.name
            ));
        }
        if onboard {
            return Err(
                "dev takeover cannot combine --onboard with an existing non-dev env".to_string(),
            );
        }
        if root.is_some() {
            return Err("dev takeover uses the existing env root; remove --root".to_string());
        }
        if gateway_port.is_some() {
            return Err(
                "dev takeover uses the existing env gateway port; remove --port".to_string(),
            );
        }
        let Some(repo_root) = repo_root else {
            return Err(
                "dev takeover of an existing non-dev env requires --repo <path>".to_string(),
            );
        };
        let repo_root = resolve_absolute_path(&repo_root, &self.env, &self.cwd)?;
        let repo_root = detect_openclaw_checkout(&repo_root).ok_or_else(|| {
            format!(
                "OpenClaw checkout not found at {}",
                display_path(&repo_root)
            )
        })?;
        let meta = self
            .environment_service()
            .apply_effective_gateway_port(existing)?;
        let source_watch_lease = self
            .environment_service()
            .acquire_source_watch_lease(&meta.name)?;
        let stderr_profile = self.dev_stderr_profile();
        self.stderr_lines(render_source_watch_takeover_summary(
            &meta,
            &repo_root,
            stderr_profile,
        ));
        self.stderr_lines(render_dev_external_plugin_warnings(
            &meta,
            &repo_root,
            stderr_profile,
        ));

        let restore_service = meta.service_running;
        if restore_service {
            self.stderr_lines(render_dev_run_step(
                "Takeover",
                format!(
                    "Stopping background service for {} while source watch takes over; OCM will restore it when watch exits",
                    meta.name
                ),
                stderr_profile,
            ));
            self.stop_service_for_source_watch(&meta.name)?;
        }

        self.stderr_lines(render_dev_run_step(
            "Watch",
            format!(
                "Watching {} on port {} for env {}",
                display_path(&repo_root),
                meta.gateway_port.unwrap_or_default(),
                meta.name
            ),
            stderr_profile,
        ));
        let watch_result =
            self.run_source_gateway_watch(&meta, &repo_root, true, &source_watch_lease);

        let restore_result = if restore_service {
            self.stderr_lines(render_dev_run_step(
                "Restore",
                format!("Starting background service for {}", meta.name),
                stderr_profile,
            ));
            self.service_service().start(&meta.name).map(|_| {
                self.stdout_lines(render_source_watch_service_restored(
                    &meta,
                    &repo_root,
                    &self.command_example(),
                    self.dev_stdout_profile(),
                ));
            })
        } else {
            Ok(())
        };

        combine_watch_and_restore_results(watch_result, restore_result, &meta.name)
    }

    fn stop_service_for_source_watch(&self, env_name: &str) -> Result<(), String> {
        let stop_result = self.service_service().stop(env_name);
        let stop_error = match stop_result {
            Ok(summary) if !summary.running => return Ok(()),
            Ok(summary) => source_watch_stop_timeout_error(&summary),
            Err(error) => format!("failed stopping background service for {env_name}: {error}"),
        };

        match self.service_service().start(env_name) {
            Ok(_) => Err(format!(
                "{stop_error}; restored the background service policy and did not start source watch"
            )),
            Err(restore_error) => Err(format!(
                "{stop_error}; also failed restoring the background service policy: {restore_error}"
            )),
        }
    }

    fn ensure_dev_env(
        &self,
        name: &str,
        repo_root: Option<String>,
        root: Option<String>,
        gateway_port: Option<u32>,
    ) -> Result<(EnvMeta, bool), String> {
        if let Some(existing) = self.environment_service().find(name)? {
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
            if let Some(requested_port) = gateway_port {
                let current_port = meta.gateway_port.unwrap_or_default();
                if requested_port != current_port {
                    return Err(format!(
                        "dev cannot change the port for existing env {}; current port is {}",
                        meta.name, current_port
                    ));
                }
            }
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
            &build_openclaw_dev_source_env(meta, &self.env, Path::new(&dev.worktree_root)),
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
            &build_openclaw_dev_source_env(meta, &self.env, Path::new(&dev.worktree_root)),
            Path::new(&dev.worktree_root),
        )
    }

    fn run_dev_gateway_watch(
        &self,
        meta: &EnvMeta,
        source_watch_lease: &SourceWatchLease,
    ) -> Result<i32, String> {
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        self.run_source_gateway_watch(
            meta,
            Path::new(&dev.worktree_root),
            false,
            source_watch_lease,
        )
    }

    fn run_source_gateway_watch(
        &self,
        meta: &EnvMeta,
        repo_root: &Path,
        tee_to_env_logs: bool,
        _source_watch_lease: &SourceWatchLease,
    ) -> Result<i32, String> {
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

        let mut command = Command::new("node");
        command
            .args(&args)
            .stdin(Stdio::inherit())
            .env_clear()
            .envs(build_openclaw_dev_source_env(meta, &self.env, repo_root))
            .current_dir(repo_root);

        let mut log_files = if tee_to_env_logs {
            Some(open_source_watch_log_files(meta)?)
        } else {
            None
        };

        if log_files.is_some() {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
        } else {
            command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
        }

        let mut child = command
            .spawn()
            .map_err(|error| format!("failed to run \"node\": {error}"))?;
        let source_watch = match self.environment_service().create_source_watch_override(
            CreateSourceWatchOverrideOptions {
                env_name: meta.name.clone(),
                repo_root: repo_root.to_path_buf(),
                watch_pid: child.id(),
            },
        ) {
            Ok(source_watch) => source_watch,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let mut tee_threads = Vec::new();
        if let Some(log_files) = log_files.take() {
            let Some(stdout) = child.stdout.take() else {
                let _ = child.kill();
                let _ = child.wait();
                let _ = self
                    .environment_service()
                    .clear_source_watch_override(&meta.name, &source_watch.token);
                return Err("failed to capture source watch stdout".to_string());
            };
            let Some(stderr) = child.stderr.take() else {
                let _ = child.kill();
                let _ = child.wait();
                let _ = self
                    .environment_service()
                    .clear_source_watch_override(&meta.name, &source_watch.token);
                return Err("failed to capture source watch stderr".to_string());
            };
            tee_threads.push(spawn_tee_thread(
                stdout,
                io::stdout(),
                log_files.stdout,
                "stdout",
            ));
            tee_threads.push(spawn_tee_thread(
                stderr,
                io::stderr(),
                log_files.stderr,
                "stderr",
            ));
        }

        let status_result =
            wait_for_source_watch_child(&mut child, &stop_requested).map_err(|error| {
                let _ = child.kill();
                let _ = child.wait();
                error
            });
        let tee_result = wait_for_tee_threads(tee_threads);
        let clear_result = self
            .environment_service()
            .clear_source_watch_override(&meta.name, &source_watch.token)
            .map(|_| ());

        let status = combine_source_watch_cleanup_results(status_result, tee_result, clear_result)?;
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
            logs_command: format!("{} logs {} --follow", self.command_example(), env_name),
            status_command: format!("{} service status {}", self.command_example(), env_name),
        }))
    }
}

fn source_watch_stop_timeout_error(summary: &crate::service::ServiceActionSummary) -> String {
    let warnings = if summary.warnings.is_empty() {
        String::new()
    } else {
        format!(" ({})", summary.warnings.join("; "))
    };
    format!(
        "background service for {} is still running after the stop request{warnings}",
        summary.env_name
    )
}

fn combine_watch_and_restore_results(
    watch_result: Result<i32, String>,
    restore_result: Result<(), String>,
    env_name: &str,
) -> Result<i32, String> {
    match (watch_result, restore_result) {
        (Ok(code), Ok(())) => Ok(code),
        (Err(watch_error), Ok(())) => Err(watch_error),
        (Ok(_), Err(restore_error)) => Err(format!(
            "source watch ended, but failed restoring background service for {env_name}: {restore_error}"
        )),
        (Err(watch_error), Err(restore_error)) => Err(format!(
            "{watch_error}; also failed restoring background service for {env_name}: {restore_error}"
        )),
    }
}

fn combine_source_watch_cleanup_results(
    status_result: Result<std::process::ExitStatus, String>,
    tee_result: Result<(), String>,
    clear_result: Result<(), String>,
) -> Result<std::process::ExitStatus, String> {
    let mut errors = Vec::new();
    let status = match status_result {
        Ok(status) => Some(status),
        Err(error) => {
            errors.push(error);
            None
        }
    };
    if let Err(error) = tee_result {
        errors.push(error);
    }
    if let Err(error) = clear_result {
        errors.push(error);
    }
    if errors.is_empty() {
        status.ok_or_else(|| "source watch ended without an exit status".to_string())
    } else {
        Err(errors.join("; "))
    }
}

fn wait_for_source_watch_child(
    child: &mut std::process::Child,
    stop_requested: &AtomicBool,
) -> Result<std::process::ExitStatus, String> {
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("failed waiting for source watch: {error}"))?
        {
            return Ok(status);
        }
        if stop_requested.load(Ordering::SeqCst) {
            child
                .kill()
                .map_err(|error| format!("failed stopping source watch: {error}"))?;
            return child
                .wait()
                .map_err(|error| format!("failed waiting for stopped source watch: {error}"));
        }
        thread::sleep(Duration::from_millis(50));
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
            format!("logs={} logs {} --follow", command_example, meta.name),
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
                format!("{command_example} logs {} --follow", meta.name),
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
            format!("logs={} logs {} --follow", command_example, meta.name),
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
                format!("{command_example} logs {} --follow", meta.name),
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

fn render_dev_external_plugin_warnings(
    meta: &EnvMeta,
    source_root: &Path,
    profile: RenderProfile,
) -> Vec<String> {
    let external_plugins = collect_external_installed_plugin_ids(meta, source_root);
    external_plugins
        .into_iter()
        .flat_map(|plugin_id| {
            render_dev_run_step(
                "Warning",
                format!(
                    "Installed plugin \"{plugin_id}\" is not present in {}; dev mode will keep using the env-installed plugin for that id",
                    display_path(&source_root.join("extensions"))
                ),
                profile,
            )
        })
        .collect()
}

fn render_source_watch_takeover_summary(
    meta: &EnvMeta,
    repo_root: &Path,
    profile: RenderProfile,
) -> Vec<String> {
    let log_paths = source_watch_log_paths(meta);
    if !profile.pretty {
        return vec![
            format!("taking over env {}", meta.name),
            "binding=unchanged".to_string(),
            format!("port={}", meta.gateway_port.unwrap_or_default()),
            format!("root={}", meta.root),
            format!("repo={}", display_path(repo_root)),
            format!("stdoutLog={}", display_path(&log_paths.stdout)),
            format!("stderrLog={}", display_path(&log_paths.stderr)),
            "mode=watch".to_string(),
        ];
    }

    let mut lines = vec![paint(
        &format!("Source watch {}", meta.name),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Environment",
        &[
            KeyValueRow::accent("Port", meta.gateway_port.unwrap_or_default().to_string()),
            KeyValueRow::plain("Root", meta.root.clone()),
            KeyValueRow::plain("Binding", "unchanged"),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Source",
        &[KeyValueRow::plain("Repo", display_path(repo_root))],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Logs",
        &[
            KeyValueRow::plain("Stdout", display_path(&log_paths.stdout)),
            KeyValueRow::plain("Stderr", display_path(&log_paths.stderr)),
        ],
        profile.color,
    ));
    lines
}

fn collect_external_installed_plugin_ids(meta: &EnvMeta, source_root: &Path) -> BTreeSet<String> {
    let source_ids = collect_source_plugin_ids(source_root);
    collect_installed_plugin_ids(meta)
        .into_iter()
        .filter(|plugin_id| !source_ids.contains(plugin_id))
        .collect()
}

fn collect_source_plugin_ids(source_root: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let extensions_dir = source_root.join("extensions");
    let Ok(entries) = fs::read_dir(extensions_dir) else {
        return ids;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let plugin_dir = entry.path();
        if let Some(id) =
            read_json_string_at_path(&plugin_dir.join("openclaw.plugin.json"), &["id"])
        {
            ids.insert(id);
            continue;
        }
        if let Some(id) =
            read_json_string_at_path(&plugin_dir.join("package.json"), &["openclaw", "id"])
        {
            ids.insert(id);
        }
    }
    ids
}

fn collect_installed_plugin_ids(meta: &EnvMeta) -> BTreeSet<String> {
    let paths = derive_env_paths(Path::new(&meta.root));
    let mut ids = collect_installed_plugin_ids_from_json_file(&paths.config_path);
    ids.extend(collect_installed_plugin_ids_from_json_file(
        &paths.state_dir.join("plugins/installs.json"),
    ));
    ids
}

fn collect_installed_plugin_ids_from_json_file(path: &Path) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let Ok(raw) = fs::read_to_string(path) else {
        return ids;
    };
    let Ok(value) = serde_json::from_str::<Value>(&raw) else {
        return ids;
    };
    collect_installed_plugin_ids_from_value(&value, &mut ids);
    ids
}

fn collect_installed_plugin_ids_from_value(value: &Value, ids: &mut BTreeSet<String>) {
    if let Some(installs) = value
        .pointer("/plugins/installs")
        .and_then(Value::as_object)
    {
        ids.extend(
            installs
                .keys()
                .filter(|key| !key.trim().is_empty())
                .cloned(),
        );
    }
    if let Some(install_records) = value.get("installRecords").and_then(Value::as_object) {
        ids.extend(
            install_records
                .keys()
                .filter(|key| !key.trim().is_empty())
                .cloned(),
        );
    }
    if let Some(plugins) = value.get("plugins").and_then(Value::as_array) {
        ids.extend(
            plugins
                .iter()
                .filter_map(|plugin| plugin.get("pluginId").and_then(Value::as_str))
                .filter(|plugin_id| !plugin_id.trim().is_empty())
                .map(ToOwned::to_owned),
        );
    }
}

fn read_json_string_at_path(path: &Path, keys: &[&str]) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    let mut current = &value;
    for key in keys {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn render_source_watch_service_restored(
    meta: &EnvMeta,
    repo_root: &Path,
    command_example: &str,
    profile: RenderProfile,
) -> Vec<String> {
    if !profile.pretty {
        return vec![
            format!("service restored for {}", meta.name),
            format!("port={}", meta.gateway_port.unwrap_or_default()),
            format!(
                "url={}",
                dev_gateway_url(meta.gateway_port.unwrap_or_default())
            ),
            format!("repo={}", display_path(repo_root)),
            "binding=unchanged".to_string(),
            format!("status={} service status {}", command_example, meta.name),
            format!("logs={} logs {} --follow", command_example, meta.name),
        ];
    }

    let mut lines = vec![paint(
        &format!("Env service {}", meta.name),
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
            KeyValueRow::plain("Binding", "unchanged"),
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
                format!("{command_example} logs {} --follow", meta.name),
            ),
        ],
        profile.color,
    ));
    lines
}

struct SourceWatchLogPaths {
    stdout: PathBuf,
    stderr: PathBuf,
}

struct SourceWatchLogFiles {
    stdout: File,
    stderr: File,
}

fn source_watch_log_paths(meta: &EnvMeta) -> SourceWatchLogPaths {
    let env_paths = derive_env_paths(Path::new(&meta.root));
    let logs_dir = env_paths.state_dir.join("logs");
    SourceWatchLogPaths {
        stdout: logs_dir.join("gateway.log"),
        stderr: logs_dir.join("gateway.err.log"),
    }
}

fn open_source_watch_log_files(meta: &EnvMeta) -> Result<SourceWatchLogFiles, String> {
    let paths = source_watch_log_paths(meta);
    if let Some(parent) = paths.stdout.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            format!(
                "failed creating env log directory for {}: {error}",
                meta.name
            )
        })?;
    }
    Ok(SourceWatchLogFiles {
        stdout: open_append_log(&paths.stdout, &meta.name, "stdout")?,
        stderr: open_append_log(&paths.stderr, &meta.name, "stderr")?,
    })
}

fn open_append_log(path: &Path, env_name: &str, stream: &str) -> Result<File, String> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            format!(
                "failed opening {stream} log for env \"{env_name}\": {}: {error}",
                display_path(path)
            )
        })
}

fn spawn_tee_thread<R, W>(
    input: R,
    terminal: W,
    log_file: File,
    stream: &'static str,
) -> JoinHandle<Result<(), String>>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
{
    thread::spawn(move || {
        tee_stream(input, terminal, log_file)
            .map_err(|error| format!("failed writing source watch {stream} log: {error}"))
    })
}

fn tee_stream<R, W>(mut input: R, mut terminal: W, mut log_file: File) -> io::Result<()>
where
    R: Read,
    W: Write,
{
    let mut buffer = [0_u8; 8 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let chunk = &buffer[..count];
        terminal.write_all(chunk)?;
        terminal.flush()?;
        log_file.write_all(chunk)?;
        log_file.flush()?;
    }
    Ok(())
}

fn wait_for_tee_threads(threads: Vec<JoinHandle<Result<(), String>>>) -> Result<(), String> {
    for thread in threads {
        let result = thread
            .join()
            .map_err(|_| "source watch log tee thread panicked".to_string())?;
        result?;
    }
    Ok(())
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
    use super::{
        DevStatusSummary, RenderProfile, combine_watch_and_restore_results, render_dev_status,
        source_watch_stop_timeout_error,
    };
    use crate::service::ServiceActionSummary;

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
            logs_command: "ocm logs demo --follow".to_string(),
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
                .any(|line| line.contains("ocm logs demo --follow"))
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

    #[test]
    fn source_watch_stop_timeout_preserves_service_diagnostics() {
        let summary = ServiceActionSummary {
            env_name: "demo".to_string(),
            service_kind: "supervisor".to_string(),
            action: "stop".to_string(),
            installed: true,
            loaded: true,
            running: true,
            desired_running: false,
            gateway_port: 18789,
            binding_kind: Some("runtime".to_string()),
            binding_name: Some("stable".to_string()),
            stdout_path: None,
            stderr_path: None,
            warnings: vec!["gateway is still shutting down".to_string()],
        };

        assert_eq!(
            source_watch_stop_timeout_error(&summary),
            "background service for demo is still running after the stop request (gateway is still shutting down)"
        );
    }

    #[test]
    fn source_watch_reports_watch_and_restore_failures_together() {
        let result = combine_watch_and_restore_results(
            Err("watch failed".to_string()),
            Err("restore failed".to_string()),
            "demo",
        );

        assert_eq!(
            result,
            Err(
                "watch failed; also failed restoring background service for demo: restore failed"
                    .to_string()
            )
        );
    }
}
