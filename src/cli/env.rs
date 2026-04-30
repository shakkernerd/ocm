use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::thread::sleep;
#[cfg(unix)]
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Cli, render};
use crate::env::{
    CloneEnvironmentOptions, CreateEnvSnapshotOptions, CreateEnvironmentOptions, EnvMeta,
    EnvSummary, ExportEnvironmentOptions, ImportEnvironmentOptions, RemoveEnvSnapshotOptions,
    RestoreEnvSnapshotOptions,
};
use crate::infra::process::{run_direct, run_shell};
use crate::infra::shell::{build_openclaw_env, render_use_script, resolve_shell_name};
use crate::openclaw_repo::remove_openclaw_worktree;
use crate::store::{derive_env_paths, summarize_env, validate_name};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnvDestroyStepSummary {
    pub kind: String,
    pub description: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EnvDestroySummary {
    pub env_name: String,
    pub root: String,
    pub dev_worktree: Option<String>,
    pub protected: bool,
    pub apply: bool,
    pub force: bool,
    pub snapshot_count: usize,
    pub service_installed: bool,
    pub service_loaded: bool,
    pub service_running: bool,
    pub service_label: String,
    pub blockers: Vec<String>,
    pub steps: Vec<EnvDestroyStepSummary>,
    pub snapshots_removed: usize,
    pub service_uninstalled: bool,
    pub processes_terminated: usize,
    pub worktree_removed: bool,
    pub removed: bool,
}

impl Cli {
    pub(super) fn handle_env_protect(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env protect")?;
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env protect <env> <on|off>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let value = args[1].trim().to_ascii_lowercase();
        Self::assert_no_extra_args(&args[2..])?;
        if value != "on" && value != "off" {
            return Err("protection must be \"on\" or \"off\"".to_string());
        }

        let meta = self
            .environment_service()
            .set_protected(name, value == "on")?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }
        self.stdout_lines(render::env::env_protected(
            &meta.name,
            meta.protected,
            profile,
        ));
        Ok(0)
    }

    pub(super) fn handle_env_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env remove")?;
        let (args, force) = Self::consume_flag(args, "--force");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.environment_service().remove(name, force)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }
        let root = derive_env_paths(Path::new(&meta.root))
            .root
            .display()
            .to_string();
        self.stdout_lines(render::env::env_removed(&meta.name, &root, profile));
        Ok(0)
    }

    pub(super) fn handle_env_destroy(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env destroy")?;
        let (args, yes) = Self::consume_flag(args, "--yes");
        let (args, force) = Self::consume_flag(args, "--force");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let mut summary = self.build_env_destroy_summary(name, yes, force)?;
        if !yes {
            if json_flag {
                self.print_json(&summary)?;
                return Ok(0);
            }

            self.stdout_lines(render::env::env_destroy_preview(
                &summary,
                profile,
                &self.command_example(),
            ));
            return Ok(0);
        }

        if !summary.blockers.is_empty() {
            if json_flag {
                self.print_json(&summary)?;
            } else {
                self.stdout_lines(render::env::env_destroy_preview(
                    &summary,
                    profile,
                    &self.command_example(),
                ));
            }
            return Ok(1);
        }

        let env_meta = self.environment_service().get(name)?;

        let snapshot_ids = self
            .environment_service()
            .list_snapshots(Some(name))?
            .into_iter()
            .map(|snapshot| snapshot.id)
            .collect::<Vec<_>>();

        if summary.service_installed || summary.service_loaded || summary.service_running {
            self.service_service().uninstall(name)?;
            summary.service_uninstalled = true;
        }

        summary.processes_terminated = self.terminate_env_processes(&env_meta)?;

        for snapshot_id in &snapshot_ids {
            self.environment_service()
                .remove_snapshot(RemoveEnvSnapshotOptions {
                    env_name: name.clone(),
                    snapshot_id: snapshot_id.clone(),
                })?;
        }
        summary.snapshots_removed = snapshot_ids.len();

        if let Some(dev) = env_meta.dev.as_ref() {
            remove_openclaw_worktree(Path::new(&dev.repo_root), Path::new(&dev.worktree_root))?;
            summary.worktree_removed = !Path::new(&dev.worktree_root).exists();
        }

        self.environment_service().remove(name, force)?;
        summary.removed = true;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_destroyed(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_env_prune(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env prune")?;
        let (args, yes) = Self::consume_flag(args, "--yes");
        let (args, older_than_raw) = Self::consume_option(args, "--older-than")?;
        Self::assert_no_extra_args(&args)?;

        let older_than_days = match older_than_raw.as_deref() {
            Some(raw) => Self::parse_positive_u32(raw, "--older-than")? as i64,
            _ => 14,
        };

        let candidates = self
            .environment_service()
            .prune_candidates(older_than_days)?;
        let candidate_summaries = candidates.iter().map(summarize_env).collect::<Vec<_>>();

        if !yes {
            if json_flag {
                self.print_json(&serde_json::json!({
                    "apply": false,
                    "olderThanDays": older_than_days,
                    "count": candidate_summaries.len(),
                    "candidates": candidate_summaries,
                }))?;
                return Ok(0);
            }

            self.stdout_lines(render::env::env_prune_preview(
                older_than_days,
                &candidate_summaries,
                profile,
            ));
            return Ok(0);
        }

        let mut removed = Vec::<EnvSummary>::new();
        let removed_meta = self.environment_service().prune(older_than_days)?;
        for meta in removed_meta {
            removed.push(summarize_env(&meta));
        }

        if json_flag {
            self.print_json(&serde_json::json!({
                "apply": true,
                "olderThanDays": older_than_days,
                "count": removed.len(),
                "removed": removed,
            }))?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_pruned(&removed, profile));
        Ok(0)
    }

    pub(super) fn handle_env_create(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env create")?;
        let (args, protect) = Self::consume_flag(args, "--protect");
        let (args, root) = Self::consume_option(args, "--root")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--port")?),
            _ => None,
        };
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        let (args, runtime_name) = Self::consume_option(args, "--runtime")?;
        let runtime_name = Self::require_option_value(runtime_name, "--runtime")?;
        let (args, launcher_name) = Self::consume_option(args, "--launcher")?;
        let launcher_name = Self::require_option_value(launcher_name, "--launcher")?;

        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let uses_release_selector = version.is_some() || channel.is_some();
        let runtime_name = if uses_release_selector {
            self.with_progress(format!("Preparing OpenClaw runtime for {name}"), || {
                self.environment_service().resolve_runtime_binding_request(
                    runtime_name,
                    version,
                    channel,
                    "env create",
                )
            })?
        } else {
            self.environment_service().resolve_runtime_binding_request(
                runtime_name,
                version,
                channel,
                "env create",
            )?
        };
        let meta = self
            .environment_service()
            .create(CreateEnvironmentOptions {
                name: name.clone(),
                root,
                gateway_port,
                service_enabled: false,
                service_running: false,
                default_runtime: runtime_name,
                default_launcher: launcher_name,
                dev: None,
                protected: protect,
            })?;

        if json_flag {
            self.print_json(&summarize_env(&meta))?;
            return Ok(0);
        }

        let (gateway_port, gateway_port_source) = self
            .environment_service()
            .resolve_effective_gateway_port(&meta)?;
        let mut display_meta = meta.clone();
        display_meta.gateway_port = Some(gateway_port);
        let summary = summarize_env(&display_meta);
        self.stdout_lines(render::env::env_created(
            &summary,
            Some(gateway_port_source),
            &self.command_example(),
            profile,
        ));
        Ok(0)
    }

    pub(super) fn handle_env_clone(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env clone")?;
        let (args, root) = Self::consume_option(args, "--root")?;
        let Some(source_name) = args.first() else {
            return Err("source environment name is required".to_string());
        };
        let Some(target_name) = args.get(1) else {
            return Err("target environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let meta = self.with_progress(
            format!("Cloning env {source_name} to {target_name}"),
            || {
                self.environment_service().clone(CloneEnvironmentOptions {
                    source_name: source_name.clone(),
                    name: target_name.clone(),
                    root,
                })
            },
        )?;

        if json_flag {
            self.print_json(&summarize_env(&meta))?;
            return Ok(0);
        }

        let (gateway_port, gateway_port_source) = self
            .environment_service()
            .resolve_effective_gateway_port(&meta)?;
        let mut display_meta = meta.clone();
        display_meta.gateway_port = Some(gateway_port);
        let summary = summarize_env(&display_meta);
        self.stdout_lines(render::env::env_cloned(
            &summary,
            Some(gateway_port_source),
            source_name,
            &self.command_example(),
            profile,
        ));
        Ok(0)
    }

    pub(super) fn handle_env_export(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env export")?;
        let (args, output) = Self::consume_option(args, "--output")?;
        let output = Self::require_option_value(output, "--output")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.with_progress(format!("Exporting env {name}"), || {
            self.environment_service().export(ExportEnvironmentOptions {
                name: name.clone(),
                output,
            })
        })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_exported(&summary, profile));
        Ok(0)
    }

    pub(super) fn handle_env_import(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env import")?;
        let (args, name) = Self::consume_option(args, "--name")?;
        let name = Self::require_option_value(name, "--name")?;
        let (args, root) = Self::consume_option(args, "--root")?;
        let root = Self::require_option_value(root, "--root")?;
        let Some(archive) = args.first() else {
            return Err("archive path is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.with_progress("Importing environment archive", || {
            self.environment_service().import(ImportEnvironmentOptions {
                archive: archive.clone(),
                name,
                root,
            })
        })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_imported(
            &summary,
            &self.command_example(),
            profile,
        ));
        Ok(0)
    }

    pub(super) fn handle_env_doctor(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env doctor")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let doctor = self.environment_service().doctor(name)?;
        if json_flag {
            self.print_json(&doctor)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_doctor(
            &doctor,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_env_cleanup(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env cleanup")?;
        let (args, yes_flag) = Self::consume_flag(args, "--yes");
        let (args, all_flag) = Self::consume_flag(args, "--all");

        if all_flag {
            if !args.is_empty() {
                return Err("env cleanup accepts either <name> or --all".to_string());
            }
            let cleanup = if yes_flag {
                self.environment_service().cleanup_all()?
            } else {
                self.environment_service().cleanup_all_preview()?
            };
            if json_flag {
                self.print_json(&cleanup)?;
                return Ok(0);
            }

            self.stdout_lines(render::env::env_cleanup_batch(&cleanup, profile));
            return Ok(0);
        }

        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let cleanup = if yes_flag {
            self.environment_service().cleanup(name)?
        } else {
            self.environment_service().cleanup_preview(name)?
        };
        if json_flag {
            self.print_json(&cleanup)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_cleanup(&cleanup, profile));
        Ok(0)
    }

    pub(super) fn handle_env_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env list")?;
        Self::assert_no_extra_args(&args)?;

        let envs = self.environment_service().list()?;
        let summaries = envs
            .into_iter()
            .map(|meta| {
                self.environment_service()
                    .apply_effective_gateway_port(meta)
            })
            .collect::<Result<Vec<_>, _>>()?
            .iter()
            .map(summarize_env)
            .collect::<Vec<_>>();
        if json_flag {
            self.print_json(&summaries)?;
            return Ok(0);
        }
        self.stdout_lines(render::env::env_list(&summaries, profile));
        Ok(0)
    }

    pub(super) fn handle_env_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env show")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.environment_service().get(name)?;
        let summary = summarize_env(&meta);
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_show(
            &summary,
            profile,
            &self.command_example(),
        )?);
        Ok(0)
    }

    pub(super) fn handle_env_status(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "env status")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let status = self.environment_service().status(name)?;
        if json_flag {
            self.print_json(&status)?;
            return Ok(0);
        }
        self.stdout_lines(render::env::env_status(
            &status,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    fn handle_env_snapshot_create(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env snapshot create")?;
        let (args, label) = Self::consume_option(args, "--label")?;
        let label = Self::require_option_value(label, "--label")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let snapshot = self.with_progress(format!("Creating snapshot for {name}"), || {
            self.environment_service()
                .create_snapshot(CreateEnvSnapshotOptions {
                    env_name: name.clone(),
                    label,
                })
        })?;

        if json_flag {
            self.print_json(&snapshot)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_snapshot_created(&snapshot, profile));
        Ok(0)
    }

    fn handle_env_snapshot_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env snapshot show")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        let Some(snapshot_id) = args.get(1) else {
            return Err("snapshot id is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let snapshot = self.environment_service().get_snapshot(name, snapshot_id)?;
        if json_flag {
            self.print_json(&snapshot)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_snapshot_show(&snapshot, profile)?);
        Ok(0)
    }

    fn handle_env_snapshot_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env snapshot list")?;
        let (args, all) = Self::consume_flag(args, "--all");
        let env_name = if all {
            if !args.is_empty() {
                return Err("env snapshot list accepts either <name> or --all".to_string());
            }
            None
        } else {
            let Some(name) = args.first() else {
                return Err("environment name is required".to_string());
            };
            Self::assert_no_extra_args(&args[1..])?;
            Some(name.as_str())
        };

        let snapshots = self.environment_service().list_snapshots(env_name)?;
        if json_flag {
            self.print_json(&snapshots)?;
            return Ok(0);
        }
        self.stdout_lines(render::env::env_snapshot_list(&snapshots, profile)?);
        Ok(0)
    }

    fn handle_env_snapshot_restore(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env snapshot restore")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        let Some(snapshot_id) = args.get(1) else {
            return Err("snapshot id is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let restored = self.with_progress(
            format!("Restoring snapshot {snapshot_id} for {name}"),
            || {
                self.environment_service()
                    .restore_snapshot(RestoreEnvSnapshotOptions {
                        env_name: name.clone(),
                        snapshot_id: snapshot_id.clone(),
                    })
            },
        )?;

        if json_flag {
            self.print_json(&restored)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_snapshot_restored(&restored, profile));
        Ok(0)
    }

    fn handle_env_snapshot_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env snapshot remove")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        let Some(snapshot_id) = args.get(1) else {
            return Err("snapshot id is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let removed = self
            .environment_service()
            .remove_snapshot(RemoveEnvSnapshotOptions {
                env_name: name.clone(),
                snapshot_id: snapshot_id.clone(),
            })?;

        if json_flag {
            self.print_json(&removed)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_snapshot_removed(&removed, profile));
        Ok(0)
    }

    fn handle_env_snapshot_prune(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env snapshot prune")?;
        let (args, yes) = Self::consume_flag(args, "--yes");
        let (args, all) = Self::consume_flag(args, "--all");
        let (args, keep_raw) = Self::consume_option(args, "--keep")?;
        let keep = match keep_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--keep")? as usize),
            None => None,
        };
        let (args, older_than_raw) = Self::consume_option(args, "--older-than")?;
        let older_than_days = match older_than_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--older-than")? as i64),
            None => None,
        };

        let env_name = if all {
            if !args.is_empty() {
                return Err("env snapshot prune accepts either <name> or --all".to_string());
            }
            None
        } else {
            let Some(name) = args.first() else {
                return Err("environment name is required".to_string());
            };
            Self::assert_no_extra_args(&args[1..])?;
            Some(name.as_str())
        };

        if keep.is_none() && older_than_days.is_none() {
            return Err("env snapshot prune requires --keep or --older-than".to_string());
        }

        let scope_label = env_name.unwrap_or("all");
        if !yes {
            let candidates = self.environment_service().prune_snapshot_candidates(
                env_name,
                keep,
                older_than_days,
            )?;
            if json_flag {
                self.print_json(&serde_json::json!({
                    "apply": false,
                    "scope": scope_label,
                    "keep": keep,
                    "olderThanDays": older_than_days,
                    "count": candidates.len(),
                    "candidates": candidates,
                }))?;
                return Ok(0);
            }

            self.stdout_lines(render::env::env_snapshot_prune_preview(
                scope_label,
                &candidates,
                profile,
            )?);
            return Ok(0);
        }

        let removed =
            self.environment_service()
                .prune_snapshots(env_name, keep, older_than_days)?;

        if json_flag {
            self.print_json(&serde_json::json!({
                "apply": true,
                "scope": scope_label,
                "keep": keep,
                "olderThanDays": older_than_days,
                "count": removed.len(),
                "removed": removed,
            }))?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_snapshot_pruned(&removed, profile));
        Ok(0)
    }

    pub(super) fn dispatch_env_snapshot_command(&self, args: Vec<String>) -> Result<i32, String> {
        let Some(action) = args.first() else {
            return Err("env snapshot command is required".to_string());
        };

        match action.as_str() {
            "create" => self.handle_env_snapshot_create(args[1..].to_vec()),
            "show" => self.handle_env_snapshot_show(args[1..].to_vec()),
            "list" => self.handle_env_snapshot_list(args[1..].to_vec()),
            "restore" => self.handle_env_snapshot_restore(args[1..].to_vec()),
            "remove" => self.handle_env_snapshot_remove(args[1..].to_vec()),
            "prune" => self.handle_env_snapshot_prune(args[1..].to_vec()),
            _ => Err(format!("unknown env snapshot command: {action}")),
        }
    }

    pub(super) fn dispatch_env_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "create" => self.handle_env_create(args),
            "clone" => self.handle_env_clone(args),
            "export" => self.handle_env_export(args),
            "import" => self.handle_env_import(args),
            "snapshot" => self.dispatch_env_snapshot_command(args),
            "list" => self.handle_env_list(args),
            "show" => self.handle_env_show(args),
            "status" => self.handle_env_status(args),
            "doctor" => self.handle_env_doctor(args),
            "cleanup" => self.handle_env_cleanup(args),
            "use" => self.handle_env_use(args),
            "exec" => self.handle_env_exec(args),
            "resolve" => self.handle_env_resolve(args),
            "run" => self.handle_env_run(args),
            "set-runtime" => self.handle_env_set_runtime(args),
            "set-launcher" => self.handle_env_set_launcher(args),
            "protect" => self.handle_env_protect(args),
            "destroy" => self.handle_env_destroy(args),
            "remove" | "rm" => self.handle_env_remove(args),
            "prune" => self.handle_env_prune(args),
            _ => Err(format!("unknown env command: {action}")),
        }
    }
}

impl Cli {
    fn build_env_destroy_summary(
        &self,
        name: &str,
        apply: bool,
        force: bool,
    ) -> Result<EnvDestroySummary, String> {
        let env_meta = self.environment_service().get(name)?;
        let service = self.service_service().status(name)?;
        let snapshots = self.environment_service().list_snapshots(Some(name))?;
        let mut blockers = Vec::new();
        let process_candidates = self
            .destroy_process_candidates(&env_meta)
            .unwrap_or_default();

        if env_meta.protected && !force {
            blockers.push("env is protected; re-run with --force to destroy it".to_string());
        }
        let mut steps = Vec::new();
        if !snapshots.is_empty() {
            steps.push(EnvDestroyStepSummary {
                kind: "snapshots".to_string(),
                description: format!("remove {} env snapshot(s)", snapshots.len()),
            });
        }
        if service.installed || service.loaded || service.running {
            steps.push(EnvDestroyStepSummary {
                kind: "service".to_string(),
                description: "disable env gateway in the OCM background service".to_string(),
            });
        }
        if !process_candidates.is_empty() {
            steps.push(EnvDestroyStepSummary {
                kind: "processes".to_string(),
                description: "terminate live OpenClaw processes for the env".to_string(),
            });
        }
        if let Some(dev) = env_meta.dev.as_ref() {
            steps.push(EnvDestroyStepSummary {
                kind: "worktree".to_string(),
                description: format!("remove dev worktree {}", dev.worktree_root),
            });
        }
        steps.push(EnvDestroyStepSummary {
            kind: "env".to_string(),
            description: "remove env root and metadata".to_string(),
        });

        Ok(EnvDestroySummary {
            env_name: env_meta.name,
            root: env_meta.root,
            dev_worktree: env_meta.dev.as_ref().map(|dev| dev.worktree_root.clone()),
            protected: env_meta.protected,
            apply,
            force,
            snapshot_count: snapshots.len(),
            service_installed: service.installed,
            service_loaded: service.loaded,
            service_running: service.running,
            service_label: "ocm".to_string(),
            blockers,
            steps,
            snapshots_removed: 0,
            service_uninstalled: false,
            processes_terminated: 0,
            worktree_removed: false,
            removed: false,
        })
    }

    fn terminate_env_processes(&self, meta: &EnvMeta) -> Result<usize, String> {
        terminate_associated_processes(meta)
    }

    fn destroy_process_candidates(&self, meta: &EnvMeta) -> Result<Vec<u32>, String> {
        associated_process_pids(meta)
    }

    pub(super) fn handle_env_use(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, shell_name) = Self::consume_option(args, "--shell")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self
            .environment_service()
            .apply_effective_gateway_port(self.environment_service().touch(name)?)?;
        let shell = resolve_shell_name(shell_name.as_deref(), &self.env);
        print!("{}", render_use_script(&meta, &shell));
        Ok(0)
    }

    pub(super) fn handle_env_exec(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_command_separator(&before, "env exec requires -- before the command")?;
        if after.is_empty() {
            return Err("env exec requires a command after --".to_string());
        }

        let meta = self
            .environment_service()
            .apply_effective_gateway_port(self.environment_service().touch(name)?)?;
        run_direct(
            &after[0],
            &after[1..],
            &build_openclaw_env(&meta, &self.env),
            &self.cwd,
        )
    }

    pub(super) fn handle_env_resolve(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let (before, json_flag, profile) =
            self.consume_human_output_flags(before, "env resolve")?;
        let (before, runtime_override) = Self::consume_option(before, "--runtime")?;
        let runtime_override = Self::require_option_value(runtime_override, "--runtime")?;
        let (before, launcher_override) = Self::consume_option(before, "--launcher")?;
        let launcher_override = Self::require_option_value(launcher_override, "--launcher")?;
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&before[1..])?;

        let summary = self
            .environment_service()
            .resolve(name, runtime_override, launcher_override, &after)?
            .into_summary();

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::env::env_resolved(&summary, profile));
        Ok(0)
    }

    pub(super) fn handle_env_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let (before, runtime_override) = Self::consume_option(before, "--runtime")?;
        let runtime_override = Self::require_option_value(runtime_override, "--runtime")?;
        let (before, launcher_override) = Self::consume_option(before, "--launcher")?;
        let launcher_override = Self::require_option_value(launcher_override, "--launcher")?;
        let Some(name) = before.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_command_separator(&before, "env run requires -- before OpenClaw arguments")?;

        let resolved = self.environment_service().resolve_run(
            name,
            runtime_override,
            launcher_override,
            &after,
        )?;
        match resolved {
            crate::env::ResolvedExecution::Launcher {
                env,
                command,
                run_dir,
                ..
            } => run_shell(&command, &build_openclaw_env(&env, &self.env), &run_dir),
            crate::env::ResolvedExecution::Runtime {
                env,
                program,
                program_args,
                run_dir,
                ..
            } => run_direct(
                &program,
                &program_args,
                &build_openclaw_env(&env, &self.env),
                &run_dir,
            ),
            crate::env::ResolvedExecution::Dev {
                env,
                program,
                program_args,
                run_dir,
                ..
            } => run_direct(
                &program,
                &program_args,
                &build_openclaw_env(&env, &self.env),
                &run_dir,
            ),
        }
    }

    pub(super) fn handle_env_set_runtime(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env set-runtime")?;
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;

        if args.is_empty() {
            return Err(format!(
                "usage: {} env set-runtime <env> <runtime|none>\n       {} env set-runtime <env> (--version <version> | --channel <channel>)",
                self.command_example(),
                self.command_example()
            ));
        }
        let name = &args[0];
        let runtime_name = args.get(1).cloned();
        let extra_args = if runtime_name.is_some() {
            &args[2..]
        } else {
            &args[1..]
        };
        Self::assert_no_extra_args(extra_args)?;
        if matches!(runtime_name.as_deref(), Some(name) if name.eq_ignore_ascii_case("none"))
            && (version.is_some() || channel.is_some())
        {
            return Err(
                "env set-runtime accepts only one runtime source: --runtime, --version, or --channel"
                    .to_string(),
            );
        }

        let validated = match runtime_name {
            Some(runtime_name) if runtime_name.eq_ignore_ascii_case("none") => runtime_name,
            Some(runtime_name) => self
                .environment_service()
                .resolve_runtime_binding_request(
                    Some(validate_name(&runtime_name, "Runtime name")?),
                    version,
                    channel,
                    "env set-runtime",
                )?
                .unwrap_or(runtime_name),
            None if version.is_some() || channel.is_some() => self
                .with_progress(format!("Preparing OpenClaw runtime for {name}"), || {
                    self.environment_service().resolve_runtime_binding_request(
                        None,
                        version,
                        channel,
                        "env set-runtime",
                    )
                })?
                .ok_or_else(|| {
                    "env set-runtime requires a runtime, none, --version, or --channel".to_string()
                })?,
            None => {
                return Err(format!(
                    "usage: {} env set-runtime <env> <runtime|none>\n       {} env set-runtime <env> (--version <version> | --channel <channel>)",
                    self.command_example(),
                    self.command_example()
                ));
            }
        };
        let meta = self.environment_service().set_runtime(name, &validated)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }
        let default_runtime = meta.default_runtime.unwrap_or_else(|| "none".to_string());
        self.stdout_lines(render::env::env_runtime_updated(
            &meta.name,
            &default_runtime,
            profile,
        ));
        Ok(0)
    }

    pub(super) fn handle_env_set_launcher(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "env set-launcher")?;
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env set-launcher <env> <launcher|none>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let launcher_name = &args[1];
        Self::assert_no_extra_args(&args[2..])?;

        let validated = if launcher_name.eq_ignore_ascii_case("none") {
            launcher_name.to_string()
        } else {
            validate_name(launcher_name, "Launcher name")?
        };
        let meta = self.environment_service().set_launcher(name, &validated)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }
        let default_launcher = meta.default_launcher.unwrap_or_else(|| "none".to_string());
        self.stdout_lines(render::env::env_launcher_updated(
            &meta.name,
            &default_launcher,
            profile,
        ));
        Ok(0)
    }
}

#[cfg(unix)]
fn terminate_associated_processes(meta: &EnvMeta) -> Result<usize, String> {
    let pids = associated_process_pids(meta)?;
    if pids.is_empty() {
        return Ok(0);
    }

    for pid in &pids {
        terminate_pid(*pid)?;
    }

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if associated_process_pids(meta)?.is_empty() {
            return Ok(pids.len());
        }
        sleep(Duration::from_millis(100));
    }

    Err("failed to terminate all live env processes".to_string())
}

#[cfg(not(unix))]
fn terminate_associated_processes(_meta: &EnvMeta) -> Result<usize, String> {
    Ok(0)
}

#[cfg(unix)]
fn associated_process_pids(meta: &EnvMeta) -> Result<Vec<u32>, String> {
    let processes = process_table()?;
    let cwd_map = process_cwd_map(&processes)?;
    let parent_map = processes
        .iter()
        .map(|process| (process.pid, process.ppid))
        .collect::<std::collections::HashMap<_, _>>();
    let mut children_map = std::collections::HashMap::<u32, Vec<u32>>::new();
    for process in &processes {
        children_map
            .entry(process.ppid)
            .or_default()
            .push(process.pid);
    }

    let mut seeds = BTreeSet::new();
    for process in &processes {
        let command = process_command(process.pid)?;
        if interactive_shell_command(&command) {
            continue;
        }
        if process_belongs_to_env(process.pid, &command, meta, &cwd_map) {
            seeds.insert(process.pid);
        }
    }

    if seeds.is_empty() {
        return Ok(Vec::new());
    }

    let mut related = seeds;
    let mut queue = related.iter().copied().collect::<Vec<_>>();
    while let Some(pid) = queue.pop() {
        if let Some(children) = children_map.get(&pid) {
            for child in children {
                let command = process_command(*child)?;
                if interactive_shell_command(&command) {
                    continue;
                }
                if related.insert(*child) {
                    queue.push(*child);
                }
            }
        }
    }

    let depths = process_depths(&parent_map);
    let mut ordered = related.into_iter().collect::<Vec<_>>();
    ordered.sort_by_key(|pid| std::cmp::Reverse(depths.get(pid).copied().unwrap_or(0)));
    Ok(ordered)
}

#[cfg(unix)]
fn process_belongs_to_env(
    pid: u32,
    command: &str,
    meta: &EnvMeta,
    cwd_map: &std::collections::HashMap<u32, String>,
) -> bool {
    let paths = derive_env_paths(Path::new(&meta.root));
    let mut markers = vec![
        meta.root.clone(),
        paths.state_dir.to_string_lossy().into_owned(),
        paths.config_path.to_string_lossy().into_owned(),
        paths.workspace_dir.to_string_lossy().into_owned(),
    ]
    .into_iter()
    .map(|value| normalize_process_path(&value))
    .collect::<Vec<_>>();
    if let Some(dev) = meta.dev.as_ref() {
        markers.push(normalize_process_path(&dev.worktree_root));
    }

    let command = normalize_process_path(command);
    if markers.iter().any(|marker| command.contains(marker)) {
        return true;
    }

    if let Some(cwd) = cwd_map.get(&pid)
        && markers
            .iter()
            .any(|marker| normalize_process_path(&cwd).starts_with(marker))
    {
        return true;
    }

    false
}

#[cfg(unix)]
#[derive(Clone, Debug)]
struct ProcessEntry {
    pid: u32,
    ppid: u32,
}

#[cfg(unix)]
fn process_table() -> Result<Vec<ProcessEntry>, String> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid="])
        .output()
        .map_err(|error| format!("failed to inspect running processes: {error}"))?;
    if !output.status.success() {
        return Err("failed to inspect running processes".to_string());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid = parts.next()?.parse::<u32>().ok()?;
            let ppid = parts.next()?.parse::<u32>().ok()?;
            Some(ProcessEntry { pid, ppid })
        })
        .collect())
}

#[cfg(unix)]
fn process_depths(
    parent_map: &std::collections::HashMap<u32, u32>,
) -> std::collections::HashMap<u32, usize> {
    fn depth_of(
        pid: u32,
        parent_map: &std::collections::HashMap<u32, u32>,
        cache: &mut std::collections::HashMap<u32, usize>,
    ) -> usize {
        if let Some(depth) = cache.get(&pid) {
            return *depth;
        }
        let depth = match parent_map.get(&pid).copied() {
            Some(parent) if parent > 1 && parent != pid => depth_of(parent, parent_map, cache) + 1,
            _ => 0,
        };
        cache.insert(pid, depth);
        depth
    }

    let mut cache = std::collections::HashMap::new();
    for pid in parent_map.keys().copied().collect::<Vec<_>>() {
        let _ = depth_of(pid, parent_map, &mut cache);
    }
    cache
}

#[cfg(unix)]
fn interactive_shell_command(command: &str) -> bool {
    let command = command.trim_start_matches('-');
    command.ends_with("/zsh")
        || command.ends_with("/bash")
        || command.ends_with("/fish")
        || command == "zsh"
        || command == "bash"
        || command == "fish"
}

#[cfg(unix)]
fn normalize_process_path(value: &str) -> String {
    value.strip_prefix("/private").unwrap_or(value).to_string()
}

#[cfg(target_os = "linux")]
fn process_cwd_map(
    processes: &[ProcessEntry],
) -> Result<std::collections::HashMap<u32, String>, String> {
    let mut cwd_map = std::collections::HashMap::new();
    for process in processes {
        match fs::read_link(format!("/proc/{}/cwd", process.pid)) {
            Ok(path) => {
                cwd_map.insert(process.pid, path.to_string_lossy().into_owned());
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) => {}
            Err(error) => {
                return Err(format!(
                    "failed to inspect process cwd for pid {}: {error}",
                    process.pid
                ));
            }
        }
    }
    Ok(cwd_map)
}

#[cfg(all(unix, not(target_os = "linux")))]
fn process_cwd_map(
    _processes: &[ProcessEntry],
) -> Result<std::collections::HashMap<u32, String>, String> {
    let output = Command::new("lsof")
        .args(["-b", "-w", "-a", "-d", "cwd", "-Fpn"])
        .output()
        .map_err(|error| format!("failed to inspect process cwd: {error}"))?;
    if !output.status.success() {
        return Ok(std::collections::HashMap::new());
    }

    let mut cwd_map = std::collections::HashMap::new();
    let mut current_pid = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(pid) = line.strip_prefix('p') {
            current_pid = pid.parse::<u32>().ok();
        } else if let Some(path) = line.strip_prefix('n')
            && let Some(pid) = current_pid
        {
            cwd_map.insert(pid, path.trim().to_string());
        }
    }
    Ok(cwd_map)
}

#[cfg(unix)]
fn process_command(pid: u32) -> Result<String, String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .map_err(|error| format!("failed to inspect process command for pid {pid}: {error}"))?;
    if !output.status.success() {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(unix)]
fn terminate_pid(pid: u32) -> Result<(), String> {
    let pid_text = pid.to_string();
    let _ = quiet_kill("-TERM", &pid_text);
    let deadline = Instant::now() + Duration::from_millis(750);
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return Ok(());
        }
        sleep(Duration::from_millis(50));
    }
    let _ = quiet_kill("-KILL", &pid_text);
    let deadline = Instant::now() + Duration::from_secs(1);
    while Instant::now() < deadline {
        if !process_alive(pid) {
            return Ok(());
        }
        sleep(Duration::from_millis(50));
    }
    Err(format!("failed to terminate pid {pid}"))
}

#[cfg(unix)]
fn quiet_kill(signal: &str, pid: &str) -> Result<(), String> {
    Command::new("kill")
        .args([signal, pid])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|_| ())
        .map_err(|error| format!("failed to run kill {signal} {pid}: {error}"))
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    let Ok(status_output) = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "stat="])
        .output()
    else {
        return false;
    };
    if !status_output.status.success() {
        return false;
    }
    let status = String::from_utf8_lossy(&status_output.stdout);
    if status.trim().is_empty() || status.contains('Z') {
        return false;
    }

    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}
