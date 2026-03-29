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

fn state_tone(state: &str) -> Tone {
    match state {
        "running" | "match" | "healthy" => Tone::Success,
        "auth-required" => Tone::Warning,
        "loaded" | "installed" | "loaded-other" | "installed-other" | "running-other" => {
            Tone::Warning
        }
        "responding-but-invalid" | "wrong-service" | "unreachable" => Tone::Danger,
        "stopped" | "unknown" => Tone::Muted,
        "absent" => Tone::Muted,
        _ => Tone::Plain,
    }
}

fn source_kind_tone(source_kind: &str) -> Tone {
    match source_kind {
        "ocm-managed" => Tone::Success,
        "openclaw-global" => Tone::Warning,
        "foreign" => Tone::Accent,
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

    if summary.services.is_empty() {
        let mut lines = vec!["No OCM services.".to_string()];
        if let Some(note) = service_list_openclaw_note(summary, profile) {
            lines.push(String::new());
            lines.push(note);
        }
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
                    service.openclaw_state.clone(),
                    crate::infra::terminal::Align::Left,
                    state_tone(&service.openclaw_state),
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
    let mut lines = render_table(
        if show_notes {
            &["Env", "Binding", "Port", "Service", "OpenClaw", "Notes"]
        } else {
            &["Env", "Binding", "Port", "Service", "OpenClaw"]
        },
        &rows,
        profile.color,
    );
    if !show_notes {
        lines.push(String::new());
        lines.push(paint(
            "Use service status <env> or --raw for notes and readiness details.",
            Tone::Muted,
            profile.color,
        ));
    }
    if let Some(note) = service_list_openclaw_note(summary, profile) {
        lines.push(String::new());
        lines.push(note);
    }
    lines
}

fn service_list_openclaw_note(
    summary: &ServiceSummaryList,
    profile: RenderProfile,
) -> Option<String> {
    if !summary.global_installed {
        return None;
    }

    let message = match summary.global_env_name.as_deref() {
        Some(env_name) => format!(
            "Separate OpenClaw service detected for env {env_name}; use service discover for details."
        ),
        None => "Separate OpenClaw service detected; use service discover for details.".to_string(),
    };
    Some(paint(&message, Tone::Muted, profile.color))
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
    if let Some(env_name) = summary.global_env_name.as_deref() {
        lines.push(format!("globalEnvName: {env_name}"));
    }
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
            format!("openclaw={}", service.openclaw_state),
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

pub fn service_status(
    summary: &ServiceSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return service_status_raw(summary);
    }

    let managed_state = daemon_state(summary.installed, summary.loaded, summary.running);
    let mut lines = vec![paint(
        &format!("Service {}", summary.env_name),
        Tone::Strong,
        profile.color,
    )];

    let mut status_rows = vec![
        KeyValueRow::plain("Type", summary.service_kind.clone()),
        KeyValueRow::accent("Port", summary.gateway_port.to_string()),
        KeyValueRow::new("Service", managed_state, state_tone(managed_state)),
        KeyValueRow::new(
            "OpenClaw",
            summary.openclaw_state.clone(),
            state_tone(&summary.openclaw_state),
        ),
        optional_value_row(
            "Binding",
            summary
                .binding_kind
                .as_deref()
                .zip(summary.binding_name.as_deref())
                .map(|(kind, name)| format!("{kind}:{name}")),
        ),
    ];
    if let Some(detail) = summary.openclaw_detail.as_ref() {
        status_rows.push(KeyValueRow::muted("Detail", detail.clone()));
    }
    push_card(&mut lines, "Status", status_rows, profile.color);

    push_card(
        &mut lines,
        "OCM service",
        vec![
            KeyValueRow::plain("Label", summary.managed_label.clone()),
            KeyValueRow::plain("Service file", summary.managed_plist_path.clone()),
            optional_value_row("PID", summary.pid.map(|pid| pid.to_string())),
            optional_value_row("Manager state", summary.state.clone()),
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

    if summary.global_matches_env || summary.can_adopt_global || summary.can_restore_global {
        push_card(
            &mut lines,
            "OpenClaw service",
            vec![
                KeyValueRow::plain("Label", summary.global_label.clone()),
                optional_value_row("Env", summary.global_env_name.clone()),
                KeyValueRow::new(
                    "State",
                    daemon_state(
                        summary.global_installed,
                        summary.global_loaded,
                        summary.global_running,
                    ),
                    state_tone(daemon_state(
                        summary.global_installed,
                        summary.global_loaded,
                        summary.global_running,
                    )),
                ),
                optional_value_row("PID", summary.global_pid.map(|pid| pid.to_string())),
                optional_value_row("Current config", summary.global_config_path.clone()),
                available_row("Backup available", summary.backup_available),
                action_row("Move to OCM", summary.can_adopt_global),
                action_row("Can restore", summary.can_restore_global),
                optional_value_row("Latest backup", summary.latest_backup_plist_path.clone()),
            ],
            profile.color,
        );
    }

    if let Some(issue) = summary.issue.as_ref() {
        push_card(
            &mut lines,
            "Issue",
            vec![KeyValueRow::danger("Problem", issue.clone())],
            profile.color,
        );
    }

    let next_steps = service_status_next_steps(summary, command_example);
    if !next_steps.is_empty() {
        push_card(&mut lines, "Next", next_steps, profile.color);
    }

    lines
}

fn service_status_next_steps(summary: &ServiceSummary, command_example: &str) -> Vec<KeyValueRow> {
    if summary.can_adopt_global && !summary.installed {
        return vec![KeyValueRow::warning(
            "Move to OCM",
            format!(
                "{command_example} service adopt-global {}",
                summary.env_name
            ),
        )];
    }

    if summary.can_restore_global && !summary.global_installed {
        return vec![KeyValueRow::warning(
            "Restore",
            format!(
                "{command_example} service restore-global {}",
                summary.env_name
            ),
        )];
    }

    if summary.binding_kind.is_none() {
        return vec![KeyValueRow::accent(
            "Start",
            format!("{command_example} start {}", summary.env_name),
        )];
    }

    if summary.binding_kind.as_deref() == Some("launcher") && summary.command.is_none() {
        return vec![
            KeyValueRow::accent("List launchers", format!("{command_example} launcher list")),
            KeyValueRow::warning(
                "Rebind",
                format!(
                    "{command_example} env set-launcher {} <launcher>",
                    summary.env_name
                ),
            ),
        ];
    }

    if summary.binding_kind.as_deref() == Some("runtime") && summary.issue.is_some() {
        let mut rows = Vec::new();
        if let Some(runtime_name) = summary.binding_name.as_deref() {
            rows.push(KeyValueRow::accent(
                "Check runtime",
                format!("{command_example} runtime verify {runtime_name}"),
            ));
        }
        if !rows.is_empty() {
            return rows;
        }
    }

    match daemon_state(summary.installed, summary.loaded, summary.running) {
        "absent" => {
            return vec![KeyValueRow::accent(
                "Install",
                format!("{command_example} service install {}", summary.env_name),
            )];
        }
        "installed" => {
            return vec![KeyValueRow::accent(
                "Start",
                format!("{command_example} service start {}", summary.env_name),
            )];
        }
        "loaded" | "running" => match summary.openclaw_state.as_str() {
            "auth-required" => {
                return vec![
                    KeyValueRow::warning(
                        "Repair",
                        format!("{command_example} @{} -- onboard", summary.env_name),
                    ),
                    KeyValueRow::accent(
                        "Logs",
                        format!(
                            "{command_example} service logs {} --tail 50",
                            summary.env_name
                        ),
                    ),
                ];
            }
            "stopped" | "unreachable" | "responding-but-invalid" | "wrong-service" => {
                return vec![
                    KeyValueRow::warning(
                        "Restart",
                        format!("{command_example} service restart {}", summary.env_name),
                    ),
                    KeyValueRow::accent(
                        "Logs",
                        format!(
                            "{command_example} service logs {} --tail 50",
                            summary.env_name
                        ),
                    ),
                ];
            }
            _ => {}
        },
        _ => {}
    }

    Vec::new()
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
        format!("openclawState: {}", summary.openclaw_state),
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
        lines.push(format!("managedStateDetail: {state}"));
    }
    if let Some(global_pid) = summary.global_pid {
        lines.push(format!("globalPid: {global_pid}"));
    }
    if let Some(global_config_path) = summary.global_config_path.as_deref() {
        lines.push(format!("globalConfigPath: {global_config_path}"));
    }
    if let Some(openclaw_detail) = summary.openclaw_detail.as_deref() {
        lines.push(format!("openclawDetail: {openclaw_detail}"));
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
                Cell::new(
                    pretty_source_kind(&service.source_kind),
                    crate::infra::terminal::Align::Left,
                    source_kind_tone(&service.source_kind),
                ),
                Cell::new(
                    state,
                    crate::infra::terminal::Align::Left,
                    state_tone(state),
                ),
                Cell::new(
                    service.openclaw_state.clone(),
                    crate::infra::terminal::Align::Left,
                    state_tone(&service.openclaw_state),
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
                    Cell::warning("ready")
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
                "Service",
                "OpenClaw",
                "Port",
                "Env",
                "Move",
                "Command",
            ]
        } else {
            &[
                "Label",
                "Managed by",
                "Service",
                "OpenClaw",
                "Port",
                "Env",
                "Move",
            ]
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
        lines.push(format!("  openclawState: {}", service.openclaw_state));
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

pub fn service_installed(
    summary: &ServiceInstallSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return service_installed_raw(summary, command_example);
    }

    let mut lines = vec![paint("Service installed", Tone::Strong, profile.color)];
    let mut service_rows = vec![
        KeyValueRow::accent("Env", summary.env_name.clone()),
        KeyValueRow::plain("Type", summary.service_kind.clone()),
        KeyValueRow::accent("Port", summary.gateway_port.to_string()),
        KeyValueRow::accent(
            "Binding",
            format!("{}:{}", summary.binding_kind, summary.binding_name),
        ),
    ];
    if summary.persisted_gateway_port {
        service_rows.push(KeyValueRow::success("Port saved", "yes"));
    }
    push_card(&mut lines, "Service", service_rows, profile.color);

    let mut next = vec![
        KeyValueRow::accent(
            "Start",
            format!("{command_example} service start {}", summary.env_name),
        ),
        KeyValueRow::accent(
            "Status",
            format!("{command_example} service status {}", summary.env_name),
        ),
        KeyValueRow::accent(
            "Logs",
            format!(
                "{command_example} service logs {} --tail 50",
                summary.env_name
            ),
        ),
    ];
    if !summary.warnings.is_empty() {
        next.push(KeyValueRow::warning(
            "Warnings",
            format!("{} warning(s)", summary.warnings.len()),
        ));
    }
    push_card(&mut lines, "Next", next, profile.color);
    if !summary.warnings.is_empty() {
        let warnings = summary
            .warnings
            .iter()
            .enumerate()
            .map(|(index, warning)| KeyValueRow::warning(format!("#{}", index + 1), warning))
            .collect::<Vec<_>>();
        push_card(&mut lines, "Warnings", warnings, profile.color);
    }
    lines
}

fn service_installed_raw(summary: &ServiceInstallSummary, command_example: &str) -> Vec<String> {
    let mut lines = vec![
        format!("Installed service {}", summary.env_name),
        format!("  label: {}", summary.managed_label),
        format!("  service file: {}", summary.managed_plist_path),
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
    lines.push(format!(
        "  start: {command_example} service start {}",
        summary.env_name
    ));
    lines.push(format!(
        "  status: {command_example} service status {}",
        summary.env_name
    ));
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
}

pub fn service_adopted(summary: &ServiceAdoptionSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return service_adopted_raw(summary);
    }

    let mut lines = vec![paint(
        if summary.dry_run {
            "Global service move planned"
        } else {
            "Global service moved"
        },
        Tone::Strong,
        profile.color,
    )];
    push_card(
        &mut lines,
        "Service",
        vec![
            KeyValueRow::accent("Env", summary.env_name.clone()),
            KeyValueRow::plain("Port", summary.gateway_port.to_string()),
            KeyValueRow::plain("OpenClaw label", summary.global_label.clone()),
            KeyValueRow::plain("OCM label", summary.managed_label.clone()),
        ],
        profile.color,
    );
    if !summary.warnings.is_empty() {
        let warnings = summary
            .warnings
            .iter()
            .enumerate()
            .map(|(index, warning)| KeyValueRow::warning(format!("#{}", index + 1), warning))
            .collect::<Vec<_>>();
        push_card(&mut lines, "Warnings", warnings, profile.color);
    }
    lines
}

fn service_adopted_raw(summary: &ServiceAdoptionSummary) -> Vec<String> {
    let mut lines = vec![
        if summary.dry_run {
            format!("Would adopt global service {}", summary.env_name)
        } else {
            format!("Adopted global service {}", summary.env_name)
        },
        format!("  global label: {}", summary.global_label),
        format!("  global service file: {}", summary.global_plist_path),
        format!("  backup service file: {}", summary.backup_plist_path),
        format!("  managed label: {}", summary.managed_label),
        format!("  managed service file: {}", summary.managed_plist_path),
        format!("  port: {}", summary.gateway_port),
    ];
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
}

pub fn service_restored(summary: &ServiceRestoreSummary, profile: RenderProfile) -> Vec<String> {
    if !profile.pretty {
        return service_restored_raw(summary);
    }

    let mut lines = vec![paint(
        if summary.dry_run {
            "Global service restore planned"
        } else {
            "Global service restored"
        },
        Tone::Strong,
        profile.color,
    )];
    push_card(
        &mut lines,
        "Service",
        vec![
            KeyValueRow::accent("Env", summary.env_name.clone()),
            KeyValueRow::plain("Port", summary.gateway_port.to_string()),
            KeyValueRow::plain("OpenClaw label", summary.global_label.clone()),
            KeyValueRow::plain("OCM label", summary.managed_label.clone()),
        ],
        profile.color,
    );
    if !summary.warnings.is_empty() {
        let warnings = summary
            .warnings
            .iter()
            .enumerate()
            .map(|(index, warning)| KeyValueRow::warning(format!("#{}", index + 1), warning))
            .collect::<Vec<_>>();
        push_card(&mut lines, "Warnings", warnings, profile.color);
    }
    lines
}

fn service_restored_raw(summary: &ServiceRestoreSummary) -> Vec<String> {
    let mut lines = vec![
        if summary.dry_run {
            format!("Would restore global service {}", summary.env_name)
        } else {
            format!("Restored global service {}", summary.env_name)
        },
        format!("  global label: {}", summary.global_label),
        format!("  global service file: {}", summary.global_plist_path),
        format!("  backup service file: {}", summary.backup_plist_path),
        format!("  managed label: {}", summary.managed_label),
        format!("  managed service file: {}", summary.managed_plist_path),
        format!("  port: {}", summary.gateway_port),
    ];
    for warning in &summary.warnings {
        lines.push(format!("  warning: {warning}"));
    }
    lines
}

pub fn service_action(
    summary: &ServiceActionSummary,
    profile: RenderProfile,
    command_example: &str,
) -> Vec<String> {
    if !profile.pretty {
        return service_action_raw(summary, command_example);
    }

    let title = match summary.action.as_str() {
        "start" => "Service started",
        "stop" => "Service stopped",
        "restart" => "Service restarted",
        "uninstall" => "Service uninstalled",
        _ => "Service updated",
    };
    let mut lines = vec![paint(title, Tone::Strong, profile.color)];
    let mut service_rows = vec![
        KeyValueRow::accent("Env", summary.env_name.clone()),
        KeyValueRow::plain("Type", summary.service_kind.clone()),
    ];
    if let Some(port) = summary.gateway_port {
        service_rows.push(KeyValueRow::accent("Port", port.to_string()));
    }
    push_card(&mut lines, "Service", service_rows, profile.color);

    let next = match summary.action.as_str() {
        "uninstall" => vec![
            KeyValueRow::accent(
                "Install",
                format!("{command_example} service install {}", summary.env_name),
            ),
            KeyValueRow::accent("List", format!("{command_example} service list")),
        ],
        _ => vec![
            KeyValueRow::accent(
                "Status",
                format!("{command_example} service status {}", summary.env_name),
            ),
            KeyValueRow::accent(
                "Logs",
                format!(
                    "{command_example} service logs {} --tail 50",
                    summary.env_name
                ),
            ),
        ],
    };
    push_card(&mut lines, "Next", next, profile.color);
    if !summary.warnings.is_empty() {
        let warnings = summary
            .warnings
            .iter()
            .enumerate()
            .map(|(index, warning)| KeyValueRow::warning(format!("#{}", index + 1), warning))
            .collect::<Vec<_>>();
        push_card(&mut lines, "Warnings", warnings, profile.color);
    }
    lines
}

fn service_action_raw(summary: &ServiceActionSummary, command_example: &str) -> Vec<String> {
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
        format!("  service file: {}", summary.managed_plist_path),
    ];
    if let Some(port) = summary.gateway_port {
        lines.push(format!("  port: {port}"));
    }
    match summary.action.as_str() {
        "uninstall" => lines.push(format!(
            "  install: {command_example} service install {}",
            summary.env_name
        )),
        _ => lines.push(format!(
            "  status: {command_example} service status {}",
            summary.env_name
        )),
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

fn available_row(label: &str, value: bool) -> KeyValueRow {
    if value {
        KeyValueRow::accent(label, "yes")
    } else {
        KeyValueRow::muted(label, "no")
    }
}

fn action_row(label: &str, value: bool) -> KeyValueRow {
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
                global_env_name: Some("demo".to_string()),
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
                    global_env_name: Some("demo".to_string()),
                    binding_kind: Some("launcher".to_string()),
                    binding_name: Some("stable".to_string()),
                    command: Some("openclaw gateway run".to_string()),
                    binary_path: None,
                    args: Vec::new(),
                    run_dir: "/tmp/demo".to_string(),
                    gateway_port: 18789,
                    openclaw_state: "healthy".to_string(),
                    openclaw_detail: None,
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

        assert!(lines[0].starts_with('┌'));
        assert!(lines[1].contains("Env"));
        assert!(lines[1].contains("Service"));
        assert!(lines[1].contains("OpenClaw"));
        assert!(lines[3].contains("demo"));
        assert!(lines[4].starts_with('└'));
        assert_eq!(
            lines.last().unwrap(),
            "Separate OpenClaw service detected for env demo; use service discover for details."
        );
    }

    #[test]
    fn service_status_uses_relation_style_global_state() {
        let lines = service_status(&sample_service_summary(), RenderProfile::raw(), "ocm");

        assert!(lines.contains(&"globalState: loaded-other".to_string()));
        assert!(lines.contains(&"globalMatchesEnv: false".to_string()));
    }

    #[test]
    fn service_status_pretty_uses_cards() {
        let lines = service_status(
            &sample_service_summary(),
            RenderProfile::pretty(false),
            "ocm",
        );

        assert_eq!(lines[0], "Service demo");
        assert!(lines.iter().any(|line| line.contains("OpenClaw")));
        assert!(lines.iter().any(|line| line.contains("OCM service")));
        assert!(!lines.iter().any(|line| line.contains("OpenClaw service")));
    }

    #[test]
    fn service_status_pretty_shows_openclaw_service_when_it_belongs_to_the_env() {
        let mut summary = sample_service_summary();
        summary.global_env_name = Some("demo".to_string());
        summary.global_matches_env = true;

        let lines = service_status(&summary, RenderProfile::pretty(false), "ocm");

        assert!(lines.iter().any(|line| line.contains("OpenClaw service")));
        assert!(lines.iter().any(|line| line.contains("Env")));
    }

    #[test]
    fn service_status_pretty_suggests_install_when_service_is_absent() {
        let mut summary = sample_service_summary();
        summary.installed = false;
        summary.loaded = false;
        summary.running = false;
        summary.openclaw_state = "stopped".to_string();

        let lines = service_status(&summary, RenderProfile::pretty(false), "ocm");

        assert!(lines.iter().any(|line| line.contains("Next")));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("ocm service install demo"))
        );
    }

    #[test]
    fn service_status_pretty_suggests_repair_for_auth_required() {
        let mut summary = sample_service_summary();
        summary.running = true;
        summary.openclaw_state = "auth-required".to_string();
        summary.openclaw_detail = Some("gateway token mismatch".to_string());

        let lines = service_status(&summary, RenderProfile::pretty(false), "ocm");

        assert!(
            lines
                .iter()
                .any(|line| line.contains("ocm @demo -- onboard"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("ocm service logs demo --tail 50"))
        );
    }

    #[test]
    fn service_list_pretty_compacts_on_narrow_terminals() {
        let lines = service_list_with_width(
            &ServiceSummaryList {
                global_label: "ai.openclaw.gateway".to_string(),
                global_env_name: Some("demo".to_string()),
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

        assert!(lines[0].starts_with('┌'));
        assert!(lines[1].contains("Binding"));
        assert!(lines[1].contains("Service"));
        assert!(lines[1].contains("OpenClaw"));
        assert!(!lines[1].contains("Notes"));
        assert_eq!(
            lines[6],
            "Use service status <env> or --raw for notes and readiness details."
        );
        assert_eq!(
            lines.last().unwrap(),
            "Separate OpenClaw service detected for env demo; use service discover for details."
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
        assert!(!lines
            .iter()
            .any(|line| line.contains("Use --raw or --json for full command details.")));
    }

    fn sample_service_summary() -> ServiceSummary {
        ServiceSummary {
            env_name: "demo".to_string(),
            service_kind: "gateway".to_string(),
            managed_label: "ai.openclaw.gateway.ocm.demo".to_string(),
            managed_plist_path: "/tmp/demo.plist".to_string(),
            global_label: "ai.openclaw.gateway".to_string(),
            global_env_name: Some("other".to_string()),
            binding_kind: Some("launcher".to_string()),
            binding_name: Some("stable".to_string()),
            command: Some("openclaw gateway run".to_string()),
            binary_path: None,
            args: Vec::new(),
            run_dir: "/tmp/demo".to_string(),
            gateway_port: 18789,
            openclaw_state: "stopped".to_string(),
            openclaw_detail: None,
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
                openclaw_state: "stopped".to_string(),
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
