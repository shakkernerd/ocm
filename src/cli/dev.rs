use std::path::{Path, PathBuf};

use serde::Serialize;

use super::Cli;
use crate::env::{CreateEnvironmentOptions, EnvDevMeta, EnvMeta};
use crate::infra::process::run_direct;
use crate::infra::shell::build_openclaw_env;
use crate::openclaw_repo::{
    detect_openclaw_checkout, discover_openclaw_checkout, ensure_openclaw_worktree,
};
use crate::store::{
    derive_env_paths, display_path, ensure_minimum_local_openclaw_config, resolve_absolute_path,
    validate_name,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DevStatusSummary {
    env_name: String,
    root: String,
    repo_root: String,
    worktree_root: String,
    gateway_port: u32,
    config_path: String,
    workspace_dir: String,
    service_enabled: bool,
    service_running: bool,
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

        for (index, summary) in summaries.iter().enumerate() {
            if index > 0 {
                self.stdout_line("");
            }
            self.stdout_lines(render_dev_status(summary, profile));
        }
        Ok(0)
    }

    fn handle_dev_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, watch) = Self::consume_flag(args, "--watch");
        let (args, onboard) = Self::consume_flag(args, "--onboard");
        let (args, repo_root) = Self::consume_option(args, "--repo")?;
        let repo_root = Self::require_option_value(repo_root, "--repo")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--port")?),
            None => None,
        };
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;
        let name = validate_name(name, "Environment name")?;

        let (meta, created) = self.ensure_dev_env(&name, repo_root, gateway_port)?;
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;

        self.stderr_line(format!(
            "{} dev env {} on port {} from {}",
            if created { "Prepared" } else { "Using" },
            meta.name,
            meta.gateway_port.unwrap_or_default(),
            dev.worktree_root,
        ));

        let install_code = self.ensure_dev_dependencies(&meta)?;
        if install_code != 0 {
            return Ok(install_code);
        }

        if onboard {
            let code = self.run_dev_onboard(&meta)?;
            if code != 0 {
                return Ok(code);
            }
        }

        if watch {
            self.stderr_line(format!("Watching {}", dev.worktree_root));
            return self.run_dev_gateway_watch(&meta);
        }

        self.stderr_line(format!("Starting gateway for {}", meta.name));
        self.run_dev_gateway(&meta)
    }

    fn ensure_dev_env(
        &self,
        name: &str,
        repo_root: Option<String>,
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

            ensure_openclaw_worktree(&existing_repo, &existing.name)?;
            let meta = self
                .environment_service()
                .apply_effective_gateway_port(existing)?;
            self.bootstrap_dev_env(&meta)?;
            return Ok((meta, false));
        }

        let repo_root = match repo_root {
            Some(repo_root) => resolve_absolute_path(&repo_root, &self.env, &self.cwd)?,
            None => discover_openclaw_checkout(&self.cwd).ok_or_else(|| {
                "could not find an OpenClaw checkout; pass --repo /path/to/openclaw".to_string()
            })?,
        };
        let repo_root = detect_openclaw_checkout(&repo_root).ok_or_else(|| {
            format!(
                "OpenClaw checkout not found at {}",
                display_path(&repo_root)
            )
        })?;
        let worktree_root = ensure_openclaw_worktree(&repo_root, name)?;

        let created = self.environment_service().create(CreateEnvironmentOptions {
            name: name.to_string(),
            root: None,
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

        self.stderr_line(format!("Installing dependencies in {}", dev.worktree_root));
        run_direct(
            "pnpm",
            &["install".to_string()],
            &build_openclaw_env(meta, &self.env),
            worktree_root,
        )
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
        run_direct(
            "node",
            &args,
            &build_openclaw_env(meta, &self.env),
            Path::new(&dev.worktree_root),
        )
    }

    fn build_dev_status_summary(&self, meta: EnvMeta) -> Result<Option<DevStatusSummary>, String> {
        let Some(dev) = meta.dev.as_ref() else {
            return Ok(None);
        };
        let (gateway_port, _) = self
            .environment_service()
            .resolve_effective_gateway_port(&meta)?;
        let paths = derive_env_paths(Path::new(&meta.root));
        Ok(Some(DevStatusSummary {
            env_name: meta.name,
            root: meta.root,
            repo_root: dev.repo_root.clone(),
            worktree_root: dev.worktree_root.clone(),
            gateway_port,
            config_path: display_path(&paths.config_path),
            workspace_dir: display_path(&paths.workspace_dir),
            service_enabled: meta.service_enabled,
            service_running: meta.service_running,
        }))
    }
}

fn render_dev_status(
    summary: &DevStatusSummary,
    profile: super::render::RenderProfile,
) -> Vec<String> {
    if profile.pretty {
        vec![
            format!("Dev env {}", summary.env_name),
            format!("  port: {}", summary.gateway_port),
            format!("  repo: {}", summary.repo_root),
            format!("  worktree: {}", summary.worktree_root),
            format!("  root: {}", summary.root),
            format!("  config: {}", summary.config_path),
            format!("  workspace: {}", summary.workspace_dir),
        ]
    } else {
        vec![
            format!("env={}", summary.env_name),
            format!("port={}", summary.gateway_port),
            format!("repo={}", summary.repo_root),
            format!("worktree={}", summary.worktree_root),
            format!("root={}", summary.root),
            format!("config={}", summary.config_path),
            format!("workspace={}", summary.workspace_dir),
        ]
    }
}
