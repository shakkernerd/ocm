use crate::cli::upgrade::{UpgradeBatchSummary, UpgradeEnvSummary};
use crate::infra::terminal::{
    Cell, KeyValueRow, Tone, paint, render_key_value_card, render_table, terminal_width,
};

use super::RenderProfile;

pub fn upgrade_env(
    summary: &UpgradeEnvSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return upgrade_env_raw(summary);
    }

    let mut lines = vec![paint(
        &format!("Upgrade {}", summary.env_name),
        Tone::Strong,
        profile.color,
    )];
    lines.push(String::new());
    lines.extend(render_key_value_card(
        "Result",
        &[
            KeyValueRow::accent("Env", &summary.env_name),
            KeyValueRow::plain(
                "Was using",
                format!(
                    "{}:{}",
                    summary.previous_binding_kind, summary.previous_binding_name
                ),
            ),
            KeyValueRow::plain(
                "Now using",
                format!("{}:{}", summary.binding_kind, summary.binding_name),
            ),
            KeyValueRow::new("OpenClaw", &summary.outcome, outcome_tone(&summary.outcome)),
            KeyValueRow::new(
                "Service",
                summary.service_action.as_deref().unwrap_or("unchanged"),
                service_tone(summary.service_action.as_deref()),
            ),
        ],
        profile.color,
    ));

    let mut release_rows = Vec::new();
    if let Some(version) = summary.runtime_release_version.as_deref() {
        release_rows.push(KeyValueRow::accent("Version", version));
    }
    if let Some(channel) = summary.runtime_release_channel.as_deref() {
        release_rows.push(KeyValueRow::plain("Channel", channel));
    }
    if !release_rows.is_empty() {
        lines.push(String::new());
        lines.extend(render_key_value_card(
            "Release",
            &release_rows,
            profile.color,
        ));
    }

    if let Some(note) = summary.note.as_deref() {
        lines.push(String::new());
        lines.extend(render_key_value_card(
            "Next",
            &[KeyValueRow::muted("Note", note)],
            profile.color,
        ));
    } else if matches!(
        summary.outcome.as_str(),
        "pinned" | "local-command" | "manual-runtime"
    ) {
        lines.push(String::new());
        lines.extend(render_key_value_card(
            "Next",
            &[KeyValueRow::muted(
                "Hint",
                format!("{command_example} help upgrade"),
            )],
            profile.color,
        ));
    }

    lines
}

pub fn upgrade_batch(
    summary: &UpgradeBatchSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return upgrade_batch_raw(summary);
    }

    if summary.results.is_empty() {
        return vec![paint("No environments.", Tone::Muted, profile.color)];
    }

    let wide = terminal_width().map(|width| width >= 110).unwrap_or(true);
    let rows = summary
        .results
        .iter()
        .map(|result| {
            let mut row = vec![
                Cell::accent(result.env_name.clone()),
                Cell::plain(format!("{}:{}", result.binding_kind, result.binding_name)),
                Cell::new(
                    result.outcome.clone(),
                    crate::infra::terminal::Align::Left,
                    outcome_tone(&result.outcome),
                ),
                Cell::new(
                    result
                        .service_action
                        .clone()
                        .unwrap_or_else(|| "unchanged".to_string()),
                    crate::infra::terminal::Align::Left,
                    service_tone(result.service_action.as_deref()),
                ),
            ];
            if wide {
                row.push(
                    result
                        .runtime_release_version
                        .as_deref()
                        .map(Cell::plain)
                        .unwrap_or_else(|| Cell::muted("—")),
                );
                row.push(
                    result
                        .note
                        .as_deref()
                        .map(Cell::muted)
                        .unwrap_or_else(|| Cell::muted("—")),
                );
            }
            row
        })
        .collect::<Vec<_>>();

    let mut lines = render_table(
        if wide {
            &["Env", "Using", "OpenClaw", "Service", "Version", "Notes"]
        } else {
            &["Env", "Using", "OpenClaw", "Service"]
        },
        &rows,
        profile.color,
    );
    lines.push(String::new());
    lines.extend(render_key_value_card(
        "Summary",
        &[
            KeyValueRow::accent("Checked", summary.count.to_string()),
            KeyValueRow::success("Changed", summary.changed.to_string()),
            KeyValueRow::plain("Current", summary.current.to_string()),
            KeyValueRow::warning("Skipped", summary.skipped.to_string()),
            KeyValueRow::plain("Services", summary.restarted.to_string()),
            KeyValueRow::danger("Failed", summary.failed.to_string()),
        ],
        profile.color,
    ));
    if !wide {
        lines.push(String::new());
        lines.push(paint(
            &format!("Use {command_example} upgrade <env> or --raw for notes."),
            Tone::Muted,
            profile.color,
        ));
    }
    lines
}

fn upgrade_env_raw(summary: &UpgradeEnvSummary) -> Vec<String> {
    let mut bits = vec![
        summary.env_name.clone(),
        format!(
            "from={}:{}",
            summary.previous_binding_kind, summary.previous_binding_name
        ),
        format!("to={}:{}", summary.binding_kind, summary.binding_name),
        format!("outcome={}", summary.outcome),
    ];
    if let Some(action) = summary.service_action.as_deref() {
        bits.push(format!("service={action}"));
    }
    if let Some(version) = summary.runtime_release_version.as_deref() {
        bits.push(format!("version={version}"));
    }
    if let Some(channel) = summary.runtime_release_channel.as_deref() {
        bits.push(format!("channel={channel}"));
    }
    if let Some(note) = summary.note.as_deref() {
        bits.push(format!("note={note}"));
    }
    vec![bits.join("  ")]
}

fn upgrade_batch_raw(summary: &UpgradeBatchSummary) -> Vec<String> {
    let mut lines = vec![format!(
        "checked={}  changed={}  current={}  skipped={}  restarted={}  failed={}",
        summary.count,
        summary.changed,
        summary.current,
        summary.skipped,
        summary.restarted,
        summary.failed
    )];
    for result in &summary.results {
        lines.extend(upgrade_env_raw(result));
    }
    lines
}

fn outcome_tone(outcome: &str) -> Tone {
    match outcome {
        "updated" | "switched" => Tone::Success,
        "up-to-date" => Tone::Accent,
        "pinned" | "local-command" | "manual-runtime" => Tone::Warning,
        "failed" => Tone::Danger,
        _ => Tone::Plain,
    }
}

fn service_tone(action: Option<&str>) -> Tone {
    match action {
        Some("restarted") | Some("reloaded") => Tone::Success,
        Some(_) => Tone::Warning,
        None => Tone::Muted,
    }
}
