use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};

use super::{Cli, render};
use crate::env::{
    CloneEnvironmentOptions, CreateEnvSnapshotOptions, EnvDevMeta, RestoreEnvSnapshotOptions,
};
use crate::infra::shell::build_openclaw_env;
use crate::openclaw_repo::{detect_openclaw_checkout, ensure_openclaw_worktree};
use crate::runtime::releases::{
    OpenClawRelease, is_official_openclaw_releases_url, normalize_openclaw_channel_selector,
    official_openclaw_releases_url,
};
use crate::runtime::{
    InstallRuntimeFromOfficialReleaseOptions, OfficialRuntimePrepareAction, RuntimeMeta,
    RuntimeReleaseSelectorKind, RuntimeService,
};
use crate::service::ServiceSummary;
use crate::store::{
    InstallContext, RuntimeReleaseDetails, clean_path, copy_dir_recursive, derive_env_paths,
    display_path, ensure_minimum_local_openclaw_config, ensure_store, get_runtime,
    install_runtime_from_selected_official_openclaw_release, remove_runtime, resolve_absolute_path,
    runtime_install_root, runtime_integrity_issue, runtime_meta_path, save_environment, write_json,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeEnvSummary {
    pub env_name: String,
    pub previous_binding_kind: String,
    pub previous_binding_name: String,
    pub binding_kind: String,
    pub binding_name: String,
    pub outcome: String,
    pub runtime_release_version: Option<String>,
    pub runtime_release_channel: Option<String>,
    pub service_action: Option<String>,
    pub snapshot_id: Option<String>,
    pub rollback: Option<String>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeBatchSummary {
    pub count: usize,
    pub changed: usize,
    pub current: usize,
    pub skipped: usize,
    pub restarted: usize,
    pub failed: usize,
    pub results: Vec<UpgradeEnvSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeSimulationCheck {
    pub name: String,
    pub status: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeSimulationSummary {
    pub scenario: String,
    pub source_env: String,
    pub simulation_env: String,
    pub from_binding_kind: String,
    pub from_binding_name: String,
    pub to_binding_kind: String,
    pub to_binding_name: String,
    pub to: String,
    pub outcome: String,
    pub checks: Vec<UpgradeSimulationCheck>,
    pub cleanup_command: String,
    pub cleanup: String,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeSimulationBatchSummary {
    pub source_env: String,
    pub to: String,
    pub count: usize,
    pub passed: usize,
    pub failed: usize,
    pub results: Vec<UpgradeSimulationSummary>,
}

#[derive(Clone, Debug)]
struct UpgradeTarget {
    version: Option<String>,
    channel: Option<String>,
    runtime: Option<String>,
}

#[derive(Clone, Debug)]
enum UpgradeSimulationTarget {
    Official {
        target: UpgradeTarget,
        display: String,
    },
    LocalRepo {
        repo_root: PathBuf,
        display: String,
    },
}

#[derive(Clone, Copy, Debug)]
enum UpgradeSimulationScenario {
    Current,
    Minimum,
    Telegram,
}

#[derive(Clone, Copy, Debug)]
struct UpgradeOptions {
    dry_run: bool,
    rollback_enabled: bool,
}

#[derive(Clone, Copy, Debug)]
struct UpgradeSimulationOptions {
    keep_envs: bool,
}

#[derive(Clone, Debug)]
struct PreparedSimulationRuntime {
    name: String,
    note: String,
    temporary: bool,
}

impl UpgradeTarget {
    fn parse(args: Vec<String>) -> Result<(Vec<String>, Self), String> {
        let (args, version) = Cli::consume_option(args, "--version")?;
        let version = Cli::require_option_value(version, "--version")?;
        let (args, channel) = Cli::consume_option(args, "--channel")?;
        let channel = Cli::require_option_value(channel, "--channel")?;
        let (args, runtime) = Cli::consume_option(args, "--runtime")?;
        let runtime = Cli::require_option_value(runtime, "--runtime")?;
        let explicit_count = usize::from(version.is_some())
            + usize::from(channel.is_some())
            + usize::from(runtime.is_some());
        if explicit_count > 1 {
            return Err(
                "upgrade accepts only one of --version, --channel, or --runtime".to_string(),
            );
        }
        Ok((
            args,
            Self {
                version,
                channel,
                runtime,
            },
        ))
    }

    fn is_explicit(&self) -> bool {
        self.version.is_some() || self.channel.is_some() || self.runtime.is_some()
    }

    fn canonical_runtime_name(&self) -> Result<String, String> {
        if let Some(runtime) = self.runtime.as_deref() {
            return Ok(runtime.to_string());
        }
        RuntimeService::canonical_official_openclaw_runtime_name(
            self.version.as_deref(),
            self.channel.as_deref(),
        )
    }

    fn release_channel_hint(&self) -> Option<String> {
        self.channel.clone()
    }

    fn is_named_runtime(&self) -> bool {
        self.runtime.is_some()
    }
}

impl Cli {
    pub(super) fn handle_upgrade_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "upgrade")?;
        if matches!(args.first().map(String::as_str), Some("simulate")) {
            let summaries = self.upgrade_simulate(args[1..].to_vec())?;
            let failed = summaries.iter().any(|summary| summary.outcome == "failed");
            if json_flag {
                if summaries.len() == 1 {
                    self.print_json(&summaries[0])?;
                } else {
                    self.print_json(&build_simulation_batch_summary(summaries))?;
                }
                return Ok(if failed { 1 } else { 0 });
            }
            if summaries.len() == 1 {
                self.stdout_lines(render::upgrade::upgrade_simulation(
                    &summaries[0],
                    profile,
                    &self.command_example(),
                ));
            } else {
                self.stdout_lines(render::upgrade::upgrade_simulation_batch(
                    &build_simulation_batch_summary(summaries),
                    profile,
                    &self.command_example(),
                ));
            }
            return Ok(if failed { 1 } else { 0 });
        }

        let (args, dry_run) = Self::consume_flag(args, "--dry-run");
        let (args, no_rollback) = Self::consume_flag(args, "--no-rollback");
        let (args, all_flag) = Self::consume_flag(args, "--all");
        let (args, target) = UpgradeTarget::parse(args)?;
        let options = UpgradeOptions {
            dry_run,
            rollback_enabled: !no_rollback,
        };

        if all_flag {
            Self::assert_no_extra_args(&args)?;
            if target.is_explicit() {
                return Err(
                    "upgrade --all does not accept --version, --channel, or --runtime; upgrade one env at a time when changing selectors"
                        .to_string(),
                );
            }

            let envs = self.environment_service().list()?;
            let mut results = Vec::with_capacity(envs.len());
            for env in envs {
                match self.upgrade_env(&env.name, &target, options) {
                    Ok(summary) => results.push(summary),
                    Err(error) => results.push(UpgradeEnvSummary {
                        env_name: env.name,
                        previous_binding_kind: "unknown".to_string(),
                        previous_binding_name: "—".to_string(),
                        binding_kind: "unknown".to_string(),
                        binding_name: "—".to_string(),
                        outcome: "failed".to_string(),
                        runtime_release_version: None,
                        runtime_release_channel: None,
                        service_action: None,
                        snapshot_id: None,
                        rollback: None,
                        note: Some(error),
                    }),
                }
            }

            let summary = UpgradeBatchSummary {
                count: results.len(),
                changed: results
                    .iter()
                    .filter(|summary| is_changed_upgrade_outcome(&summary.outcome))
                    .count(),
                current: results
                    .iter()
                    .filter(|summary| summary.outcome == "up-to-date")
                    .count(),
                skipped: results
                    .iter()
                    .filter(|summary| {
                        matches!(
                            summary.outcome.as_str(),
                            "pinned" | "local-command" | "manual-runtime"
                        )
                    })
                    .count(),
                restarted: results
                    .iter()
                    .filter(|summary| summary.service_action.is_some())
                    .count(),
                failed: results
                    .iter()
                    .filter(|summary| is_failed_upgrade_outcome(&summary.outcome))
                    .count(),
                results,
            };

            if json_flag {
                self.print_json(&summary)?;
                return Ok(if summary.failed == 0 { 0 } else { 1 });
            }

            self.stdout_lines(render::upgrade::upgrade_batch(
                &summary,
                profile,
                &self.command_example(),
            ));
            return Ok(if summary.failed == 0 { 0 } else { 1 });
        }

        let Some(name) = args.first() else {
            return Err("upgrade requires <env> or --all".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.upgrade_env(name, &target, options)?;
        let failed = is_failed_upgrade_outcome(&summary.outcome);
        if json_flag {
            self.print_json(&summary)?;
            return Ok(if failed { 1 } else { 0 });
        }

        self.stdout_lines(render::upgrade::upgrade_env(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(if failed { 1 } else { 0 })
    }

    fn upgrade_simulate(&self, args: Vec<String>) -> Result<Vec<UpgradeSimulationSummary>, String> {
        let (args, keep_simulations) = Self::consume_flag(args, "--keep-simulations");
        let (args, keep_simulation) = Self::consume_flag(args, "--keep-simulation");
        let options = UpgradeSimulationOptions {
            keep_envs: keep_simulations || keep_simulation,
        };
        let (args, to) = Self::consume_option(args, "--to")?;
        let to = Self::require_option_value(to, "--to")?.ok_or_else(|| {
            "upgrade simulate requires --to <version|channel|repo-path>".to_string()
        })?;
        let (args, scenario) = Self::consume_option(args, "--scenario")?;
        let scenario = Self::require_option_value(scenario, "--scenario")?;
        let scenarios = UpgradeSimulationScenario::parse_many(scenario.as_deref())?;
        let Some(source_name) = args.first() else {
            return Err("upgrade simulate requires an environment name".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;
        self.environment_service().get(source_name)?;

        let target = self.resolve_simulation_target(&to)?;
        self.validate_simulation_target(&target)?;
        let prepared_runtime = self.prepare_shared_simulation_runtime(source_name, &target)?;
        let mut summaries = Vec::with_capacity(scenarios.len());
        for scenario in scenarios {
            match self.upgrade_simulate_one(
                source_name,
                &target,
                prepared_runtime.as_ref(),
                scenario,
                options,
            ) {
                Ok(summary) => summaries.push(summary),
                Err(error) => {
                    self.finish_shared_simulation_runtime(
                        &mut summaries,
                        prepared_runtime.as_ref(),
                        options,
                    )?;
                    return Err(error);
                }
            }
        }
        self.finish_shared_simulation_runtime(&mut summaries, prepared_runtime.as_ref(), options)?;
        Ok(summaries)
    }

    fn upgrade_simulate_one(
        &self,
        source_name: &str,
        target: &UpgradeSimulationTarget,
        prepared_runtime: Option<&PreparedSimulationRuntime>,
        scenario: UpgradeSimulationScenario,
        options: UpgradeSimulationOptions,
    ) -> Result<UpgradeSimulationSummary, String> {
        let source = self.environment_service().get(source_name)?;
        let (from_binding_kind, from_binding_name) = source_binding(&source);
        let simulation_name = simulation_env_name(source_name, scenario.id());
        let cloned = self.environment_service().clone(CloneEnvironmentOptions {
            source_name: source_name.to_string(),
            name: simulation_name.clone(),
            root: None,
        })?;
        if let Err(error) =
            self.environment_service()
                .set_service_policy(&cloned.name, Some(false), Some(false))
        {
            let _ = self.environment_service().remove(&cloned.name, true);
            return Err(error);
        }
        if cloned.protected {
            if let Err(error) = self
                .environment_service()
                .set_protected(&cloned.name, false)
            {
                let _ = self.environment_service().remove(&cloned.name, true);
                return Err(error);
            }
        }

        let mut checks = vec![UpgradeSimulationCheck::passed(
            "clone env",
            format!("created isolated env {}", cloned.name),
        )];
        let mut to_binding_kind = "unknown".to_string();
        let mut to_binding_name = "unknown".to_string();

        let scenario_check = self.apply_simulation_scenario(&cloned.name, scenario);
        let scenario_failed = scenario_check.status == "failed";
        checks.push(scenario_check);
        if scenario_failed {
            let summary = self.build_simulation_summary(
                source_name,
                &cloned.name,
                from_binding_kind,
                from_binding_name,
                to_binding_kind,
                to_binding_name,
                scenario,
                target.display(),
                checks,
            );
            return self.finish_simulation_summary(summary, options);
        }
        checks.push(self.run_update_plan_check(&cloned.name, &target));

        match self.apply_simulation_target(&cloned.name, target, prepared_runtime) {
            Ok((kind, name, note)) => {
                to_binding_kind = kind;
                to_binding_name = name;
                checks.push(UpgradeSimulationCheck::passed("prepare target", note));
            }
            Err(error) => {
                checks.push(UpgradeSimulationCheck::failed("prepare target", error));
                let summary = self.build_simulation_summary(
                    source_name,
                    &cloned.name,
                    from_binding_kind,
                    from_binding_name,
                    to_binding_kind,
                    to_binding_name,
                    scenario,
                    target.display(),
                    checks,
                );
                return self.finish_simulation_summary(summary, options);
            }
        }

        if matches!(target, UpgradeSimulationTarget::LocalRepo { .. }) {
            checks.push(self.run_local_repo_script_check(&cloned.name, "pnpm build", "build"));
            checks.push(self.run_local_repo_script_check(
                &cloned.name,
                "pnpm ui:build",
                "ui:build",
            ));
        }

        checks.push(self.run_simulation_check(&cloned.name, "openclaw --version", &["--version"]));
        checks.push(self.run_simulation_check_with_env(
            &cloned.name,
            "openclaw doctor",
            &["doctor", "--non-interactive", "--fix"],
            &[("OPENCLAW_UPDATE_IN_PROGRESS", "1")],
        ));
        checks.push(self.run_simulation_check(
            &cloned.name,
            "openclaw plugins update",
            &["plugins", "update", "--all", "--dry-run"],
        ));
        checks.push(self.run_simulation_check(
            &cloned.name,
            "openclaw gateway status",
            &["gateway", "status", "--deep", "--json"],
        ));

        let summary = self.build_simulation_summary(
            source_name,
            &cloned.name,
            from_binding_kind,
            from_binding_name,
            to_binding_kind,
            to_binding_name,
            scenario,
            target.display(),
            checks,
        );
        self.finish_simulation_summary(summary, options)
    }

    fn apply_simulation_scenario(
        &self,
        simulation_name: &str,
        scenario: UpgradeSimulationScenario,
    ) -> UpgradeSimulationCheck {
        match self.seed_simulation_scenario(simulation_name, scenario) {
            Ok(note) => UpgradeSimulationCheck::passed("seed scenario", note),
            Err(error) => UpgradeSimulationCheck::failed("seed scenario", error),
        }
    }

    fn seed_simulation_scenario(
        &self,
        simulation_name: &str,
        scenario: UpgradeSimulationScenario,
    ) -> Result<String, String> {
        let meta = self
            .environment_service()
            .apply_effective_gateway_port(self.environment_service().get(simulation_name)?)?;
        let gateway_port = meta.gateway_port.unwrap_or_default();
        let paths = derive_env_paths(Path::new(&meta.root));
        match scenario {
            UpgradeSimulationScenario::Current => Ok("using source env config".to_string()),
            UpgradeSimulationScenario::Minimum => {
                reset_to_minimum_simulation_config(&paths, gateway_port)?;
                Ok("seeded minimum OpenClaw config".to_string())
            }
            UpgradeSimulationScenario::Telegram => {
                reset_to_minimum_simulation_config(&paths, gateway_port)?;
                seed_telegram_simulation_config(&paths)?;
                Ok("seeded Telegram channel/plugin config".to_string())
            }
        }
    }

    fn run_update_plan_check(
        &self,
        simulation_name: &str,
        target: &UpgradeSimulationTarget,
    ) -> UpgradeSimulationCheck {
        let Some(update_args) = target.update_plan_args() else {
            return UpgradeSimulationCheck::skipped(
                "openclaw update plan",
                "local repo targets are validated through checkout build and post-update checks",
            );
        };
        let refs = update_args.iter().map(String::as_str).collect::<Vec<_>>();
        self.run_simulation_check(simulation_name, "openclaw update plan", &refs)
    }

    fn resolve_simulation_target(&self, to: &str) -> Result<UpgradeSimulationTarget, String> {
        let path = resolve_absolute_path(to, &self.env, &self.cwd)?;
        if let Some(repo_root) = detect_openclaw_checkout(&path) {
            return Ok(UpgradeSimulationTarget::LocalRepo {
                display: display_path(&repo_root),
                repo_root,
            });
        }

        let trimmed = to.trim();
        if matches!(trimmed, "stable" | "latest" | "beta" | "dev") {
            return Ok(UpgradeSimulationTarget::Official {
                target: UpgradeTarget {
                    version: None,
                    channel: Some(normalize_openclaw_channel_selector(trimmed)?),
                    runtime: None,
                },
                display: trimmed.to_string(),
            });
        }

        Ok(UpgradeSimulationTarget::Official {
            target: UpgradeTarget {
                version: Some(trimmed.to_string()),
                channel: None,
                runtime: None,
            },
            display: trimmed.to_string(),
        })
    }

    fn validate_simulation_target(&self, target: &UpgradeSimulationTarget) -> Result<(), String> {
        let UpgradeSimulationTarget::Official { target, .. } = target else {
            return Ok(());
        };

        let releases = self
            .runtime_service()
            .official_openclaw_releases(None, None)?;
        match (target.version.as_deref(), target.channel.as_deref()) {
            (Some(version), None) => {
                if releases.iter().any(|release| release.version == version) {
                    Ok(())
                } else {
                    Err(missing_simulation_version_error(version, &releases))
                }
            }
            (None, Some(channel)) => {
                if releases
                    .iter()
                    .any(|release| release.channel.as_deref() == Some(channel))
                {
                    Ok(())
                } else {
                    Err(format!(
                        "OpenClaw release channel \"{channel}\" was not found; simulation did not create any scenario envs"
                    ))
                }
            }
            _ => Err(
                "upgrade simulate requires a published version, channel, or local repo path"
                    .to_string(),
            ),
        }
    }

    fn apply_simulation_target(
        &self,
        simulation_name: &str,
        target: &UpgradeSimulationTarget,
        prepared_runtime: Option<&PreparedSimulationRuntime>,
    ) -> Result<(String, String, String), String> {
        match target {
            UpgradeSimulationTarget::Official { .. } => {
                let prepared_runtime = prepared_runtime.ok_or_else(|| {
                    "simulation target runtime was not prepared before scenario execution"
                        .to_string()
                })?;
                self.environment_service()
                    .set_runtime(simulation_name, &prepared_runtime.name)?;
                Ok((
                    "runtime".to_string(),
                    prepared_runtime.name.clone(),
                    prepared_runtime.note.clone(),
                ))
            }
            UpgradeSimulationTarget::LocalRepo { repo_root, .. } => {
                let worktree_root = ensure_openclaw_worktree(repo_root, simulation_name)?;
                let mut meta = self.environment_service().get(simulation_name)?;
                meta.default_runtime = None;
                meta.default_launcher = None;
                meta.dev = Some(EnvDevMeta {
                    repo_root: display_path(repo_root),
                    worktree_root: display_path(&worktree_root),
                });
                let mut meta = save_environment(meta, &self.env, &self.cwd)?;
                meta = self
                    .environment_service()
                    .apply_effective_gateway_port(meta)?;
                let paths = derive_env_paths(Path::new(&meta.root));
                ensure_minimum_local_openclaw_config(
                    &paths,
                    meta.gateway_port.unwrap_or_default(),
                )?;
                self.ensure_simulation_dev_dependencies(&meta)?;
                Ok((
                    "dev".to_string(),
                    "local-repo".to_string(),
                    format!("prepared local repo {}", display_path(repo_root)),
                ))
            }
        }
    }

    fn prepare_shared_simulation_runtime(
        &self,
        source_name: &str,
        target: &UpgradeSimulationTarget,
    ) -> Result<Option<PreparedSimulationRuntime>, String> {
        let UpgradeSimulationTarget::Official { target, .. } = target else {
            return Ok(None);
        };

        let canonical_name = target.canonical_runtime_name()?;
        let releases = self
            .runtime_service()
            .official_openclaw_releases(target.version.as_deref(), target.channel.as_deref())?;
        let selected = releases
            .into_iter()
            .next()
            .ok_or_else(|| "OpenClaw release was not found".to_string())?;

        if let Ok(existing) = get_runtime(&canonical_name, &self.env, &self.cwd) {
            let healthy = runtime_integrity_issue(&existing, &self.env).is_none();
            let same_release = existing.release_version.as_deref()
                == Some(selected.version.as_str())
                && existing.source_url.as_deref() == Some(selected.tarball_url.as_str());
            if healthy && same_release {
                return Ok(Some(PreparedSimulationRuntime {
                    name: canonical_name.clone(),
                    note: format!("using installed runtime {canonical_name}"),
                    temporary: false,
                }));
            }
        }

        let runtime_name = simulation_runtime_name(source_name);
        install_runtime_from_selected_official_openclaw_release(
            runtime_name.clone(),
            false,
            official_openclaw_releases_url(&self.env),
            selected,
            RuntimeReleaseDetails::with_selector(
                if target.version.is_some() {
                    Some(RuntimeReleaseSelectorKind::Version)
                } else {
                    Some(RuntimeReleaseSelectorKind::Channel)
                },
                target.version.clone().or_else(|| target.channel.clone()),
            ),
            Some("Temporary runtime for ocm upgrade simulation".to_string()),
            InstallContext {
                env: &self.env,
                cwd: &self.cwd,
            },
        )?;
        Ok(Some(PreparedSimulationRuntime {
            name: runtime_name,
            note: "installed temporary runtime for simulation".to_string(),
            temporary: true,
        }))
    }

    fn ensure_simulation_dev_dependencies(&self, meta: &crate::env::EnvMeta) -> Result<(), String> {
        let dev = meta
            .dev
            .as_ref()
            .ok_or_else(|| format!("environment \"{}\" is missing its dev binding", meta.name))?;
        let worktree_root = Path::new(&dev.worktree_root);
        let pnpm_store = worktree_root.join("node_modules").join(".pnpm");
        let tsx_bin = worktree_root.join("node_modules").join(".bin").join("tsx");
        if pnpm_store.exists() && tsx_bin.exists() {
            return Ok(());
        }

        let output = Command::new("pnpm")
            .arg("install")
            .env_clear()
            .envs(build_openclaw_env(meta, &self.env))
            .current_dir(worktree_root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("failed to run pnpm install: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        Err(format!(
            "pnpm install failed: {}",
            summarize_command_output(&output.stdout, &output.stderr)
        ))
    }

    fn run_simulation_check(
        &self,
        simulation_name: &str,
        name: &str,
        args: &[&str],
    ) -> UpgradeSimulationCheck {
        self.run_simulation_check_with_env(simulation_name, name, args, &[])
    }

    fn run_simulation_check_with_env(
        &self,
        simulation_name: &str,
        name: &str,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> UpgradeSimulationCheck {
        let args = args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        match self
            .environment_service()
            .resolve(simulation_name, None, None, &args)
        {
            Ok(resolved) => match self.run_resolved_for_simulation(resolved, extra_env) {
                Ok(output) if output.status.success() => {
                    UpgradeSimulationCheck::passed(name, output.first_line())
                }
                Ok(output) => UpgradeSimulationCheck::failed(name, output.failure_summary()),
                Err(error) => UpgradeSimulationCheck::failed(name, error),
            },
            Err(error) => UpgradeSimulationCheck::failed(name, error),
        }
    }

    fn run_local_repo_script_check(
        &self,
        simulation_name: &str,
        name: &str,
        script: &str,
    ) -> UpgradeSimulationCheck {
        match self.environment_service().get(simulation_name) {
            Ok(env_meta) => {
                let Some(dev) = env_meta.dev.as_ref() else {
                    return UpgradeSimulationCheck::failed(
                        name,
                        format!(
                            "environment \"{}\" is missing its dev binding",
                            env_meta.name
                        ),
                    );
                };
                let mut command = Command::new("pnpm");
                command
                    .arg(script)
                    .current_dir(&dev.worktree_root)
                    .env_clear()
                    .envs(build_openclaw_env(&env_meta, &self.env))
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                match command.output() {
                    Ok(output) if output.status.success() => UpgradeSimulationCheck::passed(
                        name,
                        SimulationCommandOutput::from_output(output).first_line(),
                    ),
                    Ok(output) => UpgradeSimulationCheck::failed(
                        name,
                        SimulationCommandOutput::from_output(output).failure_summary(),
                    ),
                    Err(error) => UpgradeSimulationCheck::failed(
                        name,
                        format!("failed to run simulation check: {error}"),
                    ),
                }
            }
            Err(error) => UpgradeSimulationCheck::failed(name, error),
        }
    }

    fn run_resolved_for_simulation(
        &self,
        resolved: crate::env::ResolvedExecution,
        extra_env: &[(&str, &str)],
    ) -> Result<SimulationCommandOutput, String> {
        let (mut command, env_meta) = match resolved {
            crate::env::ResolvedExecution::Launcher {
                env,
                command,
                run_dir,
                ..
            } => {
                let mut process = shell_command(&command);
                process.current_dir(run_dir);
                (process, env)
            }
            crate::env::ResolvedExecution::Runtime {
                env,
                program,
                program_args,
                run_dir,
                ..
            }
            | crate::env::ResolvedExecution::Dev {
                env,
                program,
                program_args,
                run_dir,
                ..
            } => {
                let mut process = Command::new(program);
                process.args(program_args).current_dir(run_dir);
                (process, env)
            }
        };
        let mut process_env = build_openclaw_env(&env_meta, &self.env);
        for (key, value) in extra_env {
            process_env.insert((*key).to_string(), (*value).to_string());
        }
        let output = command
            .env_clear()
            .envs(process_env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("failed to run simulation check: {error}"))?;
        Ok(SimulationCommandOutput::from_output(output))
    }

    #[allow(clippy::too_many_arguments)]
    fn build_simulation_summary(
        &self,
        source_name: &str,
        simulation_name: &str,
        from_binding_kind: String,
        from_binding_name: String,
        to_binding_kind: String,
        to_binding_name: String,
        scenario: UpgradeSimulationScenario,
        to: String,
        checks: Vec<UpgradeSimulationCheck>,
    ) -> UpgradeSimulationSummary {
        let failed = checks.iter().any(|check| check.status == "failed");
        UpgradeSimulationSummary {
            scenario: scenario.id().to_string(),
            source_env: source_name.to_string(),
            simulation_env: simulation_name.to_string(),
            from_binding_kind,
            from_binding_name,
            to_binding_kind,
            to_binding_name,
            to,
            outcome: if failed { "failed" } else { "passed" }.to_string(),
            cleanup_command: format!(
                "{} env destroy {} --yes",
                self.command_example(),
                simulation_name
            ),
            cleanup: "pending".to_string(),
            note: None,
            checks,
        }
    }

    fn finish_simulation_summary(
        &self,
        mut summary: UpgradeSimulationSummary,
        options: UpgradeSimulationOptions,
    ) -> Result<UpgradeSimulationSummary, String> {
        if options.keep_envs {
            summary.cleanup = "kept".to_string();
            summary.note = Some(
                "simulation artifacts retained because --keep-simulations was set".to_string(),
            );
            return Ok(summary);
        }

        match self
            .environment_service()
            .remove(&summary.simulation_env, true)
        {
            Ok(_) => {
                summary.cleanup = "cleaned".to_string();
            }
            Err(error) => {
                summary.cleanup = "failed".to_string();
                summary.checks.push(UpgradeSimulationCheck::failed(
                    "cleanup simulation env",
                    error,
                ));
                summary.outcome = "failed".to_string();
            }
        }
        Ok(summary)
    }

    fn finish_shared_simulation_runtime(
        &self,
        summaries: &mut [UpgradeSimulationSummary],
        prepared_runtime: Option<&PreparedSimulationRuntime>,
        options: UpgradeSimulationOptions,
    ) -> Result<(), String> {
        let Some(prepared_runtime) = prepared_runtime else {
            return Ok(());
        };
        if options.keep_envs || !prepared_runtime.temporary {
            return Ok(());
        }

        match self.remove_runtime_created_during_upgrade(&prepared_runtime.name) {
            Ok(()) => Ok(()),
            Err(error) => {
                for summary in summaries
                    .iter_mut()
                    .filter(|summary| summary.to_binding_name == prepared_runtime.name)
                {
                    summary.cleanup = "failed".to_string();
                    summary.checks.push(UpgradeSimulationCheck::failed(
                        "cleanup simulation runtime",
                        error.clone(),
                    ));
                    summary.outcome = "failed".to_string();
                }
                Ok(())
            }
        }
    }

    fn upgrade_env(
        &self,
        name: &str,
        target: &UpgradeTarget,
        options: UpgradeOptions,
    ) -> Result<UpgradeEnvSummary, String> {
        let env = self.environment_service().get(name)?;
        let service = self.service_service().status(name)?;

        if let Some(runtime_name) = env.default_runtime.as_deref() {
            return self.upgrade_runtime_bound_env(
                name,
                runtime_name,
                target,
                Some(&service),
                options,
            );
        }

        if let Some(launcher_name) = env.default_launcher.as_deref() {
            return self.upgrade_launcher_bound_env(
                name,
                launcher_name,
                target,
                Some(&service),
                options,
            );
        }

        Err(format!(
            "env \"{name}\" does not have a runtime or launcher binding; use start or env set-runtime/set-launcher first"
        ))
    }

    fn upgrade_runtime_bound_env(
        &self,
        env_name: &str,
        runtime_name: &str,
        target: &UpgradeTarget,
        service: Option<&ServiceSummary>,
        options: UpgradeOptions,
    ) -> Result<UpgradeEnvSummary, String> {
        let current = self.runtime_service().show(runtime_name)?;
        let previous_binding_name = current.name.clone();

        if target.is_explicit() {
            if options.dry_run {
                let target_runtime = target.canonical_runtime_name()?;
                let binding_changed = target_runtime != current.name;
                return Ok(UpgradeEnvSummary {
                    env_name: env_name.to_string(),
                    previous_binding_kind: "runtime".to_string(),
                    previous_binding_name,
                    binding_kind: "runtime".to_string(),
                    binding_name: target_runtime,
                    outcome: if binding_changed {
                        "would-switch".to_string()
                    } else {
                        "would-update".to_string()
                    },
                    runtime_release_version: None,
                    runtime_release_channel: target.release_channel_hint(),
                    service_action: service_action_for_dry_run(service, binding_changed, true),
                    snapshot_id: None,
                    rollback: None,
                    note: Some(
                        "dry run: no runtime, env, service, or snapshot changed".to_string(),
                    ),
                });
            }
            let target_runtime_name = target.canonical_runtime_name()?;
            let transaction = self.begin_upgrade_transaction(
                env_name,
                &[current.name.clone(), target_runtime_name.clone()],
                options.rollback_enabled,
            )?;
            let prepared = match self.prepare_upgrade_target(env_name, target) {
                Ok(prepared) => prepared,
                Err(error) => {
                    return self.rollback_failed_upgrade(
                        env_name,
                        "runtime",
                        previous_binding_name,
                        "runtime",
                        target_runtime_name,
                        None,
                        target.release_channel_hint(),
                        transaction,
                        error,
                    );
                }
            };
            let binding_changed = prepared.name != current.name;
            if binding_changed {
                self.environment_service()
                    .set_runtime(env_name, prepared.name.as_str())?;
            }
            let post_update_note = match self.run_post_core_update(env_name) {
                Ok(note) => note,
                Err(error) => {
                    return self.rollback_failed_upgrade(
                        env_name,
                        "runtime",
                        previous_binding_name,
                        "runtime",
                        prepared.name,
                        prepared.meta.release_version,
                        prepared.meta.release_channel,
                        transaction,
                        error,
                    );
                }
            };
            let service_result =
                self.reconcile_upgraded_service(env_name, service, binding_changed, true);
            let (service_action, service_note) = match service_result {
                Ok(result) => result,
                Err(error) => {
                    return self.rollback_failed_upgrade(
                        env_name,
                        "runtime",
                        previous_binding_name,
                        "runtime",
                        prepared.name,
                        prepared.meta.release_version,
                        prepared.meta.release_channel,
                        transaction,
                        error,
                    );
                }
            };
            let runtime_note = service_note.or_else(|| {
                if binding_changed {
                    Some(format!("env now uses runtime {}", prepared.name))
                } else {
                    note_for_official_prepare_action(&prepared.action)
                }
            });
            let note = join_optional_warnings(post_update_note, runtime_note);

            let summary = UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "runtime".to_string(),
                previous_binding_name,
                binding_kind: "runtime".to_string(),
                binding_name: prepared.name.clone(),
                outcome: if binding_changed {
                    "switched".to_string()
                } else {
                    outcome_for_official_prepare_action(&prepared.action)
                },
                runtime_release_version: prepared.meta.release_version.clone(),
                runtime_release_channel: prepared.meta.release_channel.clone(),
                service_action,
                snapshot_id: Some(transaction.snapshot_id.clone()),
                rollback: None,
                note,
            };
            transaction.cleanup();
            return Ok(summary);
        }

        if current.source_manifest_url.is_none() {
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "runtime".to_string(),
                previous_binding_name: previous_binding_name.clone(),
                binding_kind: "runtime".to_string(),
                binding_name: previous_binding_name,
                outcome: "manual-runtime".to_string(),
                runtime_release_version: current.release_version.clone(),
                runtime_release_channel: current.release_channel.clone(),
                service_action: None,
                snapshot_id: None,
                rollback: None,
                note: Some(
                    "this env uses a manual runtime; update it outside ocm or switch to a published release"
                        .to_string(),
                ),
            });
        }

        if current.release_selector_kind == Some(RuntimeReleaseSelectorKind::Version) {
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "runtime".to_string(),
                previous_binding_name: previous_binding_name.clone(),
                binding_kind: "runtime".to_string(),
                binding_name: previous_binding_name,
                outcome: "pinned".to_string(),
                runtime_release_version: current.release_version.clone(),
                runtime_release_channel: current.release_channel.clone(),
                service_action: None,
                snapshot_id: None,
                rollback: None,
                note: Some(
                    "this env is pinned to an exact release; pass --version or --channel to move it"
                        .to_string(),
                ),
            });
        }

        if options.dry_run {
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "runtime".to_string(),
                previous_binding_name: previous_binding_name.clone(),
                binding_kind: "runtime".to_string(),
                binding_name: previous_binding_name,
                outcome: "would-update".to_string(),
                runtime_release_version: current.release_version.clone(),
                runtime_release_channel: current.release_channel.clone(),
                service_action: service_action_for_dry_run(service, false, true),
                snapshot_id: None,
                rollback: None,
                note: Some("dry run: no runtime, env, service, or snapshot changed".to_string()),
            });
        }

        if is_official_openclaw_releases_url(current.source_manifest_url.as_deref(), &self.env) {
            let target = UpgradeTarget {
                version: None,
                channel: current.release_selector_value.clone(),
                runtime: None,
            };
            let target_runtime_name = target.canonical_runtime_name()?;
            let transaction = self.begin_upgrade_transaction(
                env_name,
                &[current.name.clone(), target_runtime_name.clone()],
                options.rollback_enabled,
            )?;
            let prepared = match self.prepare_upgrade_target(env_name, &target) {
                Ok(prepared) => prepared,
                Err(error) => {
                    return self.rollback_failed_upgrade(
                        env_name,
                        "runtime",
                        previous_binding_name,
                        "runtime",
                        target_runtime_name,
                        current.release_version.clone(),
                        current.release_channel.clone(),
                        transaction,
                        error,
                    );
                }
            };
            let changed = matches!(
                prepared.action,
                OfficialRuntimePrepareAction::Installed | OfficialRuntimePrepareAction::Updated
            );
            let post_update_note = if changed {
                match self.run_post_core_update(env_name) {
                    Ok(note) => note,
                    Err(error) => {
                        return self.rollback_failed_upgrade(
                            env_name,
                            "runtime",
                            previous_binding_name,
                            "runtime",
                            prepared.name,
                            prepared.meta.release_version,
                            prepared.meta.release_channel,
                            transaction,
                            error,
                        );
                    }
                }
            } else {
                None
            };
            let service_result = self.reconcile_upgraded_service(env_name, service, false, changed);
            let (service_action, service_note) = match service_result {
                Ok(result) => result,
                Err(error) => {
                    return self.rollback_failed_upgrade(
                        env_name,
                        "runtime",
                        previous_binding_name,
                        "runtime",
                        prepared.name,
                        prepared.meta.release_version,
                        prepared.meta.release_channel,
                        transaction,
                        error,
                    );
                }
            };
            let summary = UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "runtime".to_string(),
                previous_binding_name: previous_binding_name.clone(),
                binding_kind: "runtime".to_string(),
                binding_name: prepared.name.clone(),
                outcome: outcome_for_official_prepare_action(&prepared.action),
                runtime_release_version: prepared.meta.release_version.clone(),
                runtime_release_channel: prepared.meta.release_channel.clone(),
                service_action,
                snapshot_id: Some(transaction.snapshot_id.clone()),
                rollback: None,
                note: join_optional_warnings(
                    post_update_note,
                    service_note.or_else(|| note_for_official_prepare_action(&prepared.action)),
                ),
            };
            transaction.cleanup();
            return Ok(summary);
        }

        let transaction = self.begin_upgrade_transaction(
            env_name,
            std::slice::from_ref(&current.name),
            options.rollback_enabled,
        )?;
        let updated = match self.with_progress(format!("Updating runtime {}", current.name), || {
            self.runtime_service().update_from_release(
                crate::runtime::UpdateRuntimeFromReleaseOptions {
                    name: current.name.clone(),
                    version: None,
                    channel: None,
                },
            )
        }) {
            Ok(updated) => updated,
            Err(error) => {
                return self.rollback_failed_upgrade(
                    env_name,
                    "runtime",
                    previous_binding_name,
                    "runtime",
                    current.name,
                    current.release_version,
                    current.release_channel,
                    transaction,
                    error,
                );
            }
        };
        let post_update_note = match self.run_post_core_update(env_name) {
            Ok(note) => note,
            Err(error) => {
                return self.rollback_failed_upgrade(
                    env_name,
                    "runtime",
                    previous_binding_name,
                    "runtime",
                    updated.name,
                    updated.release_version,
                    updated.release_channel,
                    transaction,
                    error,
                );
            }
        };
        let service_result = self.reconcile_upgraded_service(env_name, service, false, true);
        let (service_action, service_note) = match service_result {
            Ok(result) => result,
            Err(error) => {
                return self.rollback_failed_upgrade(
                    env_name,
                    "runtime",
                    previous_binding_name,
                    "runtime",
                    updated.name,
                    updated.release_version,
                    updated.release_channel,
                    transaction,
                    error,
                );
            }
        };
        let summary = UpgradeEnvSummary {
            env_name: env_name.to_string(),
            previous_binding_kind: "runtime".to_string(),
            previous_binding_name: previous_binding_name.clone(),
            binding_kind: "runtime".to_string(),
            binding_name: updated.name.clone(),
            outcome: "updated".to_string(),
            runtime_release_version: updated.release_version.clone(),
            runtime_release_channel: updated.release_channel.clone(),
            service_action,
            snapshot_id: Some(transaction.snapshot_id.clone()),
            rollback: None,
            note: join_optional_warnings(post_update_note, service_note),
        };
        transaction.cleanup();
        Ok(summary)
    }

    fn upgrade_launcher_bound_env(
        &self,
        env_name: &str,
        launcher_name: &str,
        target: &UpgradeTarget,
        service: Option<&ServiceSummary>,
        options: UpgradeOptions,
    ) -> Result<UpgradeEnvSummary, String> {
        if !target.is_explicit() {
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "launcher".to_string(),
                previous_binding_name: launcher_name.to_string(),
                binding_kind: "launcher".to_string(),
                binding_name: launcher_name.to_string(),
                outcome: "local-command".to_string(),
                runtime_release_version: None,
                runtime_release_channel: None,
                service_action: None,
                snapshot_id: None,
                rollback: None,
                note: Some(
                    "this env uses a local command; update that checkout or command outside ocm"
                        .to_string(),
                ),
            });
        }

        if options.dry_run {
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "launcher".to_string(),
                previous_binding_name: launcher_name.to_string(),
                binding_kind: "runtime".to_string(),
                binding_name: target.canonical_runtime_name()?,
                outcome: "would-switch".to_string(),
                runtime_release_version: None,
                runtime_release_channel: target.release_channel_hint(),
                service_action: service_action_for_dry_run(service, true, true),
                snapshot_id: None,
                rollback: None,
                note: Some("dry run: no runtime, env, service, or snapshot changed".to_string()),
            });
        }

        let target_runtime_name = target.canonical_runtime_name()?;
        let transaction = self.begin_upgrade_transaction(
            env_name,
            std::slice::from_ref(&target_runtime_name),
            options.rollback_enabled,
        )?;
        let prepared = match self.prepare_upgrade_target(env_name, target) {
            Ok(prepared) => prepared,
            Err(error) => {
                return self.rollback_failed_upgrade(
                    env_name,
                    "launcher",
                    launcher_name.to_string(),
                    "runtime",
                    target_runtime_name,
                    None,
                    target.release_channel_hint(),
                    transaction,
                    error,
                );
            }
        };
        self.environment_service()
            .set_runtime(env_name, prepared.name.as_str())?;
        let post_update_note = match self.run_post_core_update(env_name) {
            Ok(note) => note,
            Err(error) => {
                return self.rollback_failed_upgrade(
                    env_name,
                    "launcher",
                    launcher_name.to_string(),
                    "runtime",
                    prepared.name,
                    prepared.meta.release_version,
                    prepared.meta.release_channel,
                    transaction,
                    error,
                );
            }
        };
        let service_result = self.reconcile_upgraded_service(env_name, service, true, true);
        let (service_action, service_note) = match service_result {
            Ok(result) => result,
            Err(error) => {
                return self.rollback_failed_upgrade(
                    env_name,
                    "launcher",
                    launcher_name.to_string(),
                    "runtime",
                    prepared.name,
                    prepared.meta.release_version,
                    prepared.meta.release_channel,
                    transaction,
                    error,
                );
            }
        };
        let summary = UpgradeEnvSummary {
            env_name: env_name.to_string(),
            previous_binding_kind: "launcher".to_string(),
            previous_binding_name: launcher_name.to_string(),
            binding_kind: "runtime".to_string(),
            binding_name: prepared.name.clone(),
            outcome: "switched".to_string(),
            runtime_release_version: prepared.meta.release_version.clone(),
            runtime_release_channel: prepared.meta.release_channel.clone(),
            service_action,
            snapshot_id: Some(transaction.snapshot_id.clone()),
            rollback: None,
            note: join_optional_warnings(
                post_update_note,
                service_note.or_else(|| Some(format!("env now uses runtime {}", prepared.name))),
            ),
        };
        transaction.cleanup();
        Ok(summary)
    }

    fn prepare_upgrade_target(
        &self,
        env_name: &str,
        target: &UpgradeTarget,
    ) -> Result<PreparedUpgradeTarget, String> {
        let runtime_name = target.canonical_runtime_name()?;
        if target.is_named_runtime() {
            let meta = self.runtime_service().show(&runtime_name)?;
            if let Some(issue) = runtime_integrity_issue(&meta, &self.env) {
                return Err(format!(
                    "runtime \"{runtime_name}\" is not healthy: {issue}",
                ));
            }
            return Ok(PreparedUpgradeTarget {
                name: runtime_name,
                meta,
                action: OfficialRuntimePrepareAction::Reused,
            });
        }
        let (meta, action) =
            self.with_progress(format!("Preparing OpenClaw runtime for {env_name}"), || {
                self.runtime_service().prepare_official_openclaw_runtime(
                    InstallRuntimeFromOfficialReleaseOptions {
                        name: runtime_name.clone(),
                        version: target.version.clone(),
                        channel: target.channel.clone(),
                        description: None,
                        force: false,
                    },
                )
            })?;
        Ok(PreparedUpgradeTarget {
            name: runtime_name,
            meta,
            action,
        })
    }

    fn reconcile_upgraded_service(
        &self,
        env_name: &str,
        service: Option<&ServiceSummary>,
        binding_changed: bool,
        runtime_changed: bool,
    ) -> Result<(Option<String>, Option<String>), String> {
        let Some(service) = service else {
            return Ok((None, None));
        };
        if !service.installed || !service.desired_running {
            return Ok((None, None));
        }
        if !binding_changed && !runtime_changed {
            return Ok((None, None));
        }

        if service.running {
            let restart = self
                .with_progress(format!("Restarting service for {env_name}"), || {
                    self.service_service().restart(env_name)
                })?;
            let note = join_optional_warnings(
                join_warnings(&restart.warnings),
                self.wait_for_restarted_gateway_health(env_name, restart.running)?,
            );
            return Ok((Some("restarted".to_string()), note));
        }

        if binding_changed || runtime_changed {
            let start = self.with_progress(format!("Starting service for {env_name}"), || {
                self.service_service().start(env_name)
            })?;
            let note = join_optional_warnings(
                join_warnings(&start.warnings),
                self.wait_for_restarted_gateway_health(env_name, start.running)?,
            );
            return Ok((Some("started".to_string()), note));
        }

        Ok((None, None))
    }

    fn wait_for_restarted_gateway_health(
        &self,
        env_name: &str,
        action_reported_running: bool,
    ) -> Result<Option<String>, String> {
        if !action_reported_running {
            return Ok(None);
        }

        let deadline = Instant::now() + Duration::from_secs(90);
        let mut latest_issue = None;
        while Instant::now() < deadline {
            let status = self.service_service().status(env_name)?;
            latest_issue = status.issue.clone();
            if status.running && gateway_health_ok(status.child_port.unwrap_or(status.gateway_port))
            {
                return Ok(None);
            }
            if status.gateway_state == "backoff" && status.last_exit_code != Some(0) {
                let issue = status
                    .issue
                    .or(status.last_error)
                    .unwrap_or_else(|| "gateway entered failed backoff after restart".to_string());
                return Err(format!("service restart did not recover: {issue}"));
            }
            sleep(Duration::from_millis(500));
        }

        Ok(Some(format!(
            "service restart returned before the gateway health endpoint became ready; latest status: {}",
            latest_issue.unwrap_or_else(|| "starting".to_string())
        )))
    }

    fn run_post_core_update(&self, env_name: &str) -> Result<Option<String>, String> {
        self.run_update_mode_openclaw_command(
            env_name,
            "openclaw doctor",
            &["doctor", "--non-interactive", "--fix"],
        )?;
        self.run_update_mode_openclaw_command(
            env_name,
            "openclaw plugins update",
            &["plugins", "update", "--all"],
        )?;
        Ok(Some(
            "post-update doctor and plugin update completed".to_string(),
        ))
    }

    fn run_update_mode_openclaw_command(
        &self,
        env_name: &str,
        name: &str,
        args: &[&str],
    ) -> Result<(), String> {
        let args = args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        let resolved = self
            .environment_service()
            .resolve(env_name, None, None, &args)
            .map_err(|error| format!("{name} failed: {error}"))?;
        match self.run_resolved_for_simulation(resolved, &[("OPENCLAW_UPDATE_IN_PROGRESS", "1")]) {
            Ok(output) if output.status.success() => Ok(()),
            Ok(output) => Err(format!("{name} failed: {}", output.failure_summary())),
            Err(error) => Err(format!("{name} failed: {error}")),
        }
    }

    fn begin_upgrade_transaction(
        &self,
        env_name: &str,
        runtime_names: &[String],
        rollback_enabled: bool,
    ) -> Result<UpgradeTransaction, String> {
        let snapshot = self
            .environment_service()
            .create_snapshot(CreateEnvSnapshotOptions {
                env_name: env_name.to_string(),
                label: Some("pre-upgrade".to_string()),
            })?;
        let mut seen = BTreeSet::new();
        let mut runtime_backups = Vec::new();
        let mut created_runtime_names = Vec::new();

        for runtime_name in runtime_names {
            if !seen.insert(runtime_name.clone()) {
                continue;
            }
            let meta_path = runtime_meta_path(runtime_name, &self.env, &self.cwd)?;
            if meta_path.exists() {
                let runtime = get_runtime(runtime_name, &self.env, &self.cwd)?;
                runtime_backups.push(self.backup_runtime_for_upgrade(&runtime)?);
            } else {
                created_runtime_names.push(runtime_name.clone());
            }
        }

        Ok(UpgradeTransaction {
            snapshot_id: snapshot.id,
            runtime_backups,
            created_runtime_names,
            rollback_enabled,
        })
    }

    fn backup_runtime_for_upgrade(
        &self,
        runtime: &RuntimeMeta,
    ) -> Result<RuntimeRollbackBackup, String> {
        let install_root = runtime_install_root(&runtime.name, &self.env, &self.cwd)?;
        let backup_root = if runtime
            .install_root
            .as_deref()
            .map(Path::new)
            .map(clean_path)
            .is_some_and(|path| path == install_root)
            && install_root.exists()
        {
            let parent = upgrade_backup_parent(&self.env, &self.cwd)?;
            fs::create_dir_all(&parent).map_err(|error| error.to_string())?;
            let backup_root = parent.join(format!(
                "{}-{}-{}",
                runtime.name,
                std::process::id(),
                time::OffsetDateTime::now_utc().unix_timestamp_nanos()
            ));
            copy_dir_recursive(&install_root, &backup_root)?;
            Some(backup_root)
        } else {
            None
        };

        Ok(RuntimeRollbackBackup {
            meta: runtime.clone(),
            backup_root,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn rollback_failed_upgrade(
        &self,
        env_name: &str,
        previous_binding_kind: &str,
        previous_binding_name: String,
        binding_kind: &str,
        binding_name: String,
        runtime_release_version: Option<String>,
        runtime_release_channel: Option<String>,
        transaction: UpgradeTransaction,
        error: String,
    ) -> Result<UpgradeEnvSummary, String> {
        if !transaction.rollback_enabled {
            let snapshot_id = transaction.snapshot_id.clone();
            transaction.cleanup();
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: previous_binding_kind.to_string(),
                previous_binding_name,
                binding_kind: binding_kind.to_string(),
                binding_name,
                outcome: "failed".to_string(),
                runtime_release_version,
                runtime_release_channel,
                service_action: None,
                snapshot_id: Some(snapshot_id),
                rollback: Some("disabled".to_string()),
                note: Some(format!("upgrade failed and rollback was disabled: {error}")),
            });
        }

        let rollback_result = self.rollback_upgrade(env_name, &transaction);
        let snapshot_id = transaction.snapshot_id.clone();
        transaction.cleanup();
        match rollback_result {
            Ok(()) => Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: previous_binding_kind.to_string(),
                previous_binding_name,
                binding_kind: binding_kind.to_string(),
                binding_name,
                outcome: "rolled-back".to_string(),
                runtime_release_version,
                runtime_release_channel,
                service_action: None,
                snapshot_id: Some(snapshot_id),
                rollback: Some("restored".to_string()),
                note: Some(format!(
                    "upgrade failed, so ocm restored the pre-upgrade snapshot: {error}"
                )),
            }),
            Err(rollback_error) => Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: previous_binding_kind.to_string(),
                previous_binding_name,
                binding_kind: binding_kind.to_string(),
                binding_name,
                outcome: "rollback-failed".to_string(),
                runtime_release_version,
                runtime_release_channel,
                service_action: None,
                snapshot_id: Some(snapshot_id),
                rollback: Some("failed".to_string()),
                note: Some(format!(
                    "upgrade failed ({error}); rollback also failed: {rollback_error}"
                )),
            }),
        }
    }

    fn rollback_upgrade(
        &self,
        env_name: &str,
        transaction: &UpgradeTransaction,
    ) -> Result<(), String> {
        self.environment_service()
            .restore_snapshot(RestoreEnvSnapshotOptions {
                env_name: env_name.to_string(),
                snapshot_id: transaction.snapshot_id.clone(),
            })?;
        for runtime_name in &transaction.created_runtime_names {
            self.remove_runtime_created_during_upgrade(runtime_name)?;
        }
        for runtime_backup in &transaction.runtime_backups {
            self.restore_runtime_backup(runtime_backup)?;
        }
        Ok(())
    }

    fn remove_runtime_created_during_upgrade(&self, runtime_name: &str) -> Result<(), String> {
        let meta_path = runtime_meta_path(runtime_name, &self.env, &self.cwd)?;
        if !meta_path.exists() {
            return Ok(());
        }
        remove_runtime(runtime_name, &self.env, &self.cwd).map(|_| ())
    }

    fn restore_runtime_backup(&self, backup: &RuntimeRollbackBackup) -> Result<(), String> {
        let meta_path = runtime_meta_path(&backup.meta.name, &self.env, &self.cwd)?;
        if let Some(backup_root) = backup.backup_root.as_ref() {
            let install_root = runtime_install_root(&backup.meta.name, &self.env, &self.cwd)?;
            if install_root.exists() {
                fs::remove_dir_all(&install_root).map_err(|error| {
                    format!(
                        "failed to remove upgraded runtime root {}: {error}",
                        display_path(&install_root)
                    )
                })?;
            }
            copy_dir_recursive(backup_root, &install_root)?;
        }
        write_json(&meta_path, &backup.meta)
    }
}

#[derive(Clone, Debug)]
struct PreparedUpgradeTarget {
    name: String,
    meta: RuntimeMeta,
    action: OfficialRuntimePrepareAction,
}

#[derive(Debug)]
struct SimulationCommandOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

impl SimulationCommandOutput {
    fn from_output(output: std::process::Output) -> Self {
        Self {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    fn first_line(&self) -> String {
        summarize_command_text(&self.stdout, &self.stderr).unwrap_or_else(|| "ok".to_string())
    }

    fn failure_summary(&self) -> String {
        let detail = summarize_command_text(&self.stderr, &self.stdout)
            .unwrap_or_else(|| "no output".to_string());
        format!(
            "exited with code {}: {detail}",
            self.status.code().unwrap_or(1)
        )
    }
}

impl UpgradeSimulationCheck {
    fn passed(name: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: "passed".to_string(),
            note: Some(note.into()),
        }
    }

    fn skipped(name: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: "skipped".to_string(),
            note: Some(note.into()),
        }
    }

    fn failed(name: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: "failed".to_string(),
            note: Some(note.into()),
        }
    }
}

impl UpgradeSimulationTarget {
    fn display(&self) -> String {
        match self {
            Self::Official { display, .. } | Self::LocalRepo { display, .. } => display.clone(),
        }
    }

    fn update_plan_args(&self) -> Option<Vec<String>> {
        match self {
            Self::Official { target, .. } => {
                let mut args = vec![
                    "update".to_string(),
                    "--dry-run".to_string(),
                    "--json".to_string(),
                    "--no-restart".to_string(),
                    "--yes".to_string(),
                ];
                if let Some(channel) = target.channel.as_deref() {
                    args.push("--channel".to_string());
                    args.push(channel.to_string());
                } else if let Some(version) = target.version.as_deref() {
                    args.push("--tag".to_string());
                    args.push(version.to_string());
                }
                Some(args)
            }
            Self::LocalRepo { .. } => None,
        }
    }
}

impl UpgradeSimulationScenario {
    fn parse_many(raw: Option<&str>) -> Result<Vec<Self>, String> {
        let Some(raw) = raw else {
            return Ok(vec![Self::Current]);
        };
        let mut scenarios: Vec<Self> = Vec::new();
        for token in raw.split(',') {
            let token = token.trim().to_ascii_lowercase();
            if token.is_empty() {
                return Err("--scenario cannot contain an empty scenario".to_string());
            }
            if token == "all" {
                return Ok(vec![Self::Current, Self::Minimum, Self::Telegram]);
            }
            let scenario = match token.as_str() {
                "current" | "source" => Self::Current,
                "minimum" | "clean" => Self::Minimum,
                "telegram" => Self::Telegram,
                _ => {
                    return Err(format!(
                        "unknown upgrade simulation scenario \"{token}\"; use current, minimum, telegram, or all"
                    ));
                }
            };
            if !scenarios
                .iter()
                .any(|existing| existing.id() == scenario.id())
            {
                scenarios.push(scenario);
            }
        }
        if scenarios.is_empty() {
            return Err("--scenario requires current, minimum, telegram, or all".to_string());
        }
        Ok(scenarios)
    }

    fn id(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Minimum => "minimum",
            Self::Telegram => "telegram",
        }
    }
}

#[derive(Debug)]
struct UpgradeTransaction {
    snapshot_id: String,
    runtime_backups: Vec<RuntimeRollbackBackup>,
    created_runtime_names: Vec<String>,
    rollback_enabled: bool,
}

impl UpgradeTransaction {
    fn cleanup(self) {
        for runtime_backup in self.runtime_backups {
            runtime_backup.cleanup();
        }
    }
}

#[derive(Debug)]
struct RuntimeRollbackBackup {
    meta: RuntimeMeta,
    backup_root: Option<PathBuf>,
}

impl RuntimeRollbackBackup {
    fn cleanup(mut self) {
        if let Some(backup_root) = self.backup_root.take() {
            let _ = fs::remove_dir_all(backup_root);
        }
    }
}

impl Drop for RuntimeRollbackBackup {
    fn drop(&mut self) {
        if let Some(backup_root) = self.backup_root.take() {
            let _ = fs::remove_dir_all(backup_root);
        }
    }
}

fn upgrade_backup_parent(
    env: &std::collections::BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    Ok(ensure_store(env, cwd)?
        .home
        .join("tmp")
        .join("upgrade-runtime-backups"))
}

fn source_binding(env: &crate::env::EnvMeta) -> (String, String) {
    if let Some(runtime) = env.default_runtime.clone() {
        return ("runtime".to_string(), runtime);
    }
    if let Some(launcher) = env.default_launcher.clone() {
        return ("launcher".to_string(), launcher);
    }
    if env.dev.is_some() {
        return ("dev".to_string(), "dev".to_string());
    }
    ("none".to_string(), "none".to_string())
}

fn build_simulation_batch_summary(
    summaries: Vec<UpgradeSimulationSummary>,
) -> UpgradeSimulationBatchSummary {
    let source_env = summaries
        .first()
        .map(|summary| summary.source_env.clone())
        .unwrap_or_default();
    let to = summaries
        .first()
        .map(|summary| summary.to.clone())
        .unwrap_or_default();
    let failed = summaries
        .iter()
        .filter(|summary| summary.outcome == "failed")
        .count();
    UpgradeSimulationBatchSummary {
        source_env,
        to,
        count: summaries.len(),
        passed: summaries.len().saturating_sub(failed),
        failed,
        results: summaries,
    }
}

fn missing_simulation_version_error(version: &str, releases: &[OpenClawRelease]) -> String {
    let prefix = format!("{version}-");
    let nearby = releases
        .iter()
        .filter(|release| release.version.starts_with(&prefix))
        .map(|release| release.version.as_str())
        .take(5)
        .collect::<Vec<_>>();

    let mut message = format!(
        "OpenClaw release version \"{version}\" was not found; simulation did not create any scenario envs"
    );
    if !nearby.is_empty() {
        message.push_str(&format!(
            ". Nearby published releases: {}",
            nearby.join(", ")
        ));
    }
    message.push_str(
        ". Use an exact published version, a channel such as beta, or a local OpenClaw repo path.",
    );
    message
}

fn simulation_env_name(source_name: &str, scenario: &str) -> String {
    format!(
        "{}-{}-sim-{}",
        source_name,
        scenario,
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}

fn simulation_runtime_name(source_name: &str) -> String {
    format!(
        "ocm-sim-runtime-{}-{}",
        source_name,
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
}

fn reset_to_minimum_simulation_config(
    paths: &crate::store::EnvPaths,
    gateway_port: u32,
) -> Result<(), String> {
    if let Some(parent) = paths.config_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    if paths.config_path.exists() {
        fs::remove_file(&paths.config_path).map_err(|error| error.to_string())?;
    }
    ensure_minimum_local_openclaw_config(paths, gateway_port)
}

fn seed_telegram_simulation_config(paths: &crate::store::EnvPaths) -> Result<(), String> {
    let raw = fs::read_to_string(&paths.config_path).map_err(|error| error.to_string())?;
    let mut value: Value = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    let root = value
        .as_object_mut()
        .ok_or_else(|| "OpenClaw config root must be an object".to_string())?;

    let channels = ensure_json_object_field(root, "channels");
    channels.insert(
        "telegram".to_string(),
        json!({
            "enabled": true,
            "botToken": "123456:simulation-token",
            "allowFrom": ["*"],
            "groupPolicy": "open"
        }),
    );

    let plugins = ensure_json_object_field(root, "plugins");
    let mut allow = plugins
        .get("allow")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if !allow.iter().any(|entry| entry.as_str() == Some("telegram")) {
        allow.push(Value::String("telegram".to_string()));
    }
    plugins.insert("allow".to_string(), Value::Array(allow));

    let mut rewritten = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    rewritten.push('\n');
    fs::write(&paths.config_path, rewritten).map_err(|error| error.to_string())
}

fn ensure_json_object_field<'a>(
    object: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> &'a mut serde_json::Map<String, Value> {
    let needs_reset = !object.get(key).is_some_and(Value::is_object);
    if needs_reset {
        object.insert(key.to_string(), Value::Object(serde_json::Map::new()));
    }
    object
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .expect("object field must exist after reset")
}

fn summarize_command_text(primary: &str, secondary: &str) -> Option<String> {
    for text in [primary, secondary] {
        if let Some(line) = text.lines().find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed.to_string())
        }) {
            return Some(line);
        }
    }
    None
}

fn summarize_command_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    summarize_command_text(&stderr, &stdout).unwrap_or_else(|| "no output".to_string())
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut process = Command::new("cmd");
        process.args(["/C", command]);
        process
    } else {
        let mut process = Command::new("/bin/sh");
        process.args(["-lc", command]);
        process
    }
}

fn service_action_for_dry_run(
    service: Option<&ServiceSummary>,
    binding_changed: bool,
    runtime_changed: bool,
) -> Option<String> {
    let service = service?;
    if !service.installed || !service.desired_running || (!binding_changed && !runtime_changed) {
        return None;
    }
    if service.running {
        Some("would-restart".to_string())
    } else {
        Some("would-start".to_string())
    }
}

fn is_changed_upgrade_outcome(outcome: &str) -> bool {
    matches!(
        outcome,
        "updated" | "switched" | "would-update" | "would-switch"
    )
}

fn is_failed_upgrade_outcome(outcome: &str) -> bool {
    matches!(outcome, "failed" | "rolled-back" | "rollback-failed")
}

fn outcome_for_official_prepare_action(action: &OfficialRuntimePrepareAction) -> String {
    match action {
        OfficialRuntimePrepareAction::Installed | OfficialRuntimePrepareAction::Updated => {
            "updated".to_string()
        }
        OfficialRuntimePrepareAction::Reused => "up-to-date".to_string(),
    }
}

fn note_for_official_prepare_action(action: &OfficialRuntimePrepareAction) -> Option<String> {
    match action {
        OfficialRuntimePrepareAction::Installed => {
            Some("installed the requested runtime".to_string())
        }
        OfficialRuntimePrepareAction::Updated => Some("updated the tracked runtime".to_string()),
        OfficialRuntimePrepareAction::Reused => None,
    }
}

fn join_warnings(warnings: &[String]) -> Option<String> {
    if warnings.is_empty() {
        None
    } else {
        Some(warnings.join(" "))
    }
}

fn join_optional_warnings(left: Option<String>, right: Option<String>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(format!("{left} {right}")),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}

fn gateway_health_ok(port: u32) -> bool {
    if port == 0 || port > u16::MAX as u32 {
        return false;
    }
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port as u16);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr.into(), Duration::from_millis(500))
    else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
    let request =
        format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = [0_u8; 256];
    let Ok(read) = stream.read(&mut response) else {
        return false;
    };
    let text = String::from_utf8_lossy(&response[..read]);
    text.starts_with("HTTP/1.1 200") || text.starts_with("HTTP/1.0 200")
}
