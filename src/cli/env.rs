use std::collections::BTreeMap;
use std::path::Path;

use super::Cli;
use crate::env::{
    CloneEnvironmentOptions, CreateEnvSnapshotOptions, CreateEnvironmentOptions,
    ExportEnvironmentOptions, ImportEnvironmentOptions, RemoveEnvSnapshotOptions,
    RestoreEnvSnapshotOptions,
};
use crate::infra::process::{run_direct, run_shell};
use crate::infra::shell::{build_openclaw_env, render_use_script, resolve_shell_name};
use crate::store::{derive_env_paths, validate_name};
use crate::store::summarize_env;
use crate::types::EnvSummary;

impl Cli {
    pub(super) fn handle_env_protect(&self, args: Vec<String>) -> Result<i32, String> {
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
        self.stdout_line(format!(
            "Updated env {}: protected={}",
            meta.name, meta.protected
        ));
        Ok(0)
    }

    pub(super) fn handle_env_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, force) = Self::consume_flag(args, "--force");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.environment_service().remove(name, force)?;
        self.stdout_line(format!("Removed env {}", meta.name));
        self.stdout_line(format!(
            "  root: {}",
            derive_env_paths(Path::new(&meta.root)).root.display()
        ));
        Ok(0)
    }

    pub(super) fn handle_env_prune(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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

        if !yes {
            if json_flag {
                let summaries = candidates.iter().map(summarize_env).collect::<Vec<_>>();
                self.print_json(&serde_json::json!({
                    "apply": false,
                    "olderThanDays": older_than_days,
                    "count": summaries.len(),
                    "candidates": summaries,
                }))?;
                return Ok(0);
            }

            self.stdout_line(format!(
                "Prune preview ({}d): {} candidate(s)",
                older_than_days,
                candidates.len()
            ));
            for meta in candidates {
                self.stdout_line(format!(
                    "  {}  {}",
                    meta.name,
                    derive_env_paths(Path::new(&meta.root)).root.display()
                ));
            }
            self.stdout_line("Re-run with --yes to remove them.");
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

        self.stdout_line(format!("Pruned {} environment(s).", removed.len()));
        for summary in removed {
            self.stdout_line(format!("  {}  {}", summary.name, summary.root));
        }
        Ok(0)
    }

    pub(super) fn handle_env_create(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, protect) = Self::consume_flag(args, "--protect");
        let (args, root) = Self::consume_option(args, "--root")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--port")?),
            _ => None,
        };
        let (args, runtime_name) = Self::consume_option(args, "--runtime")?;
        let runtime_name = Self::require_option_value(runtime_name, "--runtime")?;
        let (args, launcher_name) = Self::consume_option(args, "--launcher")?;
        let launcher_name = Self::require_option_value(launcher_name, "--launcher")?;

        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self
            .environment_service()
            .create(CreateEnvironmentOptions {
                name: name.clone(),
                root,
                gateway_port,
                default_runtime: runtime_name,
                default_launcher: launcher_name,
                protected: protect,
            })?;

        if json_flag {
            self.print_json(&summarize_env(&meta))?;
            return Ok(0);
        }

        let summary = summarize_env(&meta);
        self.stdout_line(format!("Created env {}", summary.name));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  openclaw home: {}", summary.openclaw_home));
        self.stdout_line(format!("  workspace: {}", summary.workspace_dir));
        if let Some(port) = summary.gateway_port {
            self.stdout_line(format!("  gateway port: {port}"));
        }
        if let Some(runtime) = summary.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    pub(super) fn handle_env_clone(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, root) = Self::consume_option(args, "--root")?;
        let Some(source_name) = args.first() else {
            return Err("source environment name is required".to_string());
        };
        let Some(target_name) = args.get(1) else {
            return Err("target environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let meta = self.environment_service().clone(CloneEnvironmentOptions {
            source_name: source_name.clone(),
            name: target_name.clone(),
            root,
        })?;

        if json_flag {
            self.print_json(&summarize_env(&meta))?;
            return Ok(0);
        }

        let summary = summarize_env(&meta);
        self.stdout_line(format!("Cloned env {} from {}", summary.name, source_name));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  openclaw home: {}", summary.openclaw_home));
        self.stdout_line(format!("  workspace: {}", summary.workspace_dir));
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    pub(super) fn handle_env_export(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, output) = Self::consume_option(args, "--output")?;
        let output = Self::require_option_value(output, "--output")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self
            .environment_service()
            .export(ExportEnvironmentOptions {
                name: name.clone(),
                output,
            })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_line(format!("Exported env {}", summary.name));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  archive: {}", summary.archive_path));
        if let Some(runtime) = summary.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        if summary.protected {
            self.stdout_line("  protected: true");
        }
        Ok(0)
    }

    pub(super) fn handle_env_import(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, name) = Self::consume_option(args, "--name")?;
        let name = Self::require_option_value(name, "--name")?;
        let (args, root) = Self::consume_option(args, "--root")?;
        let root = Self::require_option_value(root, "--root")?;
        let Some(archive) = args.first() else {
            return Err("archive path is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self
            .environment_service()
            .import(ImportEnvironmentOptions {
                archive: archive.clone(),
                name,
                root,
            })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_line(format!(
            "Imported env {} from {}",
            summary.name, summary.source_name
        ));
        self.stdout_line(format!("  root: {}", summary.root));
        self.stdout_line(format!("  archive: {}", summary.archive_path));
        if let Some(runtime) = summary.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        if summary.protected {
            self.stdout_line("  protected: true");
        }
        self.stdout_line(format!(
            "  activate: eval \"$({} env use {})\"",
            self.command_example(),
            summary.name
        ));
        Ok(0)
    }

    pub(super) fn handle_env_doctor(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let doctor = self.environment_service().doctor(name)?;
        if json_flag {
            self.print_json(&doctor)?;
            return Ok(0);
        }

        self.stdout_line(format!("envName: {}", doctor.env_name));
        self.stdout_line(format!("root: {}", doctor.root));
        self.stdout_line(format!("healthy: {}", doctor.healthy));
        self.stdout_line(format!("rootStatus: {}", doctor.root_status));
        self.stdout_line(format!("markerStatus: {}", doctor.marker_status));
        self.stdout_line(format!("runtimeStatus: {}", doctor.runtime_status));
        self.stdout_line(format!("launcherStatus: {}", doctor.launcher_status));
        self.stdout_line(format!("resolutionStatus: {}", doctor.resolution_status));
        if let Some(runtime) = doctor.default_runtime {
            self.stdout_line(format!("defaultRuntime: {runtime}"));
        }
        if let Some(launcher) = doctor.default_launcher {
            self.stdout_line(format!("defaultLauncher: {launcher}"));
        }
        if let Some(kind) = doctor.resolved_kind {
            self.stdout_line(format!("resolvedKind: {kind}"));
        }
        if let Some(name) = doctor.resolved_name {
            self.stdout_line(format!("resolvedName: {name}"));
        }
        for issue in doctor.issues {
            self.stdout_line(format!("issue: {issue}"));
        }
        Ok(0)
    }

    pub(super) fn handle_env_cleanup(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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

            if cleanup.apply {
                self.stdout_line(format!("Applied cleanup (--all): {} env(s)", cleanup.count));
            } else {
                self.stdout_line(format!("Cleanup preview (--all): {} env(s)", cleanup.count));
            }
            for result in cleanup.results {
                self.stdout_line(format!("  {}", result.env_name));
                self.stdout_line(format!("    root: {}", result.root));
                if result.apply {
                    self.stdout_line(format!("    applied fixes: {}", result.actions.len()));
                } else {
                    self.stdout_line(format!("    safe fixes: {}", result.actions.len()));
                }
                for action in result.actions {
                    self.stdout_line(format!("    {}: {}", action.kind, action.description));
                }
            }
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

        if cleanup.apply {
            self.stdout_line(format!("Applied cleanup for env {}", cleanup.env_name));
        } else {
            self.stdout_line(format!("Cleanup preview for env {}", cleanup.env_name));
        }
        self.stdout_line(format!("  root: {}", cleanup.root));
        if cleanup.apply {
            self.stdout_line(format!("  applied fixes: {}", cleanup.actions.len()));
        } else {
            self.stdout_line(format!("  safe fixes: {}", cleanup.actions.len()));
        }
        for action in &cleanup.actions {
            self.stdout_line(format!("  {}: {}", action.kind, action.description));
        }
        if cleanup.apply {
            if let Some(healthy_after) = cleanup.healthy_after {
                self.stdout_line(format!("  healthy after: {healthy_after}"));
            }
            if let Some(issues_after) = cleanup.issues_after {
                for issue in issues_after {
                    self.stdout_line(format!("  issue: {issue}"));
                }
            }
        } else {
            for issue in cleanup.issues_before {
                self.stdout_line(format!("  issue: {issue}"));
            }
            if !cleanup.actions.is_empty() {
                self.stdout_line("  re-run with --yes to apply them");
            }
        }
        Ok(0)
    }

    pub(super) fn handle_env_repair_marker(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let repaired = self.environment_service().repair_marker(name)?;
        if json_flag {
            self.print_json(&repaired)?;
            return Ok(0);
        }

        self.stdout_line(format!("Repaired marker for env {}", repaired.env_name));
        self.stdout_line(format!("  root: {}", repaired.root));
        self.stdout_line(format!("  marker: {}", repaired.marker_path));
        Ok(0)
    }

    pub(super) fn handle_env_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let envs = self.environment_service().list()?;
        let summaries = envs.iter().map(summarize_env).collect::<Vec<_>>();
        if json_flag {
            self.print_json(&summaries)?;
            return Ok(0);
        }
        if summaries.is_empty() {
            self.stdout_line("No environments.");
            return Ok(0);
        }
        for summary in summaries {
            let mut bits = vec![summary.name, summary.root];
            if let Some(runtime) = summary.default_runtime {
                bits.push(format!("runtime={runtime}"));
            }
            if let Some(launcher) = summary.default_launcher {
                bits.push(format!("launcher={launcher}"));
            }
            if let Some(port) = summary.gateway_port {
                bits.push(format!("port={port}"));
            }
            if summary.protected {
                bits.push("protected".to_string());
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    pub(super) fn handle_env_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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

        let mut lines = BTreeMap::new();
        lines.insert("name".to_string(), summary.name);
        lines.insert("root".to_string(), summary.root);
        lines.insert("openclawHome".to_string(), summary.openclaw_home);
        lines.insert("stateDir".to_string(), summary.state_dir);
        lines.insert("configPath".to_string(), summary.config_path);
        lines.insert("workspaceDir".to_string(), summary.workspace_dir);
        lines.insert("protected".to_string(), summary.protected.to_string());
        lines.insert(
            "createdAt".to_string(),
            summary
                .created_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
        );
        if let Some(port) = summary.gateway_port {
            lines.insert("gatewayPort".to_string(), port.to_string());
        }
        if let Some(runtime) = summary.default_runtime {
            lines.insert("defaultRuntime".to_string(), runtime);
        }
        if let Some(launcher) = summary.default_launcher {
            lines.insert("defaultLauncher".to_string(), launcher);
        }
        if let Some(last_used_at) = summary.last_used_at {
            lines.insert(
                "lastUsedAt".to_string(),
                last_used_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .map_err(|error| error.to_string())?,
            );
        }

        for (key, value) in lines {
            self.stdout_line(format!("{key}: {value}"));
        }
        Ok(0)
    }

    pub(super) fn handle_env_status(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let status = self.environment_service().status(name)?;
        if json_flag {
            self.print_json(&status)?;
            return Ok(0);
        }
        self.stdout_line(format!("envName: {}", status.env_name));
        self.stdout_line(format!("root: {}", status.root));
        if let Some(runtime) = status.default_runtime {
            self.stdout_line(format!("defaultRuntime: {runtime}"));
        }
        if let Some(launcher) = status.default_launcher {
            self.stdout_line(format!("defaultLauncher: {launcher}"));
        }
        if let Some(kind) = status.resolved_kind {
            self.stdout_line(format!("resolvedKind: {kind}"));
        }
        if let Some(name) = status.resolved_name {
            self.stdout_line(format!("resolvedName: {name}"));
        }
        if let Some(binary_path) = status.binary_path {
            self.stdout_line(format!("binaryPath: {binary_path}"));
        }
        if let Some(command) = status.command {
            self.stdout_line(format!("command: {command}"));
        }
        if let Some(run_dir) = status.run_dir {
            self.stdout_line(format!("runDir: {run_dir}"));
        }
        if let Some(source_kind) = status.runtime_source_kind {
            self.stdout_line(format!("runtimeSourceKind: {source_kind}"));
        }
        if let Some(release_version) = status.runtime_release_version {
            self.stdout_line(format!("runtimeReleaseVersion: {release_version}"));
        }
        if let Some(release_channel) = status.runtime_release_channel {
            self.stdout_line(format!("runtimeReleaseChannel: {release_channel}"));
        }
        if let Some(runtime_health) = status.runtime_health {
            self.stdout_line(format!("runtimeHealth: {runtime_health}"));
        }
        if let Some(issue) = status.issue {
            self.stdout_line(format!("issue: {issue}"));
        }
        Ok(0)
    }

    fn handle_env_snapshot_create(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, label) = Self::consume_option(args, "--label")?;
        let label = Self::require_option_value(label, "--label")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let snapshot = self
            .environment_service()
            .create_snapshot(CreateEnvSnapshotOptions {
                env_name: name.clone(),
                label,
            })?;

        if json_flag {
            self.print_json(&snapshot)?;
            return Ok(0);
        }

        self.stdout_line(format!(
            "Created snapshot {} for env {}",
            snapshot.id, snapshot.env_name
        ));
        self.stdout_line(format!("  archive: {}", snapshot.archive_path));
        self.stdout_line(format!("  root: {}", snapshot.source_root));
        if let Some(label) = snapshot.label.as_deref() {
            self.stdout_line(format!("  label: {label}"));
        }
        Ok(0)
    }

    fn handle_env_snapshot_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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

        self.stdout_line(format!("snapshotId: {}", snapshot.id));
        self.stdout_line(format!("envName: {}", snapshot.env_name));
        self.stdout_line(format!("archivePath: {}", snapshot.archive_path));
        self.stdout_line(format!("sourceRoot: {}", snapshot.source_root));
        if let Some(label) = snapshot.label {
            self.stdout_line(format!("label: {label}"));
        }
        if let Some(port) = snapshot.gateway_port {
            self.stdout_line(format!("gatewayPort: {port}"));
        }
        if let Some(runtime) = snapshot.default_runtime {
            self.stdout_line(format!("defaultRuntime: {runtime}"));
        }
        if let Some(launcher) = snapshot.default_launcher {
            self.stdout_line(format!("defaultLauncher: {launcher}"));
        }
        if snapshot.protected {
            self.stdout_line("protected: true");
        }
        self.stdout_line(format!(
            "createdAt: {}",
            snapshot
                .created_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?
        ));
        Ok(0)
    }

    fn handle_env_snapshot_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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
        if snapshots.is_empty() {
            self.stdout_line("No snapshots.");
            return Ok(0);
        }
        for snapshot in snapshots {
            let mut bits = vec![snapshot.id, snapshot.env_name];
            if let Some(label) = snapshot.label {
                bits.push(format!("label={label}"));
            }
            bits.push(snapshot.archive_path);
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    fn handle_env_snapshot_restore(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        let Some(snapshot_id) = args.get(1) else {
            return Err("snapshot id is required".to_string());
        };
        Self::assert_no_extra_args(&args[2..])?;

        let restored = self
            .environment_service()
            .restore_snapshot(RestoreEnvSnapshotOptions {
                env_name: name.clone(),
                snapshot_id: snapshot_id.clone(),
            })?;

        if json_flag {
            self.print_json(&restored)?;
            return Ok(0);
        }

        self.stdout_line(format!(
            "Restored env {} from snapshot {}",
            restored.env_name, restored.snapshot_id
        ));
        self.stdout_line(format!("  root: {}", restored.root));
        self.stdout_line(format!("  archive: {}", restored.archive_path));
        if let Some(label) = restored.label.as_deref() {
            self.stdout_line(format!("  label: {label}"));
        }
        if let Some(runtime) = restored.default_runtime.as_deref() {
            self.stdout_line(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = restored.default_launcher.as_deref() {
            self.stdout_line(format!("  launcher: {launcher}"));
        }
        if restored.protected {
            self.stdout_line("  protected: true");
        }
        Ok(0)
    }

    fn handle_env_snapshot_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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

        self.stdout_line(format!(
            "Removed snapshot {} for env {}",
            removed.snapshot_id, removed.env_name
        ));
        self.stdout_line(format!("  archive: {}", removed.archive_path));
        if let Some(label) = removed.label.as_deref() {
            self.stdout_line(format!("  label: {label}"));
        }
        Ok(0)
    }

    fn handle_env_snapshot_prune(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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
            let candidates =
                self.environment_service()
                    .prune_snapshot_candidates(env_name, keep, older_than_days)?;
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

            self.stdout_line(format!(
                "Snapshot prune preview ({scope_label}): {} candidate(s)",
                candidates.len()
            ));
            for candidate in candidates {
                let mut bits = vec![candidate.id, candidate.env_name];
                if let Some(label) = candidate.label {
                    bits.push(format!("label={label}"));
                }
                bits.push(candidate.archive_path);
                self.stdout_line(bits.join("  "));
            }
            self.stdout_line("Re-run with --yes to remove them.");
            return Ok(0);
        }

        let removed = self
            .environment_service()
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

        self.stdout_line(format!("Pruned {} snapshot(s).", removed.len()));
        for snapshot in removed {
            let mut bits = vec![snapshot.snapshot_id, snapshot.env_name];
            if let Some(label) = snapshot.label {
                bits.push(format!("label={label}"));
            }
            bits.push(snapshot.archive_path);
            self.stdout_line(format!("  {}", bits.join("  ")));
        }
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
            "repair-marker" => self.handle_env_repair_marker(args),
            "use" => self.handle_env_use(args),
            "exec" => self.handle_env_exec(args),
            "resolve" => self.handle_env_resolve(args),
            "run" => self.handle_env_run(args),
            "set-runtime" => self.handle_env_set_runtime(args),
            "set-launcher" => self.handle_env_set_launcher(args),
            "protect" => self.handle_env_protect(args),
            "remove" | "rm" => self.handle_env_remove(args),
            "prune" => self.handle_env_prune(args),
            _ => Err(format!("unknown env command: {action}")),
        }
    }
}

impl Cli {
    pub(super) fn handle_env_use(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, shell_name) = Self::consume_option(args, "--shell")?;
        let Some(name) = args.first() else {
            return Err("environment name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.environment_service().touch(name)?;
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

        let meta = self.environment_service().touch(name)?;
        run_direct(
            &after[0],
            &after[1..],
            &build_openclaw_env(&meta, &self.env),
            &self.cwd,
        )
    }

    pub(super) fn handle_env_resolve(&self, args: Vec<String>) -> Result<i32, String> {
        let (before, after) = Self::split_on_double_dash(&args);
        let (before, json_flag) = Self::consume_flag(before, "--json");
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

        self.stdout_line(format!("envName: {}", summary.env_name));
        self.stdout_line(format!("bindingKind: {}", summary.binding_kind));
        self.stdout_line(format!("bindingName: {}", summary.binding_name));
        if let Some(command) = summary.command {
            self.stdout_line(format!("command: {command}"));
        }
        if let Some(binary_path) = summary.binary_path {
            self.stdout_line(format!("binaryPath: {binary_path}"));
        }
        if !summary.forwarded_args.is_empty() {
            self.stdout_line(format!(
                "forwardedArgs: {}",
                summary.forwarded_args.join(" ")
            ));
        }
        self.stdout_line(format!("runDir: {}", summary.run_dir));
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
                binary_path,
                args,
                run_dir,
                ..
            } => run_direct(
                &binary_path,
                &args,
                &build_openclaw_env(&env, &self.env),
                &run_dir,
            ),
        }
    }

    pub(super) fn handle_env_set_runtime(&self, args: Vec<String>) -> Result<i32, String> {
        if args.len() < 2 {
            return Err(format!(
                "usage: {} env set-runtime <env> <runtime|none>",
                self.command_example()
            ));
        }
        let name = &args[0];
        let runtime_name = &args[1];
        Self::assert_no_extra_args(&args[2..])?;

        let validated = if runtime_name.eq_ignore_ascii_case("none") {
            runtime_name.to_string()
        } else {
            validate_name(runtime_name, "Runtime name")?
        };
        let meta = self.environment_service().set_runtime(name, &validated)?;
        let default_runtime = meta.default_runtime.unwrap_or_else(|| "none".to_string());
        self.stdout_line(format!(
            "Updated env {}: defaultRuntime={default_runtime}",
            meta.name
        ));
        Ok(0)
    }

    pub(super) fn handle_env_set_launcher(&self, args: Vec<String>) -> Result<i32, String> {
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
        let default_launcher = meta.default_launcher.unwrap_or_else(|| "none".to_string());
        self.stdout_line(format!(
            "Updated env {}: defaultLauncher={default_launcher}",
            meta.name
        ));
        Ok(0)
    }
}
