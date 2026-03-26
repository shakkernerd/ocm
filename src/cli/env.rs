use super::Cli;
use crate::types::{
    CreateEnvSnapshotOptions, RemoveEnvSnapshotOptions, RestoreEnvSnapshotOptions,
};

impl Cli {
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
}
