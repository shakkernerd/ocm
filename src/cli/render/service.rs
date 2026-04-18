use super::RenderProfile;
use crate::infra::terminal::{
    Cell, KeyValueRow, Tone, paint, render_key_value_card, render_table, terminal_width,
};
use crate::service::{
    ServiceActionSummary, ServiceInstallSummary, ServiceSummary, ServiceSummaryList,
};

fn daemon_state(installed: bool, loaded: bool, running: bool) -> &'static str {
    if running {
        "running"
    } else if loaded {
        "loaded"
    } else if installed {
        "installed"
    } else {
        "absent"
    }
}

fn service_state(summary: &ServiceSummary) -> &'static str {
    if summary.running {
        "running"
    } else if summary.desired_running {
        if summary.installed {
            "pending"
        } else {
            "starting"
        }
    } else if summary.installed {
        "stopped"
    } else {
        "disabled"
    }
}

fn state_tone(state: &str) -> Tone {
    match state {
        "running" => Tone::Success,
        "pending" | "starting" | "loaded" | "installed" => Tone::Warning,
        "stopped" | "disabled" | "absent" => Tone::Muted,
        _ => Tone::Plain,
    }
}

fn optional_value_row(label: &str, value: Option<String>) -> KeyValueRow {
    match value {
        Some(value) => KeyValueRow::plain(label, value),
        None => KeyValueRow::muted(label, "—"),
    }
}

fn binding_label(summary: &ServiceSummary) -> String {
    match (
        summary.binding_kind.as_deref(),
        summary.binding_name.as_deref(),
    ) {
        (Some(kind), Some(name)) => format!("{kind}:{name}"),
        _ => "—".to_string(),
    }
}

pub fn service_list(summary: &ServiceSummaryList, profile: RenderProfile) -> Vec<String> {
    service_list_with_width(summary, profile, terminal_width())
}

fn service_list_with_width(
    summary: &ServiceSummaryList,
    profile: RenderProfile,
    _width: Option<usize>,
) -> Vec<String> {
    if !profile.pretty {
        return service_list_raw(summary);
    }

    if summary.services.is_empty() {
        return vec!["No OCM services.".to_string()];
    }

    let rows = summary
        .services
        .iter()
        .map(|service| {
            vec![
                Cell::accent(service.env_name.clone()),
                if binding_label(service) == "—" {
                    Cell::muted(binding_label(service))
                } else {
                    Cell::plain(binding_label(service))
                },
                Cell::right(service.gateway_port.to_string(), Tone::Accent),
                Cell::new(
                    service_state(service),
                    crate::infra::terminal::Align::Left,
                    state_tone(service_state(service)),
                ),
                Cell::new(
                    daemon_state(
                        service.daemon_installed,
                        service.daemon_loaded,
                        service.daemon_running,
                    ),
                    crate::infra::terminal::Align::Left,
                    state_tone(daemon_state(
                        service.daemon_installed,
                        service.daemon_loaded,
                        service.daemon_running,
                    )),
                ),
            ]
        })
        .collect::<Vec<_>>();

    let mut lines = render_table(
        &["Env", "Binding", "Port", "Service", "Daemon"],
        &rows,
        profile.color,
    );
    lines.push(String::new());
    lines.push(paint(
        &format!(
            "Supervisor daemon: {}",
            daemon_state(
                summary.daemon_installed,
                summary.daemon_loaded,
                summary.daemon_running
            )
        ),
        Tone::Muted,
        profile.color,
    ));
    lines
}

fn service_list_raw(summary: &ServiceSummaryList) -> Vec<String> {
    let mut lines = vec![format!(
        "supervisor state={}",
        daemon_state(
            summary.daemon_installed,
            summary.daemon_loaded,
            summary.daemon_running
        )
    )];
    for service in &summary.services {
        let mut bits = vec![
            service.env_name.clone(),
            format!("port={}", service.gateway_port),
            format!("state={}", service_state(service)),
            format!(
                "daemon={}",
                daemon_state(
                    service.daemon_installed,
                    service.daemon_loaded,
                    service.daemon_running
                )
            ),
        ];
        if let (Some(kind), Some(name)) = (
            service.binding_kind.as_deref(),
            service.binding_name.as_deref(),
        ) {
            bits.insert(1, format!("binding={kind}:{name}"));
        }
        if let Some(issue) = service.issue.as_deref() {
            bits.push(format!("issue={issue}"));
        }
        lines.push(bits.join("  "));
    }
    lines
}

pub fn service_status(
    summary: &ServiceSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return service_status_raw(summary);
    }

    let daemon = daemon_state(
        summary.daemon_installed,
        summary.daemon_loaded,
        summary.daemon_running,
    );
    let service = service_state(summary);

    let mut lines = vec![paint(
        &format!("Service {}", summary.env_name),
        Tone::Strong,
        profile.color,
    )];

    lines.extend(render_key_value_card(
        "Service",
        &[
            KeyValueRow::plain("Type", summary.service_kind.clone()),
            KeyValueRow::accent("Port", summary.gateway_port.to_string()),
            KeyValueRow::new("State", service, state_tone(service)),
            KeyValueRow::new("Daemon", daemon, state_tone(daemon)),
            KeyValueRow::plain("Desired running", summary.desired_running.to_string()),
        ],
        profile.color,
    ));

    lines.extend(render_key_value_card(
        "Binding",
        &[
            optional_value_row("Binding", Some(binding_label(summary))),
            optional_value_row("Command", summary.command.clone()),
            optional_value_row("Binary", summary.binary_path.clone()),
            optional_value_row("Run dir", Some(summary.run_dir.clone())),
        ],
        profile.color,
    ));

    lines.extend(render_key_value_card(
        "Runtime",
        &[
            optional_value_row("Release version", summary.runtime_release_version.clone()),
            optional_value_row("Release channel", summary.runtime_release_channel.clone()),
            optional_value_row("Source", summary.runtime_source_kind.clone()),
            optional_value_row(
                "Child pid",
                summary.child_pid.map(|value| value.to_string()),
            ),
            optional_value_row(
                "Restart count",
                summary.child_restart_count.map(|value| value.to_string()),
            ),
        ],
        profile.color,
    ));

    lines.extend(render_key_value_card(
        "Logs",
        &[
            optional_value_row("Stdout", summary.stdout_path.clone()),
            optional_value_row("Stderr", summary.stderr_path.clone()),
        ],
        profile.color,
    ));

    if let Some(issue) = summary.issue.as_deref() {
        lines.extend(render_key_value_card(
            "Issue",
            &[KeyValueRow::warning("Status", issue.to_string())],
            profile.color,
        ));
    }

    let next = service_status_next_steps(summary, command_example);
    if !next.is_empty() {
        lines.extend(render_key_value_card("Next", &next, profile.color));
    }

    lines
}

fn service_status_next_steps(summary: &ServiceSummary, command_example: &str) -> Vec<KeyValueRow> {
    if !summary.installed {
        return vec![
            KeyValueRow::accent(
                "Install",
                format!("{command_example} service install {}", summary.env_name),
            ),
            KeyValueRow::plain(
                "Start",
                format!("{command_example} service start {}", summary.env_name),
            ),
        ];
    }

    if !summary.desired_running {
        return vec![KeyValueRow::accent(
            "Start",
            format!("{command_example} service start {}", summary.env_name),
        )];
    }

    if !summary.running {
        return vec![
            KeyValueRow::accent(
                "Restart",
                format!("{command_example} service restart {}", summary.env_name),
            ),
            KeyValueRow::plain(
                "Inspect daemon",
                format!("{command_example} supervisor status"),
            ),
        ];
    }

    vec![KeyValueRow::accent(
        "Logs",
        format!("{command_example} service logs {}", summary.env_name),
    )]
}

fn service_status_raw(summary: &ServiceSummary) -> Vec<String> {
    let daemon = daemon_state(
        summary.daemon_installed,
        summary.daemon_loaded,
        summary.daemon_running,
    );
    vec![
        format!("envName: {}", summary.env_name),
        format!("serviceKind: {}", summary.service_kind),
        format!("binding: {}", binding_label(summary)),
        format!("gatewayPort: {}", summary.gateway_port),
        format!("installed: {}", summary.installed),
        format!("desiredRunning: {}", summary.desired_running),
        format!("running: {}", summary.running),
        format!("daemon: {daemon}"),
        format!(
            "childPid: {}",
            summary
                .child_pid
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "stdoutPath: {}",
            summary
                .stdout_path
                .clone()
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "stderrPath: {}",
            summary
                .stderr_path
                .clone()
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "issue: {}",
            summary.issue.clone().unwrap_or_else(|| "none".to_string())
        ),
    ]
}

pub fn service_installed(
    summary: &ServiceInstallSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    service_action(summary, profile, command_example)
}

pub fn service_action(
    summary: &ServiceActionSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return service_action_raw(summary, command_example);
    }

    let state = if summary.running {
        "running"
    } else if summary.desired_running {
        "pending"
    } else if summary.installed {
        "stopped"
    } else {
        "disabled"
    };

    let mut lines = vec![paint(
        &format!(
            "{} service {}",
            action_verb(&summary.action),
            summary.env_name
        ),
        Tone::Strong,
        profile.color,
    )];
    lines.extend(render_key_value_card(
        "Result",
        &[
            KeyValueRow::plain("Action", summary.action.clone()),
            KeyValueRow::new("State", state, state_tone(state)),
            KeyValueRow::accent("Port", summary.gateway_port.to_string()),
            optional_value_row(
                "Binding",
                summary
                    .binding_kind
                    .as_ref()
                    .zip(summary.binding_name.as_ref())
                    .map(|(kind, name)| format!("{kind}:{name}")),
            ),
        ],
        profile.color,
    ));
    lines.extend(render_key_value_card(
        "Logs",
        &[
            optional_value_row("Stdout", summary.stdout_path.clone()),
            optional_value_row("Stderr", summary.stderr_path.clone()),
        ],
        profile.color,
    ));
    if !summary.warnings.is_empty() {
        let rows = summary
            .warnings
            .iter()
            .map(|warning| KeyValueRow::warning("Warning", warning.clone()))
            .collect::<Vec<_>>();
        lines.extend(render_key_value_card("Warnings", &rows, profile.color));
    }
    if summary.running {
        lines.extend(render_key_value_card(
            "Next",
            &[KeyValueRow::accent(
                "Logs",
                format!("{command_example} service logs {}", summary.env_name),
            )],
            profile.color,
        ));
    }
    lines
}

fn service_action_raw(summary: &ServiceActionSummary, _command_example: &str) -> Vec<String> {
    vec![
        format!("envName: {}", summary.env_name),
        format!("action: {}", summary.action),
        format!("installed: {}", summary.installed),
        format!("desiredRunning: {}", summary.desired_running),
        format!("running: {}", summary.running),
        format!("gatewayPort: {}", summary.gateway_port),
        format!(
            "stdoutPath: {}",
            summary
                .stdout_path
                .clone()
                .unwrap_or_else(|| "none".to_string())
        ),
        format!(
            "stderrPath: {}",
            summary
                .stderr_path
                .clone()
                .unwrap_or_else(|| "none".to_string())
        ),
    ]
}

fn action_verb(action: &str) -> &'static str {
    match action {
        "install" => "Installed",
        "start" => "Started",
        "stop" => "Stopped",
        "restart" => "Restarted",
        "uninstall" => "Uninstalled",
        _ => "Updated",
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderProfile, service_list, service_status};
    use crate::service::{ServiceSummary, ServiceSummaryList};

    fn sample_service() -> ServiceSummary {
        ServiceSummary {
            env_name: "demo".to_string(),
            service_kind: "gateway".to_string(),
            binding_kind: Some("runtime".to_string()),
            binding_name: Some("stable".to_string()),
            command: None,
            binary_path: Some("/tmp/openclaw".to_string()),
            runtime_source_kind: Some("official".to_string()),
            runtime_release_version: Some("2026.04.01".to_string()),
            runtime_release_channel: Some("stable".to_string()),
            args: vec!["gateway".to_string()],
            run_dir: "/tmp/demo".to_string(),
            gateway_port: 18789,
            installed: true,
            loaded: true,
            running: true,
            desired_running: true,
            daemon_installed: true,
            daemon_loaded: true,
            daemon_running: true,
            daemon_pid: Some(42),
            daemon_state: Some("running".to_string()),
            child_pid: Some(99),
            child_restart_count: Some(0),
            child_port: Some(18789),
            stdout_path: Some("/tmp/stdout.log".to_string()),
            stderr_path: Some("/tmp/stderr.log".to_string()),
            issue: None,
        }
    }

    #[test]
    fn service_list_pretty_renders_table() {
        let lines = service_list(
            &ServiceSummaryList {
                daemon_label: "ocm.supervisor".to_string(),
                daemon_installed: true,
                daemon_loaded: true,
                daemon_running: true,
                daemon_pid: Some(42),
                daemon_state: Some("running".to_string()),
                services: vec![sample_service()],
            },
            RenderProfile::pretty(false),
        );
        assert!(lines.iter().any(|line| line.contains("demo")));
        assert!(lines.iter().any(|line| line.contains("Supervisor daemon")));
    }

    #[test]
    fn service_status_pretty_shows_logs_next_step() {
        let lines = service_status(&sample_service(), RenderProfile::pretty(false), "ocm");
        assert!(lines.iter().any(|line| line.contains("service logs demo")));
    }
}
