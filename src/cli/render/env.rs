use std::collections::BTreeMap;

use crate::env::{
    EnvCleanupBatchSummary, EnvCleanupSummary, EnvDoctorSummary, EnvExportSummary,
    EnvImportSummary, EnvMarkerRepairSummary, EnvSnapshotRemoveSummary, EnvSnapshotRestoreSummary,
    EnvSnapshotSummary, EnvStatusSummary, EnvSummary, ExecutionSummary,
};
use crate::infra::terminal::{Cell, Tone, render_table};

use super::{RenderProfile, format_key_value_lines, format_rfc3339};

pub fn env_protected(name: &str, protected: bool) -> Vec<String> {
    vec![format!("Updated env {name}: protected={protected}")]
}

pub fn env_removed(name: &str, root: &str) -> Vec<String> {
    vec![format!("Removed env {name}"), format!("  root: {root}")]
}

pub fn env_prune_preview(older_than_days: i64, candidates: &[EnvSummary]) -> Vec<String> {
    let mut lines = vec![format!(
        "Prune preview ({}d): {} candidate(s)",
        older_than_days,
        candidates.len()
    )];
    for summary in candidates {
        lines.push(format!("  {}  {}", summary.name, summary.root));
    }
    lines.push("Re-run with --yes to remove them.".to_string());
    lines
}

pub fn env_pruned(removed: &[EnvSummary]) -> Vec<String> {
    let mut lines = vec![format!("Pruned {} environment(s).", removed.len())];
    for summary in removed {
        lines.push(format!("  {}  {}", summary.name, summary.root));
    }
    lines
}

pub fn env_created(summary: &EnvSummary, command_example: &str) -> Vec<String> {
    let mut lines = vec![
        format!("Created env {}", summary.name),
        format!("  root: {}", summary.root),
        format!("  openclaw home: {}", summary.openclaw_home),
        format!("  workspace: {}", summary.workspace_dir),
    ];
    if let Some(port) = summary.gateway_port {
        lines.push(format!("  gateway port: {port}"));
    }
    if let Some(runtime) = summary.default_runtime.as_deref() {
        lines.push(format!("  runtime: {runtime}"));
    }
    if let Some(launcher) = summary.default_launcher.as_deref() {
        lines.push(format!("  launcher: {launcher}"));
    }
    lines.push(format!(
        "  activate: eval \"$({command_example} env use {})\"",
        summary.name
    ));
    lines
}

pub fn env_cloned(summary: &EnvSummary, source_name: &str, command_example: &str) -> Vec<String> {
    vec![
        format!("Cloned env {} from {}", summary.name, source_name),
        format!("  root: {}", summary.root),
        format!("  openclaw home: {}", summary.openclaw_home),
        format!("  workspace: {}", summary.workspace_dir),
        format!(
            "  activate: eval \"$({command_example} env use {})\"",
            summary.name
        ),
    ]
}

pub fn env_exported(summary: &EnvExportSummary) -> Vec<String> {
    let mut lines = vec![
        format!("Exported env {}", summary.name),
        format!("  root: {}", summary.root),
        format!("  archive: {}", summary.archive_path),
    ];
    if let Some(runtime) = summary.default_runtime.as_deref() {
        lines.push(format!("  runtime: {runtime}"));
    }
    if let Some(launcher) = summary.default_launcher.as_deref() {
        lines.push(format!("  launcher: {launcher}"));
    }
    if summary.protected {
        lines.push("  protected: true".to_string());
    }
    lines
}

pub fn env_imported(summary: &EnvImportSummary, command_example: &str) -> Vec<String> {
    let mut lines = vec![
        format!("Imported env {} from {}", summary.name, summary.source_name),
        format!("  root: {}", summary.root),
        format!("  archive: {}", summary.archive_path),
    ];
    if let Some(runtime) = summary.default_runtime.as_deref() {
        lines.push(format!("  runtime: {runtime}"));
    }
    if let Some(launcher) = summary.default_launcher.as_deref() {
        lines.push(format!("  launcher: {launcher}"));
    }
    if summary.protected {
        lines.push("  protected: true".to_string());
    }
    lines.push(format!(
        "  activate: eval \"$({command_example} env use {})\"",
        summary.name
    ));
    lines
}

pub fn env_doctor(doctor: &EnvDoctorSummary) -> Vec<String> {
    let mut lines = vec![
        format!("envName: {}", doctor.env_name),
        format!("root: {}", doctor.root),
        format!("healthy: {}", doctor.healthy),
        format!("rootStatus: {}", doctor.root_status),
        format!("markerStatus: {}", doctor.marker_status),
        format!("runtimeStatus: {}", doctor.runtime_status),
        format!("launcherStatus: {}", doctor.launcher_status),
        format!("resolutionStatus: {}", doctor.resolution_status),
    ];
    if let Some(runtime) = doctor.default_runtime.as_deref() {
        lines.push(format!("defaultRuntime: {runtime}"));
    }
    if let Some(launcher) = doctor.default_launcher.as_deref() {
        lines.push(format!("defaultLauncher: {launcher}"));
    }
    if let Some(kind) = doctor.resolved_kind.as_deref() {
        lines.push(format!("resolvedKind: {kind}"));
    }
    if let Some(name) = doctor.resolved_name.as_deref() {
        lines.push(format!("resolvedName: {name}"));
    }
    for issue in &doctor.issues {
        lines.push(format!("issue: {issue}"));
    }
    lines
}

pub fn env_cleanup_batch(cleanup: &EnvCleanupBatchSummary) -> Vec<String> {
    let mut lines = if cleanup.apply {
        vec![format!("Applied cleanup (--all): {} env(s)", cleanup.count)]
    } else {
        vec![format!("Cleanup preview (--all): {} env(s)", cleanup.count)]
    };
    for result in &cleanup.results {
        lines.push(format!("  {}", result.env_name));
        lines.push(format!("    root: {}", result.root));
        if result.apply {
            lines.push(format!("    applied fixes: {}", result.actions.len()));
        } else {
            lines.push(format!("    safe fixes: {}", result.actions.len()));
        }
        for action in &result.actions {
            lines.push(format!("    {}: {}", action.kind, action.description));
        }
    }
    lines
}

pub fn env_cleanup(cleanup: &EnvCleanupSummary) -> Vec<String> {
    let mut lines = if cleanup.apply {
        vec![format!("Applied cleanup for env {}", cleanup.env_name)]
    } else {
        vec![format!("Cleanup preview for env {}", cleanup.env_name)]
    };
    lines.push(format!("  root: {}", cleanup.root));
    if cleanup.apply {
        lines.push(format!("  applied fixes: {}", cleanup.actions.len()));
    } else {
        lines.push(format!("  safe fixes: {}", cleanup.actions.len()));
    }
    for action in &cleanup.actions {
        lines.push(format!("  {}: {}", action.kind, action.description));
    }
    if cleanup.apply {
        if let Some(healthy_after) = cleanup.healthy_after {
            lines.push(format!("  healthy after: {healthy_after}"));
        }
        if let Some(issues_after) = cleanup.issues_after.as_ref() {
            for issue in issues_after {
                lines.push(format!("  issue: {issue}"));
            }
        }
    } else {
        for issue in &cleanup.issues_before {
            lines.push(format!("  issue: {issue}"));
        }
        if !cleanup.actions.is_empty() {
            lines.push("  re-run with --yes to apply them".to_string());
        }
    }
    lines
}

pub fn env_marker_repaired(repaired: &EnvMarkerRepairSummary) -> Vec<String> {
    vec![
        format!("Repaired marker for env {}", repaired.env_name),
        format!("  root: {}", repaired.root),
        format!("  marker: {}", repaired.marker_path),
    ]
}

pub fn env_list(summaries: &[EnvSummary], profile: RenderProfile) -> Vec<String> {
    if summaries.is_empty() {
        return vec!["No environments.".to_string()];
    }
    if !profile.pretty {
        return env_list_raw(summaries);
    }

    let rows = summaries
        .iter()
        .map(|summary| {
            let flags = if summary.protected {
                "protected"
            } else {
                "—"
            };
            vec![
                Cell::accent(summary.name.clone()),
                Cell::muted(summary.root.clone()),
                optional_cell(summary.default_runtime.as_deref(), Tone::Accent),
                optional_cell(summary.default_launcher.as_deref(), Tone::Accent),
                optional_number_cell(summary.gateway_port),
                if summary.protected {
                    Cell::warning(flags)
                } else {
                    Cell::muted(flags)
                },
            ]
        })
        .collect::<Vec<_>>();
    render_table(
        &["Name", "Root", "Runtime", "Launcher", "Port", "Flags"],
        &rows,
        profile.color,
    )
}

fn env_list_raw(summaries: &[EnvSummary]) -> Vec<String> {
    let mut lines = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let mut bits = vec![summary.name.clone(), summary.root.clone()];
        if let Some(runtime) = summary.default_runtime.as_deref() {
            bits.push(format!("runtime={runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            bits.push(format!("launcher={launcher}"));
        }
        if let Some(port) = summary.gateway_port {
            bits.push(format!("port={port}"));
        }
        if summary.protected {
            bits.push("protected".to_string());
        }
        lines.push(bits.join("  "));
    }
    lines
}

fn optional_cell(value: Option<&str>, tone: Tone) -> Cell {
    match value {
        Some(value) => Cell::new(value, crate::infra::terminal::Align::Left, tone),
        None => Cell::muted("—"),
    }
}

fn optional_number_cell(value: Option<u32>) -> Cell {
    match value {
        Some(value) => Cell::right(value.to_string(), Tone::Accent),
        None => Cell::muted("—"),
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::{RenderProfile, env_list};
    use crate::env::EnvSummary;

    #[test]
    fn env_list_pretty_uses_a_table() {
        let summaries = vec![EnvSummary {
            name: "demo".to_string(),
            root: "/tmp/demo".to_string(),
            openclaw_home: "/tmp/demo/.openclaw".to_string(),
            state_dir: "/tmp/demo/.openclaw".to_string(),
            config_path: "/tmp/demo/.openclaw/openclaw.json".to_string(),
            workspace_dir: "/tmp/demo/workspace".to_string(),
            gateway_port: Some(18789),
            default_runtime: None,
            default_launcher: Some("stable".to_string()),
            protected: true,
            created_at: OffsetDateTime::UNIX_EPOCH,
            last_used_at: None,
        }];

        let lines = env_list(&summaries, RenderProfile::pretty(false));
        assert!(lines[0].starts_with('┌'));
        assert!(lines[1].contains("Name"));
        assert!(lines[3].contains("demo"));
        assert!(lines[3].contains("protected"));
        assert!(lines[4].starts_with('└'));
    }
}

pub fn env_show(summary: &EnvSummary) -> Result<Vec<String>, String> {
    let mut lines = BTreeMap::new();
    lines.insert("name".to_string(), summary.name.clone());
    lines.insert("root".to_string(), summary.root.clone());
    lines.insert("openclawHome".to_string(), summary.openclaw_home.clone());
    lines.insert("stateDir".to_string(), summary.state_dir.clone());
    lines.insert("configPath".to_string(), summary.config_path.clone());
    lines.insert("workspaceDir".to_string(), summary.workspace_dir.clone());
    lines.insert("protected".to_string(), summary.protected.to_string());
    lines.insert("createdAt".to_string(), format_rfc3339(summary.created_at)?);
    if let Some(port) = summary.gateway_port {
        lines.insert("gatewayPort".to_string(), port.to_string());
    }
    if let Some(runtime) = summary.default_runtime.as_deref() {
        lines.insert("defaultRuntime".to_string(), runtime.to_string());
    }
    if let Some(launcher) = summary.default_launcher.as_deref() {
        lines.insert("defaultLauncher".to_string(), launcher.to_string());
    }
    if let Some(last_used_at) = summary.last_used_at {
        lines.insert("lastUsedAt".to_string(), format_rfc3339(last_used_at)?);
    }
    Ok(format_key_value_lines(lines))
}

pub fn env_status(status: &EnvStatusSummary) -> Vec<String> {
    let mut lines = vec![
        format!("envName: {}", status.env_name),
        format!("root: {}", status.root),
    ];
    if let Some(port) = status.gateway_port {
        lines.push(format!("gatewayPort: {port}"));
    }
    if let Some(source) = status.gateway_port_source.as_deref() {
        lines.push(format!("gatewayPortSource: {source}"));
    }
    if let Some(runtime) = status.default_runtime.as_deref() {
        lines.push(format!("defaultRuntime: {runtime}"));
    }
    if let Some(launcher) = status.default_launcher.as_deref() {
        lines.push(format!("defaultLauncher: {launcher}"));
    }
    if let Some(kind) = status.resolved_kind.as_deref() {
        lines.push(format!("resolvedKind: {kind}"));
    }
    if let Some(name) = status.resolved_name.as_deref() {
        lines.push(format!("resolvedName: {name}"));
    }
    if let Some(binary_path) = status.binary_path.as_deref() {
        lines.push(format!("binaryPath: {binary_path}"));
    }
    if let Some(command) = status.command.as_deref() {
        lines.push(format!("command: {command}"));
    }
    if let Some(run_dir) = status.run_dir.as_deref() {
        lines.push(format!("runDir: {run_dir}"));
    }
    if let Some(source_kind) = status.runtime_source_kind.as_deref() {
        lines.push(format!("runtimeSourceKind: {source_kind}"));
    }
    if let Some(release_version) = status.runtime_release_version.as_deref() {
        lines.push(format!("runtimeReleaseVersion: {release_version}"));
    }
    if let Some(release_channel) = status.runtime_release_channel.as_deref() {
        lines.push(format!("runtimeReleaseChannel: {release_channel}"));
    }
    if let Some(runtime_health) = status.runtime_health.as_deref() {
        lines.push(format!("runtimeHealth: {runtime_health}"));
    }
    if let Some(state) = status.managed_service_state.as_deref() {
        lines.push(format!("managedServiceState: {state}"));
    }
    if let Some(state) = status.global_service_state.as_deref() {
        lines.push(format!("globalServiceState: {state}"));
    }
    if let Some(issue) = status.issue.as_deref() {
        lines.push(format!("issue: {issue}"));
    }
    lines
}

pub fn env_snapshot_created(snapshot: &EnvSnapshotSummary) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Created snapshot {} for env {}",
            snapshot.id, snapshot.env_name
        ),
        format!("  archive: {}", snapshot.archive_path),
        format!("  root: {}", snapshot.source_root),
    ];
    if let Some(label) = snapshot.label.as_deref() {
        lines.push(format!("  label: {label}"));
    }
    lines
}

pub fn env_snapshot_show(snapshot: &EnvSnapshotSummary) -> Result<Vec<String>, String> {
    let mut lines = vec![
        format!("snapshotId: {}", snapshot.id),
        format!("envName: {}", snapshot.env_name),
        format!("archivePath: {}", snapshot.archive_path),
        format!("sourceRoot: {}", snapshot.source_root),
    ];
    if let Some(label) = snapshot.label.as_deref() {
        lines.push(format!("label: {label}"));
    }
    if let Some(port) = snapshot.gateway_port {
        lines.push(format!("gatewayPort: {port}"));
    }
    if let Some(runtime) = snapshot.default_runtime.as_deref() {
        lines.push(format!("defaultRuntime: {runtime}"));
    }
    if let Some(launcher) = snapshot.default_launcher.as_deref() {
        lines.push(format!("defaultLauncher: {launcher}"));
    }
    if snapshot.protected {
        lines.push("protected: true".to_string());
    }
    lines.push(format!(
        "createdAt: {}",
        format_rfc3339(snapshot.created_at)?
    ));
    Ok(lines)
}

pub fn env_snapshot_list(snapshots: &[EnvSnapshotSummary]) -> Vec<String> {
    if snapshots.is_empty() {
        return vec!["No snapshots.".to_string()];
    }
    let mut lines = Vec::with_capacity(snapshots.len());
    for snapshot in snapshots {
        let mut bits = vec![snapshot.id.clone(), snapshot.env_name.clone()];
        if let Some(label) = snapshot.label.as_deref() {
            bits.push(format!("label={label}"));
        }
        bits.push(snapshot.archive_path.clone());
        lines.push(bits.join("  "));
    }
    lines
}

pub fn env_snapshot_restored(restored: &EnvSnapshotRestoreSummary) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Restored env {} from snapshot {}",
            restored.env_name, restored.snapshot_id
        ),
        format!("  root: {}", restored.root),
        format!("  archive: {}", restored.archive_path),
    ];
    if let Some(label) = restored.label.as_deref() {
        lines.push(format!("  label: {label}"));
    }
    if let Some(runtime) = restored.default_runtime.as_deref() {
        lines.push(format!("  runtime: {runtime}"));
    }
    if let Some(launcher) = restored.default_launcher.as_deref() {
        lines.push(format!("  launcher: {launcher}"));
    }
    if restored.protected {
        lines.push("  protected: true".to_string());
    }
    lines
}

pub fn env_snapshot_removed(removed: &EnvSnapshotRemoveSummary) -> Vec<String> {
    let mut lines = vec![
        format!(
            "Removed snapshot {} for env {}",
            removed.snapshot_id, removed.env_name
        ),
        format!("  archive: {}", removed.archive_path),
    ];
    if let Some(label) = removed.label.as_deref() {
        lines.push(format!("  label: {label}"));
    }
    lines
}

pub fn env_snapshot_prune_preview(
    scope_label: &str,
    candidates: &[EnvSnapshotSummary],
) -> Vec<String> {
    let mut lines = vec![format!(
        "Snapshot prune preview ({scope_label}): {} candidate(s)",
        candidates.len()
    )];
    for candidate in candidates {
        let mut bits = vec![candidate.id.clone(), candidate.env_name.clone()];
        if let Some(label) = candidate.label.as_deref() {
            bits.push(format!("label={label}"));
        }
        bits.push(candidate.archive_path.clone());
        lines.push(bits.join("  "));
    }
    lines.push("Re-run with --yes to remove them.".to_string());
    lines
}

pub fn env_snapshot_pruned(removed: &[EnvSnapshotRemoveSummary]) -> Vec<String> {
    let mut lines = vec![format!("Pruned {} snapshot(s).", removed.len())];
    for snapshot in removed {
        let mut bits = vec![snapshot.snapshot_id.clone(), snapshot.env_name.clone()];
        if let Some(label) = snapshot.label.as_deref() {
            bits.push(format!("label={label}"));
        }
        bits.push(snapshot.archive_path.clone());
        lines.push(format!("  {}", bits.join("  ")));
    }
    lines
}

pub fn env_resolved(summary: &ExecutionSummary) -> Vec<String> {
    let mut lines = vec![
        format!("envName: {}", summary.env_name),
        format!("bindingKind: {}", summary.binding_kind),
        format!("bindingName: {}", summary.binding_name),
    ];
    if let Some(command) = summary.command.as_deref() {
        lines.push(format!("command: {command}"));
    }
    if let Some(binary_path) = summary.binary_path.as_deref() {
        lines.push(format!("binaryPath: {binary_path}"));
    }
    if !summary.forwarded_args.is_empty() {
        lines.push(format!(
            "forwardedArgs: {}",
            summary.forwarded_args.join(" ")
        ));
    }
    lines.push(format!("runDir: {}", summary.run_dir));
    lines
}

pub fn env_runtime_updated(name: &str, runtime_name: &str) -> Vec<String> {
    vec![format!("Updated env {name}: defaultRuntime={runtime_name}")]
}

pub fn env_launcher_updated(name: &str, launcher_name: &str) -> Vec<String> {
    vec![format!(
        "Updated env {name}: defaultLauncher={launcher_name}"
    )]
}
