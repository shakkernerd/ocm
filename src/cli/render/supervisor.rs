use super::{RenderProfile, format_rfc3339};
use crate::infra::terminal::{Cell, Tone, paint, render_table, terminal_width};
use crate::supervisor::{
    SupervisorDaemonSummary, SupervisorRunSummary, SupervisorStatusSummary, SupervisorView,
};

pub fn supervisor_state(summary: &SupervisorView, profile: RenderProfile) -> Vec<String> {
    supervisor_state_with_width(summary, profile, terminal_width())
}

fn supervisor_state_with_width(
    summary: &SupervisorView,
    profile: RenderProfile,
    _width: Option<usize>,
) -> Vec<String> {
    if !profile.pretty {
        return supervisor_state_raw(summary);
    }

    let mut lines = vec![paint("Supervisor", Tone::Strong, profile.color)];
    lines.push(format!("State: {}", summary.state_path));
    lines.push(format!(
        "Generated: {}",
        format_rfc3339(summary.generated_at).unwrap_or_else(|_| summary.generated_at.to_string())
    ));
    lines.push(format!("Children: {}", summary.children.len()));
    if !summary.skipped_envs.is_empty() {
        lines.push(format!("Skipped: {}", summary.skipped_envs.len()));
    }

    if !summary.children.is_empty() {
        lines.push(String::new());
        let rows = summary
            .children
            .iter()
            .map(|child| {
                vec![
                    Cell::accent(child.env_name.clone()),
                    Cell::plain(format!("{}:{}", child.binding_kind, child.binding_name)),
                    Cell::right(child.child_port.to_string(), Tone::Accent),
                    Cell::muted(child.start_mode.clone()),
                ]
            })
            .collect::<Vec<_>>();
        lines.extend(render_table(
            &["Env", "Binding", "Port", "Mode"],
            &rows,
            profile.color,
        ));
    }

    if !summary.skipped_envs.is_empty() {
        lines.push(String::new());
        lines.push(paint("Skipped envs", Tone::Warning, profile.color));
        for skipped in &summary.skipped_envs {
            lines.push(format!("{}  {}", skipped.env_name, skipped.reason));
        }
    }

    lines
}

fn supervisor_state_raw(summary: &SupervisorView) -> Vec<String> {
    let mut lines = vec![
        format!("statePath: {}", summary.state_path),
        format!(
            "generatedAt: {}",
            format_rfc3339(summary.generated_at)
                .unwrap_or_else(|_| summary.generated_at.to_string())
        ),
        format!("children: {}", summary.children.len()),
        format!("skipped: {}", summary.skipped_envs.len()),
    ];

    for child in &summary.children {
        lines.push(format!(
            "{}  binding={}:{}  port={}  mode={}",
            child.env_name,
            child.binding_kind,
            child.binding_name,
            child.child_port,
            child.start_mode
        ));
    }

    for skipped in &summary.skipped_envs {
        lines.push(format!(
            "skipped {}  reason={}",
            skipped.env_name, skipped.reason
        ));
    }

    lines
}

pub fn supervisor_drift(summary: &SupervisorStatusSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return supervisor_drift_raw(summary);
    }

    let mut lines = vec![paint("Supervisor drift", Tone::Strong, profile.color)];
    lines.push(format!("State: {}", summary.state_path));
    lines.push(format!(
        "State file: {}",
        if summary.state_present {
            "present"
        } else {
            "missing"
        }
    ));
    lines.push(format!(
        "Sync: {}",
        if summary.in_sync { "in-sync" } else { "stale" }
    ));
    lines.push(format!("Planned children: {}", summary.planned_child_count));
    lines.push(format!(
        "Persisted children: {}",
        summary.persisted_child_count
    ));

    if !summary.missing_children.is_empty() {
        lines.push(format!(
            "Missing children: {}",
            summary.missing_children.join(", ")
        ));
    }
    if !summary.extra_children.is_empty() {
        lines.push(format!(
            "Extra children: {}",
            summary.extra_children.join(", ")
        ));
    }
    if !summary.changed_children.is_empty() {
        lines.push(format!(
            "Changed children: {}",
            summary.changed_children.join(", ")
        ));
    }
    if !summary.skipped_env_changes.is_empty() {
        lines.push(format!(
            "Skipped env changes: {}",
            summary.skipped_env_changes.join(", ")
        ));
    }

    lines
}

fn supervisor_drift_raw(summary: &SupervisorStatusSummary) -> Vec<String> {
    let mut lines = vec![
        format!("statePath: {}", summary.state_path),
        format!("statePresent: {}", summary.state_present),
        format!("inSync: {}", summary.in_sync),
        format!("plannedChildren: {}", summary.planned_child_count),
        format!("persistedChildren: {}", summary.persisted_child_count),
        format!(
            "plannedGeneratedAt: {}",
            format_rfc3339(summary.planned_generated_at)
                .unwrap_or_else(|_| summary.planned_generated_at.to_string())
        ),
    ];
    if let Some(persisted_generated_at) = summary.persisted_generated_at {
        lines.push(format!(
            "persistedGeneratedAt: {}",
            format_rfc3339(persisted_generated_at)
                .unwrap_or_else(|_| persisted_generated_at.to_string())
        ));
    }
    if !summary.missing_children.is_empty() {
        lines.push(format!(
            "missingChildren: {}",
            summary.missing_children.join(",")
        ));
    }
    if !summary.extra_children.is_empty() {
        lines.push(format!(
            "extraChildren: {}",
            summary.extra_children.join(",")
        ));
    }
    if !summary.changed_children.is_empty() {
        lines.push(format!(
            "changedChildren: {}",
            summary.changed_children.join(",")
        ));
    }
    if !summary.skipped_env_changes.is_empty() {
        lines.push(format!(
            "skippedEnvChanges: {}",
            summary.skipped_env_changes.join(",")
        ));
    }
    lines
}

pub fn supervisor_daemon(summary: &SupervisorDaemonSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return supervisor_daemon_raw(summary);
    }

    let mut lines = vec![paint("Supervisor daemon", Tone::Strong, profile.color)];
    lines.push(format!("Action: {}", summary.action));
    lines.push(format!("Label: {}", summary.managed_label));
    lines.push(format!("Definition: {}", summary.definition_path));
    lines.push(format!("State file: {}", summary.state_path));
    lines.push(format!(
        "Installed: {}",
        if summary.installed { "yes" } else { "no" }
    ));
    lines.push(format!(
        "Loaded: {}",
        if summary.loaded { "yes" } else { "no" }
    ));
    lines.push(format!(
        "Running: {}",
        if summary.running { "yes" } else { "no" }
    ));
    if let Some(pid) = summary.pid {
        lines.push(format!("PID: {pid}"));
    }
    if let Some(state) = &summary.state {
        lines.push(format!("State: {state}"));
    }
    lines.push(format!("Stdout: {}", summary.stdout_path));
    lines.push(format!("Stderr: {}", summary.stderr_path));
    lines
}

fn supervisor_daemon_raw(summary: &SupervisorDaemonSummary) -> Vec<String> {
    let mut lines = vec![
        format!("action: {}", summary.action),
        format!("managedLabel: {}", summary.managed_label),
        format!("definitionPath: {}", summary.definition_path),
        format!("statePath: {}", summary.state_path),
        format!("installed: {}", summary.installed),
        format!("loaded: {}", summary.loaded),
        format!("running: {}", summary.running),
        format!("stdoutPath: {}", summary.stdout_path),
        format!("stderrPath: {}", summary.stderr_path),
    ];
    if let Some(pid) = summary.pid {
        lines.push(format!("pid: {pid}"));
    }
    if let Some(state) = &summary.state {
        lines.push(format!("state: {state}"));
    }
    lines
}

pub fn supervisor_run(summary: &SupervisorRunSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return supervisor_run_raw(summary);
    }

    let mut lines = vec![paint("Supervisor run", Tone::Strong, profile.color)];
    lines.push(format!("State: {}", summary.state_path));
    lines.push(format!(
        "Mode: {}",
        if summary.once { "once" } else { "watch" }
    ));
    lines.push(format!("Children: {}", summary.child_count));
    if summary.stopped_by_signal {
        lines.push("Stopped by signal: yes".to_string());
    }

    if !summary.child_results.is_empty() {
        lines.push(String::new());
        let rows = summary
            .child_results
            .iter()
            .map(|result| {
                vec![
                    Cell::accent(result.env_name.clone()),
                    Cell::plain(format!("{}:{}", result.binding_kind, result.binding_name)),
                    Cell::new(
                        result
                            .exit_code
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "signal".to_string()),
                        crate::infra::terminal::Align::Left,
                        if result.success {
                            Tone::Success
                        } else {
                            Tone::Danger
                        },
                    ),
                    Cell::right(result.restart_count.to_string(), Tone::Muted),
                ]
            })
            .collect::<Vec<_>>();
        lines.extend(render_table(
            &["Env", "Binding", "Exit", "Restarts"],
            &rows,
            profile.color,
        ));
    }

    lines
}

fn supervisor_run_raw(summary: &SupervisorRunSummary) -> Vec<String> {
    let mut lines = vec![
        format!("statePath: {}", summary.state_path),
        format!("mode: {}", if summary.once { "once" } else { "watch" }),
        format!("children: {}", summary.child_count),
        format!("stoppedBySignal: {}", summary.stopped_by_signal),
    ];

    for result in &summary.child_results {
        lines.push(format!(
            "{}  binding={}:{}  exit={}  success={}  restarts={}",
            result.env_name,
            result.binding_kind,
            result.binding_name,
            result
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            result.success,
            result.restart_count
        ));
    }

    lines
}
