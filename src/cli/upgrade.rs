use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use super::{Cli, render};
use crate::env::{
    CloneEnvironmentOptions, CreateEnvSnapshotOptions, EnvDevMeta, RestoreEnvSnapshotOptions,
};
use crate::infra::shell::build_openclaw_env;
use crate::openclaw_repo::{detect_openclaw_checkout, ensure_openclaw_worktree};
use crate::runtime::releases::{
    is_official_openclaw_releases_url, normalize_openclaw_channel_selector,
};
use crate::runtime::{
    InstallRuntimeFromOfficialReleaseOptions, OfficialRuntimePrepareAction, RuntimeMeta,
    RuntimeReleaseSelectorKind, RuntimeService,
};
use crate::service::ServiceSummary;
use crate::store::{
    clean_path, copy_dir_recursive, derive_env_paths, display_path,
    ensure_minimum_local_openclaw_config, ensure_store, get_runtime, remove_runtime,
    resolve_absolute_path, runtime_install_root, runtime_meta_path, save_environment, write_json,
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
    pub note: Option<String>,
}

#[derive(Clone, Debug)]
struct UpgradeTarget {
    version: Option<String>,
    channel: Option<String>,
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
struct UpgradeOptions {
    dry_run: bool,
    rollback_enabled: bool,
}

impl UpgradeTarget {
    fn parse(args: Vec<String>) -> Result<(Vec<String>, Self), String> {
        let (args, version) = Cli::consume_option(args, "--version")?;
        let version = Cli::require_option_value(version, "--version")?;
        let (args, channel) = Cli::consume_option(args, "--channel")?;
        let channel = Cli::require_option_value(channel, "--channel")?;
        if version.is_some() && channel.is_some() {
            return Err("upgrade accepts only one of --version or --channel".to_string());
        }
        Ok((args, Self { version, channel }))
    }

    fn is_explicit(&self) -> bool {
        self.version.is_some() || self.channel.is_some()
    }

    fn canonical_runtime_name(&self) -> Result<String, String> {
        RuntimeService::canonical_official_openclaw_runtime_name(
            self.version.as_deref(),
            self.channel.as_deref(),
        )
    }
}

impl Cli {
    pub(super) fn handle_upgrade_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "upgrade")?;
        if matches!(args.first().map(String::as_str), Some("simulate")) {
            let summary = self.upgrade_simulate(args[1..].to_vec())?;
            let failed = summary.outcome == "failed";
            if json_flag {
                self.print_json(&summary)?;
                return Ok(if failed { 1 } else { 0 });
            }
            self.stdout_lines(render::upgrade::upgrade_simulation(
                &summary,
                profile,
                &self.command_example(),
            ));
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
                    "upgrade --all does not accept --version or --channel; upgrade one env at a time when changing selectors"
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

    fn upgrade_simulate(&self, args: Vec<String>) -> Result<UpgradeSimulationSummary, String> {
        let (args, to) = Self::consume_option(args, "--to")?;
        let to = Self::require_option_value(to, "--to")?.ok_or_else(|| {
            "upgrade simulate requires --to <version|channel|repo-path>".to_string()
        })?;
        let Some(source_name) = args.first() else {
            return Err("upgrade simulate requires an environment name".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let source = self.environment_service().get(source_name)?;
        let (from_binding_kind, from_binding_name) = source_binding(&source);
        let target = self.resolve_simulation_target(&to)?;
        let simulation_name = simulation_env_name(source_name);
        let cloned = self.environment_service().clone(CloneEnvironmentOptions {
            source_name: source_name.to_string(),
            name: simulation_name.clone(),
            root: None,
        })?;
        self.environment_service()
            .set_service_policy(&cloned.name, Some(false), Some(false))?;
        if cloned.protected {
            self.environment_service()
                .set_protected(&cloned.name, false)?;
        }

        let mut checks = vec![UpgradeSimulationCheck::passed(
            "clone env",
            format!("created isolated env {}", cloned.name),
        )];
        let mut to_binding_kind = "unknown".to_string();
        let mut to_binding_name = "unknown".to_string();

        match self.apply_simulation_target(&cloned.name, &target) {
            Ok((kind, name, note)) => {
                to_binding_kind = kind;
                to_binding_name = name;
                checks.push(UpgradeSimulationCheck::passed("prepare target", note));
            }
            Err(error) => {
                checks.push(UpgradeSimulationCheck::failed("prepare target", error));
                return Ok(self.build_simulation_summary(
                    source_name,
                    &cloned.name,
                    from_binding_kind,
                    from_binding_name,
                    to_binding_kind,
                    to_binding_name,
                    target.display(),
                    checks,
                ));
            }
        }

        checks.push(self.run_simulation_check(&cloned.name, "openclaw --version", &["--version"]));
        checks.push(self.run_simulation_check(&cloned.name, "openclaw doctor", &["doctor"]));

        Ok(self.build_simulation_summary(
            source_name,
            &cloned.name,
            from_binding_kind,
            from_binding_name,
            to_binding_kind,
            to_binding_name,
            target.display(),
            checks,
        ))
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
                },
                display: trimmed.to_string(),
            });
        }

        Ok(UpgradeSimulationTarget::Official {
            target: UpgradeTarget {
                version: Some(trimmed.to_string()),
                channel: None,
            },
            display: trimmed.to_string(),
        })
    }

    fn apply_simulation_target(
        &self,
        simulation_name: &str,
        target: &UpgradeSimulationTarget,
    ) -> Result<(String, String, String), String> {
        match target {
            UpgradeSimulationTarget::Official { target, .. } => {
                let prepared = self.prepare_upgrade_target(simulation_name, target)?;
                self.environment_service()
                    .set_runtime(simulation_name, &prepared.name)?;
                Ok((
                    "runtime".to_string(),
                    prepared.name.clone(),
                    note_for_official_prepare_action(&prepared.action)
                        .unwrap_or_else(|| format!("using runtime {}", prepared.name)),
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
        let args = args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>();
        match self
            .environment_service()
            .resolve(simulation_name, None, None, &args)
        {
            Ok(resolved) => match self.run_resolved_for_simulation(resolved) {
                Ok(output) if output.status.success() => {
                    UpgradeSimulationCheck::passed(name, output.first_line())
                }
                Ok(output) => UpgradeSimulationCheck::failed(name, output.failure_summary()),
                Err(error) => UpgradeSimulationCheck::failed(name, error),
            },
            Err(error) => UpgradeSimulationCheck::failed(name, error),
        }
    }

    fn run_resolved_for_simulation(
        &self,
        resolved: crate::env::ResolvedExecution,
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
        let output = command
            .env_clear()
            .envs(build_openclaw_env(&env_meta, &self.env))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|error| format!("failed to run simulation check: {error}"))?;
        Ok(SimulationCommandOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
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
        to: String,
        checks: Vec<UpgradeSimulationCheck>,
    ) -> UpgradeSimulationSummary {
        let failed = checks.iter().any(|check| check.status == "failed");
        UpgradeSimulationSummary {
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
            note: if failed {
                Some(
                    "source env was not changed; inspect the simulation env before destroying it"
                        .to_string(),
                )
            } else {
                Some(
                    "source env was not changed; destroy the simulation env when you are done"
                        .to_string(),
                )
            },
            checks,
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
                    runtime_release_channel: target.channel.clone(),
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
                        target.channel.clone(),
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
            let note = service_note.or_else(|| {
                if binding_changed {
                    Some(format!("env now uses runtime {}", prepared.name))
                } else {
                    note_for_official_prepare_action(&prepared.action)
                }
            });

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
                note: service_note.or_else(|| note_for_official_prepare_action(&prepared.action)),
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
            note: service_note,
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
                runtime_release_channel: target.channel.clone(),
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
                    target.channel.clone(),
                    transaction,
                    error,
                );
            }
        };
        self.environment_service()
            .set_runtime(env_name, prepared.name.as_str())?;
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
            note: service_note.or_else(|| Some(format!("env now uses runtime {}", prepared.name))),
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
            let note = join_warnings(&restart.warnings);
            return Ok((Some("restarted".to_string()), note));
        }

        if binding_changed || runtime_changed {
            let start = self.with_progress(format!("Starting service for {env_name}"), || {
                self.service_service().start(env_name)
            })?;
            let note = join_warnings(&start.warnings);
            return Ok((Some("started".to_string()), note));
        }

        Ok((None, None))
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

fn simulation_env_name(source_name: &str) -> String {
    format!(
        "{}-sim-{}",
        source_name,
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    )
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
