use super::RenderProfile;
use crate::infra::terminal::{
    Cell, KeyValueRow, Tone, paint, render_key_value_card, render_table, terminal_width,
};
use crate::service::{
    DiscoveredServiceList, ServiceActionSummary, ServiceAdoptionSummary, ServiceInstallSummary,
    ServiceRestoreSummary, ServiceSummary, ServiceSummaryList,
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

fn global_relation(summary: &ServiceSummary) -> &'static str {
    if summary.global_matches_env {
        "match"
    } else if summary.global_running {
        "running-other"
    } else if summary.global_loaded {
        "loaded-other"
    } else if summary.global_installed {
        "installed-other"
    } else {
        "absent"
    }
}

fn openclaw_service_relation(state: &str) -> String {
    match state {
        "match" => "this env".to_string(),
        "running-other" | "loaded-other" | "installed-other" => "another env".to_string(),
        "absent" => "none".to_string(),
        _ => state.to_string(),
    }
}

fn openclaw_service_tone(state: &str) -> Tone {
    match state {
        "match" => Tone::Success,
        "running-other" | "loaded-other" | "installed-other" => Tone::Warning,
        "absent" => Tone::Muted,
        _ => Tone::Plain,
    }
}

fn state_tone(state: &str) -> Tone {
    match state {
        "running" | "match" => Tone::Success,
        "loaded" | "installed" | "loaded-other" | "installed-other" | "running-other" => {
            Tone::Warning
        }
        "absent" => Tone::Muted,
        _ => Tone::Plain,
    }
}

pub fn service_list(summary: &ServiceSummaryList, profile: RenderProfile) -> Vec<String> {
    service_list_with_width(summary, profile, terminal_width())
}

fn service_list_with_width(
    summary: &ServiceSummaryList,
    profile: RenderProfile,
    width: Option<usize>,
) -> Vec<String> {
    if !profile.pretty {
        return service_list_raw(summary);
    }

    let global_state = daemon_state(
        summary.global_installed,
        summary.global_loaded,
        summary.global_running,
    );
    let mut lines = vec![format!(
        "{}  {}  {}",
        paint("OpenClaw service", Tone::Strong, profile.color),
        paint(&summary.global_label, Tone::Accent, profile.color),
        paint(global_state, state_tone(global_state), profile.color)
    )];
    if let Some(config_path) = summary.global_config_path.as_deref() {
        lines.push(format!(
            "{} {}",
            paint("config", Tone::Muted, profile.color),
            paint(config_path, Tone::Muted, profile.color)
        ));
    }

    if summary.services.is_empty() {
        lines.push("No services.".to_string());
        return lines;
    }

    let show_notes = width.map(|width| width >= 110).unwrap_or(true);
    let rows = summary
        .services
        .iter()
        .map(|service| {
            let binding = match (
                service.binding_kind.as_deref(),
                service.binding_name.as_deref(),
            ) {
                (Some(kind), Some(name)) => format!("{kind}:{name}"),
                _ => "—".to_string(),
            };
            let mut notes = Vec::new();
            if service.can_adopt_global {
                notes.push("adopt-ready");
            }
            if service.can_restore_global {
                notes.push("restore-ready");
            }
            if service.backup_available {
                notes.push("backup");
            }
            if service.issue.is_some() {
                notes.push("issue");
            }

            let managed_state = daemon_state(service.installed, service.loaded, service.running);
            let global_state = global_relation(service);
            let mut row = vec![
                Cell::accent(service.env_name.clone()),
                if binding == "—" {
                    Cell::muted(binding)
                } else {
                    Cell::plain(binding)
                },
                Cell::right(service.gateway_port.to_string(), Tone::Accent),
                Cell::new(
                    managed_state,
                    crate::infra::terminal::Align::Left,
                    state_tone(managed_state),
                ),
                Cell::new(
                    openclaw_service_relation(global_state),
                    crate::infra::terminal::Align::Left,
                    openclaw_service_tone(global_state),
                ),
            ];
            if show_notes {
                row.push(if notes.is_empty() {
                    Cell::muted("—")
                } else if service.issue.is_some() {
                    Cell::danger(notes.join(","))
                } else {
                    Cell::warning(notes.join(","))
                });
            }
            row
        })
        .collect::<Vec<_>>();
    lines.extend(render_table(
        if show_notes {
            &["Env", "Binding", "Port", "OCM", "OpenClaw", "Notes"]
        } else {
            &["Env", "Binding", "Port", "OCM", "OpenClaw"]
        },
        &rows,
        profile.color,
    ));
    if !show_notes {
        lines.push(String::new());
        lines.push(paint(
            "Use service status <env> or --raw for notes and readiness details.",
            Tone::Muted,
            profile.color,
        ));
    }
    lines
}

fn service_list_raw(summary: &ServiceSummaryList) -> Vec<String> {
    let mut lines = vec![format!(
        "Global service {}  state={}",
        summary.global_label,
        daemon_state(
            summary.global_installed,
            summary.global_loaded,
            summary.global_running
        )
    )];
    if let Some(config_path) = summary.global_config_path.as_deref() {
        lines.push(format!("globalConfigPath: {config_path}"));
    }

    if summary.services.is_empty() {
        lines.push("No services.".to_string());
        return lines;
    }

    for service in &summary.services {
        let mut bits = vec![
            service.env_name.clone(),
            format!("port={}", service.gateway_port),
            format!(
                "managed={}",
                daemon_state(service.installed, service.loaded, service.running)
            ),
            format!("global={}", global_relation(service)),
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
        if service.can_adopt_global {
            bits.push("adopt=ready".to_string());
        }
        if service.can_restore_global {
            bits.push("restore=ready".to_string());
        }
        if service.backup_available {
            bits.push("backup=present".to_string());
        }
        lines.push(bits.join("  "));
    }

    lines
}

pub fn service_status(summary: &ServiceSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return service_status_raw(summary);
    }

    let managed_state = daemon_state(summary.installed, summary.loaded, summary.running);
    let global_state = global_relation(summary);
    let mut lines = vec![paint(
        &format!("Service {}", summary.env_name),
        Tone::Strong,
        profile.color,
    )];

    push_card(
        &mut lines,
        "Status",
        vec![
            KeyValueRow::plain("Type", summary.service_kind.clone()),
            KeyValueRow::accent("Port", summary.gateway_port.to_string()),
            bool_row("Managed by OCM", summary.installed),
            KeyValueRow::new("OCM service", managed_state, state_tone(managed_state)),
            KeyValueRow::new(
                "OpenClaw service",
                openclaw_service_relation(global_state),
                openclaw_service_tone(global_state),
            ),
            optional_value_row(
                "Binding",
                summary
                    .binding_kind
                    .as_deref()
                    .zip(summary.binding_name.as_deref())
                    .map(|(kind, name)| format!("{kind}:{name}")),
            ),
        ],
        profile.color,
    );

    push_card(
        &mut lines,
        "OCM service",
        vec![
            KeyValueRow::plain("Label", summary.managed_label.clone()),
            KeyValueRow::plain("Plist", summary.managed_plist_path.clone()),
            optional_value_row("PID", summary.pid.map(|pid| pid.to_string())),
            optional_value_row("Launchd state", summary.state.clone()),
        ],
        profile.color,
    );

    let mut launch = vec![KeyValueRow::plain("Run dir", summary.run_dir.clone())];
    if let Some(command) = summary.command.as_ref() {
        launch.push(KeyValueRow::accent("Command", command.clone()));
    }
    if let Some(binary_path) = summary.binary_path.as_ref() {
        launch.push(KeyValueRow::accent("Binary", binary_path.clone()));
    }
    if !summary.args.is_empty() {
        launch.push(KeyValueRow::plain("Args", summary.args.join(" ")));
    }
    push_card(&mut lines, "Launch", launch, profile.color);

    push_card(
        &mut lines,
        "OpenClaw service",
        vec![
            KeyValueRow::plain("Label", summary.global_label.clone()),
            KeyValueRow::new(
                "This env",
                if summary.global_matches_env {
                    "yes"
                } else {
                    "no"
                },
                if summary.global_matches_env {
                    Tone::Success
                } else {
                    Tone::Muted
                },
            ),
            optional_value_row("PID", summary.global_pid.map(|pid| pid.to_string())),
            optional_value_row("Current config", summary.global_config_path.clone()),
            bool_row("Backup available", summary.backup_available),
            bool_row("Move to OCM", summary.can_adopt_global),
            bool_row("Can restore", summary.can_restore_global),
            optional_value_row("Latest backup", summary.latest_backup_plist_path.clone()),
        ],
        profile.color,
    );

    if let Some(issue) = summary.issue.as_ref() {
        push_card(
            &mut lines,
            "Issue",
            vec![KeyValueRow::danger("Problem", issue.clone())],
            profile.color,
        );
    }

    lines
}

fn service_status_raw(summary: &ServiceSummary) -> Vec<String> {
    let global_state = global_relation(summary);
    let mut lines = vec![
        format!("envName: {}", summary.env_name),
        format!("serviceKind: {}", summary.service_kind),
        format!("managedLabel: {}", summary.managed_label),
        format!("managedPlistPath: {}", summary.managed_plist_path),
        format!("globalLabel: {}", summary.global_label),
        format!("gatewayPort: {}", summary.gateway_port),
        format!(
            "managedState: {}",
            daemon_state(summary.installed, summary.loaded, summary.running)
        ),
        format!("globalState: {}", global_state),
        format!("globalMatchesEnv: {}", summary.global_matches_env),
        format!("backupAvailable: {}", summary.backup_available),
        format!("canAdoptGlobal: {}", summary.can_adopt_global),
        format!("canRestoreGlobal: {}", summary.can_restore_global),
    ];

    if let (Some(kind), Some(name)) = (
        summary.binding_kind.as_deref(),
        summary.binding_name.as_deref(),
    ) {
        lines.push(format!("binding: {kind}:{name}"));
    }
    if let Some(command) = summary.command.as_deref() {
        lines.push(format!("command: {command}"));
    }
    if let Some(binary_path) = summary.binary_path.as_deref() {
        lines.push(format!("binaryPath: {binary_path}"));
    }
    if !summary.args.is_empty() {
        lines.push(format!("args: {}", summary.args.join(" ")));
    }
    lines.push(format!("runDir: {}", summary.run_dir));
    if let Some(pid) = summary.pid {
        lines.push(format!("managedPid: {pid}"));
    }
    if let Some(state) = summary.state.as_deref() {
        lines.push(format!("managedLaunchdState: {state}"));
    }
    if let Some(global_pid) = summary.global_pid {
        lines.push(format!("globalPid: {global_pid}"));
    }
    if let Some(global_config_path) = summary.global_config_path.as_deref() {
        lines.push(format!("globalConfigPath: {global_config_path}"));
    }
    if let Some(latest_backup_plist_path) = summary.latest_backup_plist_path.as_deref() {
        lines.push(format!("latestBackupPlistPath: {latest_backup_plist_path}"));
    }
    if let Some(issue) = summary.issue.as_deref() {
        lines.push(format!("issue: {issue}"));
    }

    lines
}

pub fn service_discover(summary: &DiscoveredServiceList, profile: RenderProfile) -> Vec<String> {
    service_discover_with_width(summary, profile, terminal_width())
}

fn service_discover_with_width(
    summary: &DiscoveredServiceList,
    profile: RenderProfile,
    width: Option<usize>,
) -> Vec<String> {
    if summary.services.is_empty() {
        return vec!["No OpenClaw services discovered.".to_string()];
    }
    if !profile.pretty {
        return service_discover_raw(summary);
    }

    let show_command = width.map(|width| width >= 110).unwrap_or(true);
    let rows = summary
        .services
        .iter()
        .map(|service| {
            let state = daemon_state(service.installed, service.loaded, service.running);
            let adopt = if service.adoptable { "ready" } else { "—" };
            let mut row = vec![
                Cell::accent(service.label.clone()),
                Cell::plain(pretty_source_kind(&service.source_kind)),
                Cell::new(
                    state,
                    crate::infra::terminal::Align::Left,
                    state_tone(state),
                ),
                service
                    .gateway_port
                    .map(|port| Cell::right(port.to_string(), Tone::Accent))
                    .unwrap_or_else(|| Cell::muted("—")),
                service
                    .matched_env_name
                    .as_deref()
                    .map(Cell::accent)
                    .unwrap_or_else(|| Cell::muted("—")),
                if adopt == "—" {
                    Cell::muted(adopt)
                } else {
                    Cell::warning("yes")
                },
            ];
            if show_command {
                let command = if !service.program_arguments.is_empty() {
                    service.program_arguments.join(" ")
                } else {
                    service.program.clone().unwrap_or_else(|| "—".to_string())
                };
                row.push(if command == "—" {
                    Cell::muted(command)
                } else {
                    Cell::plain(command)
                });
            }
            row
        })
        .collect::<Vec<_>>();
    let mut lines = render_table(
        if show_command {
            &[
                "Label",
                "Managed by",
                "State",
                "Port",
                "Env",
                "Move",
                "Command",
            ]
        } else {
            &["Label", "Managed by", "State", "Port", "Env", "Move"]
        },
        &rows,
        profile.color,
    );
    if !show_command {
        lines.push(String::new());
        lines.push(paint(
            "Use --raw or --json for full command details.",
            Tone::Muted,
            profile.color,
        ));
    }
    lines
}

fn pretty_source_kind(source_kind: &str) -> String {
    match source_kind {
        "openclaw-global" => "OpenClaw".to_string(),
        "ocm-managed" => "OCM".to_string(),
        "foreign" => "Other".to_string(),
        other => other.to_string(),
    }
}

fn service_discover_raw(summary: &DiscoveredServiceList) -> Vec<String> {
    let mut lines = Vec::new();
    for service in &summary.services {
        lines.push(format!(
            "{}  source={}  state={}",
            service.label,
            service.source_kind,
            daemon_state(service.installed, service.loaded, service.running)
        ));
        lines.push(format!("  plist: {}", service.plist_path));
        if let Some(config_path) = service.config_path.as_deref() {
            lines.push(format!("  config: {config_path}"));
        }
        if let Some(state_dir) = service.state_dir.as_deref() {
            lines.push(format!("  stateDir: {state_dir}"));
        }
        if let Some(openclaw_home) = service.openclaw_home.as_deref() {
            lines.push(format!("  openclawHome: {openclaw_home}"));
        }
        if let Some(gateway_port) = service.gateway_port {
            lines.push(format!("  port: {gateway_port}"));
        }
        if let Some(program) = service.program.as_deref() {
            lines.push(format!("  program: {program}"));
        }
        if !service.program_arguments.is_empty() {
            lines.push(format!(
                "  programArguments: {}",
                service.program_arguments.join(" | ")
            ));
        }
        if let Some(working_directory) = service.working_directory.as_deref() {
            lines.push(format!("  workingDirectory: {working_directory}"));
        }
        if let Some(matched_env_name) = service.matched_env_name.as_deref() {
            lines.push(format!("  matchedEnv: {matched_env_name}"));
            if service.adoptable {
                lines.push(format!("  adopt: service adopt-global {matched_env_name}"));
            }
        }
        if let Some(reason) = service.adopt_reason.as_deref() {
            lines.push(format!("  note: {reason}"));
        }
    }
    lines
}

pub fn service_installed(summary: &ServiceInstallSummary) -> Vec<String> {
    let mut lines = vec![
        format!("Installed service {}", summary.env_name),
        format!("  label: {}", summary.managed_label),
        format!("  plist: {}", summary.managed_plist_path),
        format!("  port: {}", summary.gateway_port),
        format!(
            "  binding: {}:{}",
            summary.binding_kind, summary.binding_name
        ),
        format!("  run dir: {}", summary.run_dir),
        format!("  stdout: {}", summary.stdout_path),
        format!("  stderr: {}", summary.stderr_path),
    ];
    if let Some(command) = summary.command.as_deref() {
        lines.push(format!("  command: {command}"));
    }
    if let Some(binary_path) = summary.binary_path.as_deref() {
        lines.push(format!("  binary path: {binary_path}"));
    }
    if !summary.args.is_empty() {
        lines.push(format!("  args: {}", summary.args.join(" ")));
    }
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
}

pub fn service_adopted(summary: &ServiceAdoptionSummary) -> Vec<String> {
    let mut lines = vec![
        if summary.dry_run {
            format!("Would adopt global service {}", summary.env_name)
        } else {
            format!("Adopted global service {}", summary.env_name)
        },
        format!("  global label: {}", summary.global_label),
        format!("  global plist: {}", summary.global_plist_path),
        format!("  backup plist: {}", summary.backup_plist_path),
        format!("  managed label: {}", summary.managed_label),
        format!("  managed plist: {}", summary.managed_plist_path),
        format!("  port: {}", summary.gateway_port),
    ];
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
}

pub fn service_restored(summary: &ServiceRestoreSummary) -> Vec<String> {
    let mut lines = vec![
        if summary.dry_run {
            format!("Would restore global service {}", summary.env_name)
        } else {
            format!("Restored global service {}", summary.env_name)
        },
        format!("  global label: {}", summary.global_label),
        format!("  global plist: {}", summary.global_plist_path),
        format!("  backup plist: {}", summary.backup_plist_path),
        format!("  managed label: {}", summary.managed_label),
        format!("  managed plist: {}", summary.managed_plist_path),
        format!("  port: {}", summary.gateway_port),
    ];
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
}

pub fn service_action(summary: &ServiceActionSummary) -> Vec<String> {
    let title = match summary.action.as_str() {
        "start" => "Started",
        "stop" => "Stopped",
        "restart" => "Restarted",
        "uninstall" => "Uninstalled",
        _ => "Updated",
    };
    let mut lines = vec![
        format!("{title} service {}", summary.env_name),
        format!("  label: {}", summary.managed_label),
        format!("  plist: {}", summary.managed_plist_path),
    ];
    if let Some(port) = summary.gateway_port {
        lines.push(format!("  port: {port}"));
    }
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
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

#[cfg(test)]
mod tests {
    use super::{
        RenderProfile, service_discover_with_width, service_list, service_list_with_width,
        service_status,
    };
    use crate::service::{
        DiscoveredServiceList, DiscoveredServiceSummary, ServiceSummary, ServiceSummaryList,
    };

    #[test]
    fn service_list_pretty_uses_a_table() {
        let lines = service_list(
            &ServiceSummaryList {
                global_label: "ai.openclaw.gateway".to_string(),
                global_installed: true,
                global_loaded: true,
                global_running: false,
                global_pid: None,
                global_config_path: Some("/tmp/demo/.openclaw/openclaw.json".to_string()),
                services: vec![ServiceSummary {
                    env_name: "demo".to_string(),
                    service_kind: "gateway".to_string(),
                    managed_label: "ai.openclaw.gateway.ocm.demo".to_string(),
                    managed_plist_path: "/tmp/demo.plist".to_string(),
                    global_label: "ai.openclaw.gateway".to_string(),
                    binding_kind: Some("launcher".to_string()),
                    binding_name: Some("stable".to_string()),
                    command: Some("openclaw gateway run".to_string()),
                    binary_path: None,
                    args: Vec::new(),
                    run_dir: "/tmp/demo".to_string(),
                    gateway_port: 18789,
                    installed: true,
                    loaded: true,
                    running: true,
                    pid: Some(12),
                    state: Some("running".to_string()),
                    global_installed: true,
                    global_loaded: true,
                    global_running: false,
                    global_pid: None,
                    global_matches_env: true,
                    global_config_path: Some("/tmp/demo/.openclaw/openclaw.json".to_string()),
                    latest_backup_plist_path: None,
                    backup_available: false,
                    can_adopt_global: false,
                    can_restore_global: false,
                    issue: None,
                }],
            },
            RenderProfile::pretty(false),
        );

        assert!(lines[0].contains("OpenClaw service"));
        assert!(lines[2].starts_with('┌'));
        assert!(lines[3].contains("Env"));
        assert!(lines[3].contains("OCM"));
        assert!(lines[3].contains("OpenClaw"));
        assert!(lines[5].contains("demo"));
        assert!(lines[6].starts_with('└'));
    }

    #[test]
    fn service_status_uses_relation_style_global_state() {
        let lines = service_status(&sample_service_summary(), RenderProfile::raw());

        assert!(lines.contains(&"globalState: loaded-other".to_string()));
        assert!(lines.contains(&"globalMatchesEnv: false".to_string()));
    }

    #[test]
    fn service_status_pretty_uses_cards() {
        let lines = service_status(&sample_service_summary(), RenderProfile::pretty(false));

        assert_eq!(lines[0], "Service demo");
        assert!(lines.iter().any(|line| line.contains("Managed by OCM")));
        assert!(lines.iter().any(|line| line.contains("OCM service")));
        assert!(lines.iter().any(|line| line.contains("OpenClaw service")));
    }

    #[test]
    fn service_list_pretty_compacts_on_narrow_terminals() {
        let lines = service_list_with_width(
            &ServiceSummaryList {
                global_label: "ai.openclaw.gateway".to_string(),
                global_installed: true,
                global_loaded: true,
                global_running: false,
                global_pid: None,
                global_config_path: Some("/tmp/demo/.openclaw/openclaw.json".to_string()),
                services: vec![sample_service_summary()],
            },
            RenderProfile::pretty(false),
            Some(90),
        );

        assert!(lines[2].starts_with('┌'));
        assert!(lines[3].contains("Binding"));
        assert!(lines[3].contains("OCM"));
        assert!(lines[3].contains("OpenClaw"));
        assert!(!lines[3].contains("Notes"));
        assert_eq!(
            lines.last().unwrap(),
            "Use service status <env> or --raw for notes and readiness details."
        );
    }

    #[test]
    fn service_discover_pretty_compacts_on_narrow_terminals() {
        let lines = service_discover_with_width(
            &sample_discovered_service_list(),
            RenderProfile::pretty(false),
            Some(80),
        );

        assert!(lines[1].contains("Managed by"));
        assert!(!lines[1].contains("Command"));
        assert_eq!(
            lines.last().unwrap(),
            "Use --raw or --json for full command details."
        );
    }

    #[test]
    fn service_discover_pretty_keeps_command_on_wide_terminals() {
        let lines = service_discover_with_width(
            &sample_discovered_service_list(),
            RenderProfile::pretty(false),
            Some(140),
        );

        assert!(lines[1].contains("Command"));
        assert!(lines.iter().any(|line| line.contains("/bin/sh -lc")));
    }

    fn sample_service_summary() -> ServiceSummary {
        ServiceSummary {
            env_name: "demo".to_string(),
            service_kind: "gateway".to_string(),
            managed_label: "ai.openclaw.gateway.ocm.demo".to_string(),
            managed_plist_path: "/tmp/demo.plist".to_string(),
            global_label: "ai.openclaw.gateway".to_string(),
            binding_kind: Some("launcher".to_string()),
            binding_name: Some("stable".to_string()),
            command: Some("openclaw gateway run".to_string()),
            binary_path: None,
            args: Vec::new(),
            run_dir: "/tmp/demo".to_string(),
            gateway_port: 18789,
            installed: true,
            loaded: true,
            running: false,
            pid: None,
            state: Some("loaded".to_string()),
            global_installed: true,
            global_loaded: true,
            global_running: false,
            global_pid: None,
            global_matches_env: false,
            global_config_path: Some("/tmp/other/.openclaw/openclaw.json".to_string()),
            latest_backup_plist_path: None,
            backup_available: false,
            can_adopt_global: false,
            can_restore_global: false,
            issue: None,
        }
    }

    fn sample_discovered_service_list() -> DiscoveredServiceList {
        DiscoveredServiceList {
            services: vec![DiscoveredServiceSummary {
                label: "ai.openclaw.gateway.ocm.demo".to_string(),
                plist_path: "/tmp/demo.plist".to_string(),
                source_kind: "ocm-managed".to_string(),
                installed: true,
                loaded: true,
                running: false,
                pid: None,
                state: Some("loaded".to_string()),
                config_path: Some("/tmp/demo/.openclaw/openclaw.json".to_string()),
                state_dir: Some("/tmp/demo/.openclaw".to_string()),
                openclaw_home: Some("/tmp/demo".to_string()),
                gateway_port: Some(18789),
                program: Some("/bin/sh".to_string()),
                program_arguments: vec![
                    "/bin/sh".to_string(),
                    "-lc".to_string(),
                    "pnpm openclaw gateway run --port 18789".to_string(),
                ],
                working_directory: Some("/tmp/demo".to_string()),
                matched_env_name: Some("demo".to_string()),
                adoptable: false,
                adopt_reason: None,
            }],
        }
    }
}
