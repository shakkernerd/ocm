use super::{RenderProfile, format_rfc3339};
use crate::infra::terminal::{Cell, Tone, paint, render_table, terminal_width};
use crate::supervisor::SupervisorView;

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
