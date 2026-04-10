use serde::Serialize;

use super::{Cli, render};
use crate::runtime::releases::is_official_openclaw_releases_url;
use crate::runtime::{
    InstallRuntimeFromOfficialReleaseOptions, OfficialRuntimePrepareAction, RuntimeMeta,
    RuntimeReleaseSelectorKind, RuntimeService,
};
use crate::service::ServiceSummary;

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

#[derive(Clone, Debug)]
struct UpgradeTarget {
    version: Option<String>,
    channel: Option<String>,
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
        let (args, all_flag) = Self::consume_flag(args, "--all");
        let (args, target) = UpgradeTarget::parse(args)?;

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
                match self.upgrade_env(&env.name, &target) {
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
                        note: Some(error),
                    }),
                }
            }

            let summary = UpgradeBatchSummary {
                count: results.len(),
                changed: results
                    .iter()
                    .filter(|summary| matches!(summary.outcome.as_str(), "updated" | "switched"))
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
                    .filter(|summary| summary.outcome == "failed")
                    .count(),
                results,
            };

            if json_flag {
                self.print_json(&summary)?;
                return Ok(0);
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

        let summary = self.upgrade_env(name, &target)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::upgrade::upgrade_env(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    fn upgrade_env(&self, name: &str, target: &UpgradeTarget) -> Result<UpgradeEnvSummary, String> {
        let env = self.environment_service().get(name)?;
        let service = self.service_service().status_fast(name)?;

        if let Some(runtime_name) = env.default_runtime.as_deref() {
            return self.upgrade_runtime_bound_env(name, runtime_name, target, Some(&service));
        }

        if let Some(launcher_name) = env.default_launcher.as_deref() {
            return self.upgrade_launcher_bound_env(name, launcher_name, target, Some(&service));
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
    ) -> Result<UpgradeEnvSummary, String> {
        let current = self.runtime_service().show(runtime_name)?;
        let previous_binding_name = current.name.clone();

        if target.is_explicit() {
            let prepared = self.prepare_upgrade_target(env_name, target)?;
            let binding_changed = prepared.name != current.name;
            if binding_changed {
                self.environment_service()
                    .set_runtime(env_name, prepared.name.as_str())?;
            }
            let (service_action, service_note) =
                self.reconcile_upgraded_service(env_name, service, binding_changed, true)?;
            let note = service_note.or_else(|| {
                if binding_changed {
                    Some(format!("env now uses runtime {}", prepared.name))
                } else {
                    note_for_official_prepare_action(&prepared.action)
                }
            });

            return Ok(UpgradeEnvSummary {
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
                note,
            });
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
                note: Some(
                    "this env is pinned to an exact release; pass --version or --channel to move it"
                        .to_string(),
                ),
            });
        }

        if is_official_openclaw_releases_url(current.source_manifest_url.as_deref(), &self.env) {
            let prepared = self.prepare_upgrade_target(
                env_name,
                &UpgradeTarget {
                    version: None,
                    channel: current.release_selector_value.clone(),
                },
            )?;
            let changed = matches!(
                prepared.action,
                OfficialRuntimePrepareAction::Installed | OfficialRuntimePrepareAction::Updated
            );
            let (service_action, service_note) =
                self.reconcile_upgraded_service(env_name, service, false, changed)?;
            return Ok(UpgradeEnvSummary {
                env_name: env_name.to_string(),
                previous_binding_kind: "runtime".to_string(),
                previous_binding_name: previous_binding_name.clone(),
                binding_kind: "runtime".to_string(),
                binding_name: prepared.name.clone(),
                outcome: outcome_for_official_prepare_action(&prepared.action),
                runtime_release_version: prepared.meta.release_version.clone(),
                runtime_release_channel: prepared.meta.release_channel.clone(),
                service_action,
                note: service_note.or_else(|| note_for_official_prepare_action(&prepared.action)),
            });
        }

        let updated = self.with_progress(format!("Updating runtime {}", current.name), || {
            self.runtime_service().update_from_release(
                crate::runtime::UpdateRuntimeFromReleaseOptions {
                    name: current.name.clone(),
                    version: None,
                    channel: None,
                },
            )
        })?;
        let (service_action, service_note) =
            self.reconcile_upgraded_service(env_name, service, false, true)?;
        Ok(UpgradeEnvSummary {
            env_name: env_name.to_string(),
            previous_binding_kind: "runtime".to_string(),
            previous_binding_name: previous_binding_name.clone(),
            binding_kind: "runtime".to_string(),
            binding_name: updated.name.clone(),
            outcome: "updated".to_string(),
            runtime_release_version: updated.release_version.clone(),
            runtime_release_channel: updated.release_channel.clone(),
            service_action,
            note: service_note,
        })
    }

    fn upgrade_launcher_bound_env(
        &self,
        env_name: &str,
        launcher_name: &str,
        target: &UpgradeTarget,
        service: Option<&ServiceSummary>,
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
                note: Some(
                    "this env uses a local command; update that checkout or command outside ocm"
                        .to_string(),
                ),
            });
        }

        let prepared = self.prepare_upgrade_target(env_name, target)?;
        self.environment_service()
            .set_runtime(env_name, prepared.name.as_str())?;
        let (service_action, service_note) =
            self.reconcile_upgraded_service(env_name, service, true, true)?;
        Ok(UpgradeEnvSummary {
            env_name: env_name.to_string(),
            previous_binding_kind: "launcher".to_string(),
            previous_binding_name: launcher_name.to_string(),
            binding_kind: "runtime".to_string(),
            binding_name: prepared.name.clone(),
            outcome: "switched".to_string(),
            runtime_release_version: prepared.meta.release_version.clone(),
            runtime_release_channel: prepared.meta.release_channel.clone(),
            service_action,
            note: service_note.or_else(|| Some(format!("env now uses runtime {}", prepared.name))),
        })
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
        let live_service = service.loaded || service.running;
        if !service.installed && !live_service {
            return Ok((None, None));
        }
        let definition_changed = service.definition_drift;
        if !binding_changed && !runtime_changed && !definition_changed {
            return Ok((None, None));
        }

        if live_service && service.installed {
            let restart = self
                .with_progress(format!("Restarting service for {env_name}"), || {
                    self.service_service().restart(env_name)
                })?;
            let note = join_warnings(&restart.warnings);
            return Ok((Some("restarted".to_string()), note));
        }

        if binding_changed || runtime_changed || definition_changed {
            let start = self.with_progress(format!("Starting service for {env_name}"), || {
                self.service_service().start(env_name)
            })?;
            let note = join_warnings(&start.warnings);
            return Ok((Some("started".to_string()), note));
        }

        Ok((None, None))
    }
}

#[derive(Clone, Debug)]
struct PreparedUpgradeTarget {
    name: String,
    meta: RuntimeMeta,
    action: OfficialRuntimePrepareAction,
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
