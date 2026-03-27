use super::RenderProfile;
use crate::infra::terminal::{Cell, Tone, paint, render_table};
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
        "running" | "match" => Tone::Success,
        "loaded" | "installed" | "loaded-other" | "installed-other" | "running-other" => {
            Tone::Warning
        }
        "absent" => Tone::Muted,
        _ => Tone::Plain,
    }
}

pub fn service_list(summary: &ServiceSummaryList, profile: RenderProfile) -> Vec<String> {
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
        paint("Global service", Tone::Strong, profile.color),
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
            vec![
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
                    global_state,
                    crate::infra::terminal::Align::Left,
                    state_tone(global_state),
                ),
                if notes.is_empty() {
                    Cell::muted("—")
                } else if service.issue.is_some() {
                    Cell::danger(notes.join(","))
                } else {
                    Cell::warning(notes.join(","))
                },
            ]
        })
        .collect::<Vec<_>>();
    lines.extend(render_table(
        &["Env", "Binding", "Port", "Managed", "Global", "Notes"],
        &rows,
        profile.color,
    ));
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

pub fn service_status(summary: &ServiceSummary) -> Vec<String> {
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
        format!(
            "globalState: {}",
            global_state
        ),
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
    if summary.services.is_empty() {
        return vec!["No OpenClaw services discovered.".to_string()];
    }
    if !profile.pretty {
        return service_discover_raw(summary);
    }

    let rows = summary
        .services
        .iter()
        .map(|service| {
            let state = daemon_state(service.installed, service.loaded, service.running);
            let adopt = if service.adoptable {
                service
                    .matched_env_name
                    .as_deref()
                    .map(|env_name| format!("ready:{env_name}"))
                    .unwrap_or_else(|| "ready".to_string())
            } else {
                "—".to_string()
            };
            let command = if !service.program_arguments.is_empty() {
                service.program_arguments.join(" ")
            } else {
                service.program.clone().unwrap_or_else(|| "—".to_string())
            };
            vec![
                Cell::accent(service.label.clone()),
                Cell::plain(service.source_kind.clone()),
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
                    Cell::warning(adopt)
                },
                if command == "—" {
                    Cell::muted(command)
                } else {
                    Cell::plain(command)
                },
            ]
        })
        .collect::<Vec<_>>();
    render_table(
        &[
            "Label", "Source", "State", "Port", "Env", "Adopt", "Command",
        ],
        &rows,
        profile.color,
    )
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

#[cfg(test)]
mod tests {
    use super::{RenderProfile, service_list, service_status};
    use crate::service::{ServiceSummary, ServiceSummaryList};

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

        assert!(lines[0].contains("Global service"));
        assert!(lines[2].starts_with('┌'));
        assert!(lines[3].contains("Env"));
        assert!(lines[5].contains("demo"));
        assert!(lines[6].starts_with('└'));
    }

    #[test]
    fn service_status_uses_relation_style_global_state() {
        let lines = service_status(&ServiceSummary {
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
        });

        assert!(lines.contains(&"globalState: loaded-other".to_string()));
        assert!(lines.contains(&"globalMatchesEnv: false".to_string()));
    }
}
