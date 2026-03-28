use std::collections::BTreeMap;

use crate::env::{
    EnvCleanupBatchSummary, EnvCleanupSummary, EnvDoctorSummary, EnvExportSummary,
    EnvImportSummary, EnvMarkerRepairSummary, EnvSnapshotRemoveSummary, EnvSnapshotRestoreSummary,
    EnvSnapshotSummary, EnvStatusSummary, EnvSummary, ExecutionSummary,
};
use crate::infra::terminal::{
    Cell, KeyValueRow, Tone, paint, render_key_value_card, render_table, render_tags,
};

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

pub fn env_created(
    summary: &EnvSummary,
    gateway_port_source: Option<&str>,
    command_example: &str,
) -> Vec<String> {
    let mut lines = vec![
        format!("Created env {}", summary.name),
        format!("  root: {}", summary.root),
        format!("  openclaw home: {}", summary.openclaw_home),
        format!("  workspace: {}", summary.workspace_dir),
    ];
    if let Some(port) = summary.gateway_port {
        lines.push(render_gateway_port_line(port, gateway_port_source));
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

pub fn env_cloned(
    summary: &EnvSummary,
    gateway_port_source: Option<&str>,
    source_name: &str,
    command_example: &str,
) -> Vec<String> {
    let mut lines = vec![
        format!("Cloned env {} from {}", summary.name, source_name),
        format!("  root: {}", summary.root),
        format!("  openclaw home: {}", summary.openclaw_home),
        format!("  workspace: {}", summary.workspace_dir),
    ];
    if let Some(port) = summary.gateway_port {
        lines.push(render_gateway_port_line(port, gateway_port_source));
    }
    lines.push(format!(
        "  activate: eval \"$({command_example} env use {})\"",
        summary.name
    ));
    lines
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

pub fn env_doctor(doctor: &EnvDoctorSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return env_doctor_raw(doctor);
    }

    let mut lines = vec![paint(
        &format!("Environment doctor {}", doctor.env_name),
        Tone::Strong,
        profile.color,
    )];

    push_card(
        &mut lines,
        "Summary",
        vec![
            KeyValueRow::plain("Root", doctor.root.clone()),
            KeyValueRow::new(
                "Healthy",
                if doctor.healthy { "yes" } else { "no" },
                if doctor.healthy {
                    Tone::Success
                } else {
                    Tone::Danger
                },
            ),
            optional_value_row("Default runtime", doctor.default_runtime.clone()),
            optional_value_row("Default launcher", doctor.default_launcher.clone()),
            doctor_resolution_row(doctor),
        ],
        profile.color,
    );

    push_card(
        &mut lines,
        "Checks",
        vec![
            doctor_state_row("Root", &doctor.root_status),
            doctor_state_row("Marker", &doctor.marker_status),
            doctor_state_row("Runtime", &doctor.runtime_status),
            doctor_state_row("Launcher", &doctor.launcher_status),
            doctor_state_row("Resolution", &doctor.resolution_status),
        ],
        profile.color,
    );

    if !doctor.issues.is_empty() {
        let issue_rows = doctor
            .issues
            .iter()
            .enumerate()
            .map(|(index, issue)| KeyValueRow::danger(format!("#{}", index + 1), issue.clone()))
            .collect::<Vec<_>>();
        push_card(&mut lines, "Issues", issue_rows, profile.color);
    }

    lines
}

fn env_doctor_raw(doctor: &EnvDoctorSummary) -> Vec<String> {
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

fn render_gateway_port_line(port: u32, source: Option<&str>) -> String {
    match source {
        Some("metadata") | None => format!("  gateway port: {port}"),
        Some(source) => format!("  effective gateway port: {port} ({source})"),
    }
}

fn push_card(lines: &mut Vec<String>, title: &str, rows: Vec<KeyValueRow>, color: bool) {
    if rows.is_empty() {
        return;
    }
    if !lines.is_empty() {
        lines.push(String::new());
    }
    lines.extend(render_key_value_card(title, &rows, color));
}

fn optional_value_row(label: &str, value: Option<String>) -> KeyValueRow {
    match value {
        Some(value) => KeyValueRow::plain(label, value),
        None => KeyValueRow::muted(label, "—"),
    }
}

fn bool_row(label: &str, value: bool) -> KeyValueRow {
    if value {
        KeyValueRow::warning(label, "yes")
    } else {
        KeyValueRow::muted(label, "no")
    }
}

fn optional_state_row(label: &str, value: Option<String>) -> KeyValueRow {
    match value {
        Some(value) => KeyValueRow::new(label, value.clone(), state_tone(&value)),
        None => KeyValueRow::muted(label, "—"),
    }
}

fn resolution_row(status: &EnvStatusSummary) -> KeyValueRow {
    match (
        status.resolved_kind.as_deref(),
        status.resolved_name.as_deref(),
    ) {
        (Some(kind), Some(name)) => KeyValueRow::accent("Resolved", format!("{kind}:{name}")),
        _ => KeyValueRow::muted("Resolved", "—"),
    }
}

fn doctor_resolution_row(doctor: &EnvDoctorSummary) -> KeyValueRow {
    match (
        doctor.resolved_kind.as_deref(),
        doctor.resolved_name.as_deref(),
    ) {
        (Some(kind), Some(name)) => KeyValueRow::accent("Resolved", format!("{kind}:{name}")),
        _ => KeyValueRow::muted("Resolved", "—"),
    }
}

fn doctor_state_row(label: &str, status: &str) -> KeyValueRow {
    KeyValueRow::new(label, status, doctor_state_tone(status))
}

fn doctor_state_tone(status: &str) -> Tone {
    match status {
        "ok" => Tone::Success,
        "unbound" => Tone::Muted,
        "missing" | "mismatch" | "invalid" | "broken" | "error" => Tone::Danger,
        _ => Tone::Warning,
    }
}

fn state_tone(state: &str) -> Tone {
    match state {
        "ok" | "running" | "match" | "healthy" => Tone::Success,
        "loaded" | "installed" | "loaded-other" | "installed-other" | "running-other" => {
            Tone::Warning
        }
        "broken" | "missing" | "unreachable" => Tone::Danger,
        "absent" | "stopped" | "unknown" => Tone::Muted,
        _ => Tone::Plain,
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::{
        RenderProfile, env_doctor, env_list, env_resolved, env_show, env_snapshot_list,
        env_snapshot_prune_preview, env_snapshot_show, env_status,
    };
    use crate::env::{
        EnvDoctorSummary, EnvSnapshotSummary, EnvStatusSummary, EnvSummary, ExecutionSummary,
    };

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

    #[test]
    fn env_show_pretty_uses_cards() {
        let lines = env_show(
            &EnvSummary {
                name: "demo".to_string(),
                root: "/tmp/demo".to_string(),
                openclaw_home: "/tmp/demo".to_string(),
                state_dir: "/tmp/demo/.openclaw".to_string(),
                config_path: "/tmp/demo/.openclaw/openclaw.json".to_string(),
                workspace_dir: "/tmp/demo/.openclaw/workspace".to_string(),
                gateway_port: Some(18789),
                default_runtime: None,
                default_launcher: Some("stable".to_string()),
                protected: false,
                created_at: OffsetDateTime::UNIX_EPOCH,
                last_used_at: None,
            },
            RenderProfile::pretty(false),
        )
        .unwrap();

        assert_eq!(lines[0], "Environment demo");
        assert!(lines.iter().any(|line| line.contains("Paths")));
        assert!(lines.iter().any(|line| line.contains("Metadata")));
    }

    #[test]
    fn env_status_pretty_uses_cards() {
        let lines = env_status(
            &EnvStatusSummary {
                env_name: "demo".to_string(),
                root: "/tmp/demo".to_string(),
                gateway_port: Some(18789),
                gateway_port_source: Some("computed".to_string()),
                default_runtime: None,
                default_launcher: Some("stable".to_string()),
                resolved_kind: Some("launcher".to_string()),
                resolved_name: Some("stable".to_string()),
                binary_path: None,
                command: Some("openclaw".to_string()),
                run_dir: Some("/tmp/demo".to_string()),
                runtime_source_kind: None,
                runtime_release_version: None,
                runtime_release_channel: None,
                runtime_health: None,
                managed_service_state: Some("running".to_string()),
                openclaw_state: Some("healthy".to_string()),
                global_service_state: Some("absent".to_string()),
                issue: None,
            },
            RenderProfile::pretty(false),
        );

        assert_eq!(lines[0], "Environment status demo");
        assert!(lines.iter().any(|line| line.contains("Binding")));
        assert!(lines.iter().any(|line| line.contains("Gateway")));
        assert!(lines.iter().any(|line| line.contains("OCM service")));
        assert!(lines.iter().any(|line| line.contains("OpenClaw")));
        assert!(!lines.iter().any(|line| line.contains("OpenClaw service")));
    }

    #[test]
    fn env_resolved_pretty_uses_cards() {
        let lines = env_resolved(
            &ExecutionSummary {
                env_name: "demo".to_string(),
                binding_kind: "launcher".to_string(),
                binding_name: "stable".to_string(),
                command: Some("openclaw".to_string()),
                binary_path: None,
                forwarded_args: vec!["status".to_string()],
                run_dir: "/tmp/demo".to_string(),
            },
            RenderProfile::pretty(false),
        );

        assert_eq!(lines[0], "Execution plan demo");
        assert!(lines.iter().any(|line| line.contains("Resolution")));
        assert!(lines.iter().any(|line| line.contains("Forwarded args")));
    }

    #[test]
    fn env_doctor_pretty_uses_cards() {
        let lines = env_doctor(
            &EnvDoctorSummary {
                env_name: "demo".to_string(),
                root: "/tmp/demo".to_string(),
                default_runtime: None,
                default_launcher: Some("stable".to_string()),
                healthy: false,
                root_status: "ok".to_string(),
                marker_status: "mismatch".to_string(),
                runtime_status: "unbound".to_string(),
                launcher_status: "ok".to_string(),
                resolution_status: "ok".to_string(),
                resolved_kind: Some("launcher".to_string()),
                resolved_name: Some("stable".to_string()),
                issues: vec!["environment marker name mismatch".to_string()],
            },
            RenderProfile::pretty(false),
        );

        assert_eq!(lines[0], "Environment doctor demo");
        assert!(lines.iter().any(|line| line.contains("Summary")));
        assert!(lines.iter().any(|line| line.contains("Checks")));
        assert!(lines.iter().any(|line| line.contains("Issues")));
    }

    #[test]
    fn env_snapshot_show_pretty_uses_cards() {
        let lines = env_snapshot_show(
            &sample_snapshot("demo", "before-upgrade"),
            RenderProfile::pretty(false),
        )
        .unwrap();

        assert_eq!(lines[0], "Snapshot snap-001");
        assert!(lines.iter().any(|line| line.contains("Snapshot")));
        assert!(lines.iter().any(|line| line.contains("Paths")));
        assert!(lines.iter().any(|line| line.contains("Bindings")));
    }

    #[test]
    fn env_snapshot_list_pretty_uses_a_table() {
        let lines = env_snapshot_list(
            &[sample_snapshot("demo", "before-upgrade")],
            RenderProfile::pretty(false),
        )
        .unwrap();

        assert!(lines[0].starts_with('┌'));
        assert!(lines[1].contains("Snapshot"));
        assert!(lines[3].contains("snap-001"));
        assert!(lines[3].contains("launcher:stable"));
        assert!(lines[4].starts_with('└'));
    }

    #[test]
    fn env_snapshot_prune_preview_pretty_uses_a_table() {
        let lines = env_snapshot_prune_preview(
            "all",
            &[sample_snapshot("demo", "before-upgrade")],
            RenderProfile::pretty(false),
        )
        .unwrap();

        assert_eq!(lines[0], "Snapshot prune preview");
        assert!(lines[1].contains("[scope:all]"));
        assert!(lines[1].contains("[1 candidate(s)]"));
        assert!(lines.iter().any(|line| line.starts_with('┌')));
        assert!(lines.iter().any(|line| line.contains("Archive")));
        assert_eq!(lines.last().unwrap(), "Re-run with --yes to remove them.");
    }

    fn sample_snapshot(env_name: &str, label: &str) -> EnvSnapshotSummary {
        EnvSnapshotSummary {
            id: "snap-001".to_string(),
            env_name: env_name.to_string(),
            label: Some(label.to_string()),
            archive_path: "/tmp/demo-snapshot.tar".to_string(),
            source_root: "/tmp/demo".to_string(),
            gateway_port: Some(18789),
            default_runtime: None,
            default_launcher: Some("stable".to_string()),
            protected: true,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }
}

pub fn env_show(summary: &EnvSummary, profile: RenderProfile) -> Result<Vec<String>, String> {
    if !profile.pretty {
        return env_show_raw(summary);
    }

    let mut lines = vec![paint(
        &format!("Environment {}", summary.name),
        Tone::Strong,
        profile.color,
    )];

    push_card(
        &mut lines,
        "Paths",
        vec![
            KeyValueRow::plain("Root", summary.root.clone()),
            KeyValueRow::plain("OpenClaw home", summary.openclaw_home.clone()),
            KeyValueRow::plain("State dir", summary.state_dir.clone()),
            KeyValueRow::plain("Config path", summary.config_path.clone()),
            KeyValueRow::plain("Workspace", summary.workspace_dir.clone()),
        ],
        profile.color,
    );

    let mut metadata = vec![
        optional_value_row(
            "Gateway port",
            summary.gateway_port.map(|value| value.to_string()),
        ),
        optional_value_row("Runtime", summary.default_runtime.clone()),
        optional_value_row("Launcher", summary.default_launcher.clone()),
        bool_row("Protected", summary.protected),
        KeyValueRow::plain("Created", format_rfc3339(summary.created_at)?),
    ];
    if let Some(last_used_at) = summary.last_used_at {
        metadata.push(KeyValueRow::plain(
            "Last used",
            format_rfc3339(last_used_at)?,
        ));
    }
    push_card(&mut lines, "Metadata", metadata, profile.color);

    Ok(lines)
}

fn env_show_raw(summary: &EnvSummary) -> Result<Vec<String>, String> {
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

pub fn env_status(status: &EnvStatusSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return env_status_raw(status);
    }

    let mut lines = vec![paint(
        &format!("Environment status {}", status.env_name),
        Tone::Strong,
        profile.color,
    )];

    push_card(
        &mut lines,
        "Binding",
        vec![
            optional_value_row("Default runtime", status.default_runtime.clone()),
            optional_value_row("Default launcher", status.default_launcher.clone()),
            resolution_row(status),
            optional_value_row("Run dir", status.run_dir.clone()),
        ],
        profile.color,
    );

    let mut execution = Vec::new();
    if let Some(command) = status.command.as_ref() {
        execution.push(KeyValueRow::accent("Command", command.clone()));
    }
    if let Some(binary_path) = status.binary_path.as_ref() {
        execution.push(KeyValueRow::accent("Binary", binary_path.clone()));
    }
    if let Some(source_kind) = status.runtime_source_kind.as_ref() {
        execution.push(KeyValueRow::plain("Runtime source", source_kind.clone()));
    }
    if let Some(release_version) = status.runtime_release_version.as_ref() {
        execution.push(KeyValueRow::plain(
            "Release version",
            release_version.clone(),
        ));
    }
    if let Some(release_channel) = status.runtime_release_channel.as_ref() {
        execution.push(KeyValueRow::plain(
            "Release channel",
            release_channel.clone(),
        ));
    }
    if let Some(runtime_health) = status.runtime_health.as_ref() {
        execution.push(KeyValueRow::new(
            "Runtime health",
            runtime_health.clone(),
            state_tone(runtime_health),
        ));
    }
    if !execution.is_empty() {
        push_card(&mut lines, "Execution", execution, profile.color);
    }

    push_card(
        &mut lines,
        "Gateway",
        vec![
            optional_value_row("Port", status.gateway_port.map(|value| value.to_string())),
            optional_value_row("Port source", status.gateway_port_source.clone()),
            optional_state_row("OCM service", status.managed_service_state.clone()),
            optional_state_row("OpenClaw", status.openclaw_state.clone()),
            KeyValueRow::plain("Root", status.root.clone()),
        ],
        profile.color,
    );

    if let Some(issue) = status.issue.as_ref() {
        push_card(
            &mut lines,
            "Issue",
            vec![KeyValueRow::danger("Problem", issue.clone())],
            profile.color,
        );
    }

    lines
}

fn env_status_raw(status: &EnvStatusSummary) -> Vec<String> {
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
    if let Some(state) = status.openclaw_state.as_deref() {
        lines.push(format!("openclawState: {state}"));
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

pub fn env_snapshot_show(
    snapshot: &EnvSnapshotSummary,
    profile: RenderProfile,
) -> Result<Vec<String>, String> {
    if !profile.pretty {
        return env_snapshot_show_raw(snapshot);
    }

    let mut lines = vec![paint(
        &format!("Snapshot {}", snapshot.id),
        Tone::Strong,
        profile.color,
    )];

    push_card(
        &mut lines,
        "Snapshot",
        vec![
            KeyValueRow::accent("Env", snapshot.env_name.clone()),
            KeyValueRow::plain("Created", format_rfc3339(snapshot.created_at)?),
            optional_value_row("Label", snapshot.label.clone()),
            bool_row("Protected", snapshot.protected),
        ],
        profile.color,
    );

    push_card(
        &mut lines,
        "Paths",
        vec![
            KeyValueRow::plain("Archive", snapshot.archive_path.clone()),
            KeyValueRow::plain("Source root", snapshot.source_root.clone()),
        ],
        profile.color,
    );

    push_card(
        &mut lines,
        "Bindings",
        vec![
            optional_value_row(
                "Gateway port",
                snapshot.gateway_port.map(|value| value.to_string()),
            ),
            optional_value_row("Runtime", snapshot.default_runtime.clone()),
            optional_value_row("Launcher", snapshot.default_launcher.clone()),
        ],
        profile.color,
    );

    Ok(lines)
}

fn env_snapshot_show_raw(snapshot: &EnvSnapshotSummary) -> Result<Vec<String>, String> {
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

pub fn env_snapshot_list(
    snapshots: &[EnvSnapshotSummary],
    profile: RenderProfile,
) -> Result<Vec<String>, String> {
    if snapshots.is_empty() {
        return Ok(vec!["No snapshots.".to_string()]);
    }
    if !profile.pretty {
        return Ok(env_snapshot_list_raw(snapshots));
    }

    let rows = snapshots
        .iter()
        .map(|snapshot| {
            Ok(vec![
                Cell::accent(snapshot.id.clone()),
                Cell::plain(snapshot.env_name.clone()),
                optional_cell(snapshot.label.as_deref(), Tone::Accent),
                optional_number_cell(snapshot.gateway_port),
                snapshot_binding_cell(snapshot),
                Cell::muted(format_rfc3339(snapshot.created_at)?),
            ])
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(render_table(
        &["Snapshot", "Env", "Label", "Port", "Binding", "Created"],
        &rows,
        profile.color,
    ))
}

fn env_snapshot_list_raw(snapshots: &[EnvSnapshotSummary]) -> Vec<String> {
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
    profile: RenderProfile,
) -> Result<Vec<String>, String> {
    if !profile.pretty {
        return Ok(env_snapshot_prune_preview_raw(scope_label, candidates));
    }

    let mut lines = vec![paint("Snapshot prune preview", Tone::Strong, profile.color)];
    lines.push(render_tags(
        &[
            Cell::accent(format!("scope:{scope_label}")),
            Cell::warning(format!("{} candidate(s)", candidates.len())),
        ],
        profile.color,
    ));

    if candidates.is_empty() {
        lines.push(String::new());
        lines.push(paint(
            "Nothing would be removed.",
            Tone::Muted,
            profile.color,
        ));
        return Ok(lines);
    }

    lines.push(String::new());
    let rows = candidates
        .iter()
        .map(|candidate| {
            Ok(vec![
                Cell::accent(candidate.id.clone()),
                Cell::plain(candidate.env_name.clone()),
                optional_cell(candidate.label.as_deref(), Tone::Accent),
                Cell::muted(format_rfc3339(candidate.created_at)?),
                Cell::muted(candidate.archive_path.clone()),
            ])
        })
        .collect::<Result<Vec<_>, String>>()?;
    lines.extend(render_table(
        &["Snapshot", "Env", "Label", "Created", "Archive"],
        &rows,
        profile.color,
    ));
    lines.push(String::new());
    lines.push(paint(
        "Re-run with --yes to remove them.",
        Tone::Muted,
        profile.color,
    ));
    Ok(lines)
}

fn env_snapshot_prune_preview_raw(
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

pub fn env_snapshot_pruned(
    removed: &[EnvSnapshotRemoveSummary],
    profile: RenderProfile,
) -> Vec<String> {
    if !profile.pretty {
        return env_snapshot_pruned_raw(removed);
    }

    let mut lines = vec![paint("Snapshot prune applied", Tone::Strong, profile.color)];
    lines.push(render_tags(
        &[Cell::warning(format!("{} removed", removed.len()))],
        profile.color,
    ));

    if removed.is_empty() {
        lines.push(String::new());
        lines.push(paint("Nothing was removed.", Tone::Muted, profile.color));
        return lines;
    }

    lines.push(String::new());
    let rows = removed
        .iter()
        .map(|snapshot| {
            vec![
                Cell::accent(snapshot.snapshot_id.clone()),
                Cell::plain(snapshot.env_name.clone()),
                optional_cell(snapshot.label.as_deref(), Tone::Accent),
                Cell::muted(snapshot.archive_path.clone()),
            ]
        })
        .collect::<Vec<_>>();
    lines.extend(render_table(
        &["Snapshot", "Env", "Label", "Archive"],
        &rows,
        profile.color,
    ));
    lines
}

fn env_snapshot_pruned_raw(removed: &[EnvSnapshotRemoveSummary]) -> Vec<String> {
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

fn snapshot_binding_cell(snapshot: &EnvSnapshotSummary) -> Cell {
    if let Some(runtime) = snapshot.default_runtime.as_deref() {
        return Cell::accent(format!("runtime:{runtime}"));
    }
    if let Some(launcher) = snapshot.default_launcher.as_deref() {
        return Cell::accent(format!("launcher:{launcher}"));
    }
    Cell::muted("—")
}

pub fn env_resolved(summary: &ExecutionSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return env_resolved_raw(summary);
    }

    let mut lines = vec![paint(
        &format!("Execution plan {}", summary.env_name),
        Tone::Strong,
        profile.color,
    )];

    let mut resolution = vec![
        KeyValueRow::accent(
            "Binding",
            format!("{}:{}", summary.binding_kind, summary.binding_name),
        ),
        KeyValueRow::plain("Run dir", summary.run_dir.clone()),
    ];
    if let Some(command) = summary.command.as_ref() {
        resolution.push(KeyValueRow::accent("Command", command.clone()));
    }
    if let Some(binary_path) = summary.binary_path.as_ref() {
        resolution.push(KeyValueRow::accent("Binary", binary_path.clone()));
    }
    if !summary.forwarded_args.is_empty() {
        resolution.push(KeyValueRow::plain(
            "Forwarded args",
            summary.forwarded_args.join(" "),
        ));
    }
    push_card(&mut lines, "Resolution", resolution, profile.color);

    lines
}

fn env_resolved_raw(summary: &ExecutionSummary) -> Vec<String> {
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
