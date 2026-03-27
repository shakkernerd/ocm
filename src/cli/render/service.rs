use crate::service::{
    ServiceActionSummary, ServiceAdoptionSummary, ServiceInstallSummary, ServiceSummary,
    ServiceSummaryList, ServiceRestoreSummary,
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

pub fn service_list(summary: &ServiceSummaryList) -> Vec<String> {
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
        lines.push(bits.join("  "));
    }

    lines
}

pub fn service_status(summary: &ServiceSummary) -> Vec<String> {
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
            daemon_state(
                summary.global_installed,
                summary.global_loaded,
                summary.global_running
            )
        ),
        format!("globalMatchesEnv: {}", summary.global_matches_env),
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
    if let Some(issue) = summary.issue.as_deref() {
        lines.push(format!("issue: {issue}"));
    }

    lines
}

pub fn service_installed(summary: &ServiceInstallSummary) -> Vec<String> {
    let mut lines = vec![
        format!("Installed service {}", summary.env_name),
        format!("  label: {}", summary.managed_label),
        format!("  plist: {}", summary.managed_plist_path),
        format!("  port: {}", summary.gateway_port),
        format!("  binding: {}:{}", summary.binding_kind, summary.binding_name),
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
