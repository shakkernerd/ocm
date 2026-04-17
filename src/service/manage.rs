use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

use super::inspect::{
    GLOBAL_GATEWAY_LABEL, global_plist_path, managed_plist_path, managed_service_label,
    service_status,
};
use super::platform::{
    ManagedServiceDefinition, ServiceManagerKind, activate_managed_service,
    service_backend_support_error, service_manager_kind, stop_managed_service,
    uninstall_managed_service, write_managed_service_definition,
};
use crate::env::resolve_gateway_process_spec;
use crate::env::{EnvMeta, EnvironmentService};
use crate::infra::shell::build_openclaw_env;
use crate::store::{
    derive_env_paths, display_path, list_environments, now_utc, resolve_ocm_home,
    rewrite_openclaw_gateway_port_for_target, save_environment,
};

const DEFAULT_SERVICE_PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin";
const LAUNCH_AGENT_DIR_MODE: u32 = 0o755;
const LAUNCH_AGENT_PLIST_MODE: u32 = 0o644;
const SERVICE_PROXY_ENV_KEYS: [&str; 8] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "http_proxy",
    "https_proxy",
    "no_proxy",
    "all_proxy",
];
const SERVICE_EXTRA_ENV_KEYS: [&str; 2] = ["NODE_EXTRA_CA_CERTS", "NODE_USE_SYSTEM_CA"];

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceInstallSummary {
    pub env_name: String,
    pub service_kind: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub gateway_port: u32,
    pub binding_kind: String,
    pub binding_name: String,
    pub command: Option<String>,
    pub binary_path: Option<String>,
    pub args: Vec<String>,
    pub run_dir: String,
    pub log_dir: String,
    pub stdout_path: String,
    pub stderr_path: String,
    pub persisted_gateway_port: bool,
    pub previous_gateway_port: Option<u32>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceActionSummary {
    pub env_name: String,
    pub service_kind: String,
    pub action: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub installed: bool,
    pub gateway_port: Option<u32>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLogSummary {
    pub env_name: String,
    pub service_kind: String,
    pub stream: String,
    pub path: String,
    pub tail_lines: Option<usize>,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAdoptionSummary {
    pub env_name: String,
    pub service_kind: String,
    pub global_label: String,
    pub global_plist_path: String,
    pub backup_plist_path: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub gateway_port: u32,
    pub dry_run: bool,
    pub adopted: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRestoreSummary {
    pub env_name: String,
    pub service_kind: String,
    pub global_label: String,
    pub global_plist_path: String,
    pub backup_plist_path: String,
    pub managed_label: String,
    pub managed_plist_path: String,
    pub gateway_port: u32,
    pub dry_run: bool,
    pub restored: bool,
    pub warnings: Vec<String>,
}

struct PreparedService {
    env_meta: EnvMeta,
    binding_kind: String,
    binding_name: String,
    command: Option<String>,
    binary_path: Option<String>,
    args: Vec<String>,
    program_arguments: Vec<String>,
    run_dir: PathBuf,
    managed_label: String,
    managed_plist_path: PathBuf,
    log_dir: PathBuf,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
    warnings: Vec<String>,
    persisted_gateway_port: bool,
    previous_gateway_port: Option<u32>,
}

struct PersistedGatewayPort {
    env_meta: EnvMeta,
    persisted_gateway_port: bool,
    previous_gateway_port: Option<u32>,
}

struct GlobalServiceBinding {
    plist_path: PathBuf,
    config_path: Option<String>,
    gateway_port: Option<u32>,
}

struct PreparedGlobalAdoption {
    prepared: PreparedService,
    global: GlobalServiceBinding,
    backup_plist_path: PathBuf,
}

struct PreparedGlobalRestore {
    env_meta: EnvMeta,
    global_plist_path: PathBuf,
    backup_plist_path: PathBuf,
    managed_label: String,
    managed_plist_path: PathBuf,
    gateway_port: u32,
    warnings: Vec<String>,
}

struct ExistingServiceRef {
    env_meta: EnvMeta,
    managed_label: String,
    managed_plist_path: PathBuf,
}

pub fn install_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceInstallSummary, String> {
    ensure_service_backend_ready(env)?;
    let prepared = prepare_service(name, env, cwd)?;
    write_service_definition(&prepared, env)?;
    activate_managed_service(&prepared.managed_label, &prepared.managed_plist_path, env)?;

    Ok(ServiceInstallSummary {
        env_name: prepared.env_meta.name.clone(),
        service_kind: "gateway".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        gateway_port: prepared.env_meta.gateway_port.unwrap_or_default(),
        binding_kind: prepared.binding_kind,
        binding_name: prepared.binding_name,
        command: prepared.command,
        binary_path: prepared.binary_path,
        args: prepared.args,
        run_dir: display_path(&prepared.run_dir),
        log_dir: display_path(&prepared.log_dir),
        stdout_path: display_path(&prepared.stdout_path),
        stderr_path: display_path(&prepared.stderr_path),
        persisted_gateway_port: prepared.persisted_gateway_port,
        previous_gateway_port: prepared.previous_gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn adopt_global_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    dry_run: bool,
) -> Result<ServiceAdoptionSummary, String> {
    ensure_launchd_only("service adopt-global", env)?;
    ensure_service_backend_ready(env)?;
    let adoption = prepare_global_adoption(name, env, cwd, !dry_run)?;
    if !dry_run {
        write_service_definition(&adoption.prepared, env)?;
        backup_global_plist(&adoption.global.plist_path, &adoption.backup_plist_path)?;
        stop_managed_service(GLOBAL_GATEWAY_LABEL, env)?;
        if let Err(error) = activate_managed_service(
            &adoption.prepared.managed_label,
            &adoption.prepared.managed_plist_path,
            env,
        ) {
            rollback_failed_global_adoption(&adoption, env)?;
            return Err(error);
        }
        if let Err(error) = fs::remove_file(&adoption.global.plist_path) {
            rollback_failed_global_adoption(&adoption, env)?;
            return Err(error.to_string());
        }
    }

    Ok(ServiceAdoptionSummary {
        env_name: adoption.prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_plist_path: display_path(&adoption.global.plist_path),
        backup_plist_path: display_path(&adoption.backup_plist_path),
        managed_label: adoption.prepared.managed_label,
        managed_plist_path: display_path(&adoption.prepared.managed_plist_path),
        gateway_port: adoption.prepared.env_meta.gateway_port.unwrap_or_default(),
        dry_run,
        adopted: !dry_run,
        warnings: adoption.prepared.warnings,
    })
}

pub fn restore_global_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    dry_run: bool,
) -> Result<ServiceRestoreSummary, String> {
    ensure_launchd_only("service restore-global", env)?;
    ensure_service_backend_ready(env)?;
    let restore = prepare_global_restore(name, env, cwd)?;
    if !dry_run {
        restore_global_plist(&restore.backup_plist_path, &restore.global_plist_path)?;
        stop_managed_service(&restore.managed_label, env)?;
        if let Err(error) =
            activate_managed_service(GLOBAL_GATEWAY_LABEL, &restore.global_plist_path, env)
        {
            rollback_failed_global_restore(&restore, env)?;
            return Err(error);
        }
        if let Err(error) =
            persist_restored_gateway_port(&restore.env_meta, restore.gateway_port, env, cwd)
        {
            rollback_failed_global_restore(&restore, env)?;
            return Err(error);
        }
        if restore.managed_plist_path.exists() {
            if let Err(error) = fs::remove_file(&restore.managed_plist_path) {
                rollback_failed_global_restore(&restore, env)?;
                return Err(error.to_string());
            }
        }
    }

    Ok(ServiceRestoreSummary {
        env_name: restore.env_meta.name,
        service_kind: "gateway".to_string(),
        global_label: GLOBAL_GATEWAY_LABEL.to_string(),
        global_plist_path: display_path(&restore.global_plist_path),
        backup_plist_path: display_path(&restore.backup_plist_path),
        managed_label: restore.managed_label,
        managed_plist_path: display_path(&restore.managed_plist_path),
        gateway_port: restore.gateway_port,
        dry_run,
        restored: !dry_run,
        warnings: restore.warnings,
    })
}

fn persist_restored_gateway_port(
    env_meta: &EnvMeta,
    gateway_port: u32,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let mut updated = env_meta.clone();
    updated.gateway_port = Some(gateway_port);
    let saved = save_environment(updated, env, cwd)?;
    let paths = derive_env_paths(Path::new(&saved.root));
    let _ = rewrite_openclaw_gateway_port_for_target(&paths, gateway_port)?;
    Ok(saved)
}

pub fn start_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    ensure_service_backend_ready(env)?;
    let prepared = prepare_existing_service(name, env, cwd)?;
    refresh_managed_service(&prepared, env)?;

    Ok(ServiceActionSummary {
        env_name: prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "start".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        installed: prepared.managed_plist_path.exists(),
        gateway_port: prepared.env_meta.gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn stop_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let existing = prepare_existing_service_ref(name, env, cwd)?;
    stop_managed_service(&existing.managed_label, env)?;

    Ok(ServiceActionSummary {
        env_name: existing.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "stop".to_string(),
        managed_label: existing.managed_label,
        managed_plist_path: display_path(&existing.managed_plist_path),
        installed: existing.managed_plist_path.exists(),
        gateway_port: existing.env_meta.gateway_port,
        warnings: Vec::new(),
    })
}

pub fn restart_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    ensure_service_backend_ready(env)?;
    let prepared = prepare_existing_service(name, env, cwd)?;
    refresh_managed_service(&prepared, env)?;

    Ok(ServiceActionSummary {
        env_name: prepared.env_meta.name,
        service_kind: "gateway".to_string(),
        action: "restart".to_string(),
        managed_label: prepared.managed_label,
        managed_plist_path: display_path(&prepared.managed_plist_path),
        installed: prepared.managed_plist_path.exists(),
        gateway_port: prepared.env_meta.gateway_port,
        warnings: prepared.warnings,
    })
}

pub fn uninstall_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceActionSummary, String> {
    let service = EnvironmentService::new(env, cwd);
    let env_meta = service.apply_effective_gateway_port(service.get(name)?)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;

    uninstall_managed_service(&managed_label, env)?;
    let plist_path = display_path(&managed_plist_path);
    if managed_plist_path.exists() {
        fs::remove_file(&managed_plist_path).map_err(|error| error.to_string())?;
    }

    Ok(ServiceActionSummary {
        env_name: env_meta.name,
        service_kind: "gateway".to_string(),
        action: "uninstall".to_string(),
        managed_label,
        managed_plist_path: plist_path,
        installed: false,
        gateway_port: env_meta.gateway_port,
        warnings: Vec::new(),
    })
}

pub fn service_logs(
    name: &str,
    stream: &str,
    tail_lines: Option<usize>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ServiceLogSummary, String> {
    let env_meta = EnvironmentService::new(env, cwd).get(name)?;
    let stream = normalize_log_stream(stream)?;
    if service_manager_kind(env) == ServiceManagerKind::SystemdUser {
        let label = managed_service_label(&env_meta.name, env, cwd)?;
        let path = format!("journalctl --user --unit {label}");
        let content = read_systemd_service_logs(&label, tail_lines, env)?;
        return Ok(ServiceLogSummary {
            env_name: env_meta.name,
            service_kind: "gateway".to_string(),
            stream: stream.to_string(),
            path,
            tail_lines,
            content,
        });
    }

    let log_path = match stream {
        "stdout" => service_stdout_log_path(&env_meta),
        "stderr" => service_stderr_log_path(&env_meta),
        _ => unreachable!("normalize_log_stream validates service log stream"),
    };

    if !log_path.exists() {
        return Err(format!(
            "{} log does not exist for env \"{}\": {}",
            stream,
            env_meta.name,
            display_path(&log_path)
        ));
    }

    let raw = fs::read_to_string(&log_path).map_err(|error| error.to_string())?;
    let content = if let Some(tail_lines) = tail_lines {
        tail_text(&raw, tail_lines)
    } else {
        raw
    };

    Ok(ServiceLogSummary {
        env_name: env_meta.name,
        service_kind: "gateway".to_string(),
        stream: stream.to_string(),
        path: display_path(&log_path),
        tail_lines,
        content,
    })
}

fn prepare_existing_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PreparedService, String> {
    let existing = prepare_existing_service_ref(name, env, cwd)?;
    if existing.managed_plist_path.exists() {
        return build_prepared_service(existing.env_meta, env, cwd, false, None);
    }

    prepare_service(name, env, cwd)
}

fn prepare_existing_service_ref(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ExistingServiceRef, String> {
    let service = EnvironmentService::new(env, cwd);
    let env_meta = service.apply_effective_gateway_port(service.get(name)?)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;
    Ok(ExistingServiceRef {
        env_meta,
        managed_label,
        managed_plist_path,
    })
}

fn prepare_service(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PreparedService, String> {
    prepare_service_with_allowed_busy_port(name, env, cwd, None, true)
}

fn prepare_service_with_allowed_busy_port(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    allowed_busy_port: Option<u32>,
    persist_gateway_port: bool,
) -> Result<PreparedService, String> {
    let port_assignment =
        persist_service_gateway_port(name, env, cwd, allowed_busy_port, persist_gateway_port)?;
    build_prepared_service(
        port_assignment.env_meta,
        env,
        cwd,
        port_assignment.persisted_gateway_port,
        port_assignment.previous_gateway_port,
    )
}

fn build_prepared_service(
    env_meta: EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    persisted_gateway_port: bool,
    previous_gateway_port: Option<u32>,
) -> Result<PreparedService, String> {
    let process_spec = resolve_gateway_process_spec(&env_meta, env, cwd, true)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;
    let log_dir = service_log_dir(&env_meta);
    let stdout_path = service_stdout_log_path(&env_meta);
    let stderr_path = service_stderr_log_path(&env_meta);
    let program_arguments = process_spec.program_arguments();

    let warnings = service_install_warnings(
        &env_meta.name,
        previous_gateway_port,
        env_meta.gateway_port.unwrap_or_default(),
        persisted_gateway_port,
    );

    Ok(PreparedService {
        env_meta,
        binding_kind: process_spec.binding_kind,
        binding_name: process_spec.binding_name,
        command: process_spec.command,
        binary_path: process_spec.binary_path,
        args: process_spec.args,
        program_arguments,
        run_dir: process_spec.run_dir,
        managed_label,
        managed_plist_path,
        log_dir,
        stdout_path,
        stderr_path,
        warnings,
        persisted_gateway_port,
        previous_gateway_port,
    })
}

fn service_install_warnings(
    name: &str,
    previous_gateway_port: Option<u32>,
    gateway_port: u32,
    persisted_gateway_port: bool,
) -> Vec<String> {
    if !persisted_gateway_port {
        return Vec::new();
    }

    match previous_gateway_port {
        Some(previous) => vec![format!(
            "gateway port {previous} was unavailable; assigned {gateway_port} to env \"{name}\" and saved it to env metadata"
        )],
        None => vec![format!(
            "assigned gateway port {gateway_port} to env \"{name}\" and saved it to env metadata for service stability"
        )],
    }
}

fn persist_service_gateway_port(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    allowed_busy_port: Option<u32>,
    persist_gateway_port: bool,
) -> Result<PersistedGatewayPort, String> {
    let service = EnvironmentService::new(env, cwd);
    let original = service.get(name)?;
    let effective = service.apply_effective_gateway_port(original.clone())?;
    let current_summary = service_status(name, env, cwd)?;
    let reserved_ports = collect_reserved_ports(name, env, cwd)?;
    let preferred_port = effective.gateway_port.unwrap_or_default();
    let chosen_port = choose_gateway_port(
        preferred_port,
        &reserved_ports,
        &current_summary,
        allowed_busy_port,
    );
    let persisted_gateway_port = original.gateway_port != Some(chosen_port);
    let previous_gateway_port = if chosen_port != preferred_port {
        Some(preferred_port)
    } else {
        None
    };

    if original.gateway_port == Some(chosen_port) {
        let mut env_meta = original;
        env_meta.gateway_port = Some(chosen_port);
        return Ok(PersistedGatewayPort {
            env_meta,
            persisted_gateway_port,
            previous_gateway_port,
        });
    }

    let mut updated = original;
    updated.gateway_port = Some(chosen_port);
    let env_meta = if persist_gateway_port {
        let env_meta = save_environment(updated, env, cwd)?;
        let paths = derive_env_paths(Path::new(&env_meta.root));
        let _ = rewrite_openclaw_gateway_port_for_target(&paths, chosen_port)?;
        env_meta
    } else {
        updated
    };
    Ok(PersistedGatewayPort {
        env_meta,
        persisted_gateway_port,
        previous_gateway_port,
    })
}

fn collect_reserved_ports(
    target_name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<BTreeSet<u32>, String> {
    let service = EnvironmentService::new(env, cwd);
    let mut ports = BTreeSet::new();
    for meta in list_environments(env, cwd)? {
        if meta.name == target_name {
            continue;
        }
        let port = service.apply_effective_gateway_port(meta)?.gateway_port;
        if let Some(port) = port {
            ports.insert(port);
        }
    }
    Ok(ports)
}

fn choose_gateway_port(
    preferred_port: u32,
    reserved_ports: &BTreeSet<u32>,
    current_summary: &super::ServiceSummary,
    allowed_busy_port: Option<u32>,
) -> u32 {
    let mut port = preferred_port.max(18_789);
    loop {
        let managed_self_running = current_summary.loaded || current_summary.running;
        let allowed_busy = allowed_busy_port == Some(port);
        let available = !reserved_ports.contains(&port)
            && (port_is_available(port)
                || allowed_busy
                || managed_self_running && current_summary.gateway_port == port);
        if available {
            return port;
        }
        port = port.saturating_add(1);
    }
}

fn port_is_available(port: u32) -> bool {
    TcpListener::bind(("127.0.0.1", port as u16)).is_ok()
}

fn normalize_log_stream(stream: &str) -> Result<&str, String> {
    match stream.trim().to_ascii_lowercase().as_str() {
        "stdout" => Ok("stdout"),
        "stderr" => Ok("stderr"),
        _ => Err(format!("unsupported service log stream: {stream}")),
    }
}

fn tail_text(raw: &str, tail_lines: usize) -> String {
    if tail_lines == 0 {
        return String::new();
    }

    let trailing_newline = raw.ends_with('\n');
    let lines = raw.lines().collect::<Vec<_>>();
    if lines.len() <= tail_lines {
        return raw.to_string();
    }

    let mut content = lines[lines.len() - tail_lines..].join("\n");
    if trailing_newline {
        content.push('\n');
    }
    content
}

fn service_log_dir(env_meta: &EnvMeta) -> PathBuf {
    derive_env_paths(Path::new(&env_meta.root))
        .state_dir
        .join("logs")
}

fn service_stdout_log_path(env_meta: &EnvMeta) -> PathBuf {
    service_log_dir(env_meta).join("gateway.log")
}

fn service_stderr_log_path(env_meta: &EnvMeta) -> PathBuf {
    service_log_dir(env_meta).join("gateway.err.log")
}

fn write_service_definition(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let definition = ManagedServiceDefinition {
        label: prepared.managed_label.clone(),
        description: format!(
            "OCM-managed OpenClaw gateway service for env {}",
            prepared.env_meta.name
        ),
        definition_path: prepared.managed_plist_path.clone(),
        program_arguments: prepared.program_arguments.clone(),
        working_directory: prepared.run_dir.clone(),
        stdout_path: prepared.stdout_path.clone(),
        stderr_path: prepared.stderr_path.clone(),
        environment: build_service_environment(&prepared.env_meta, &prepared.managed_label, env),
    };
    write_managed_service_definition(&definition, env)
}

fn prepare_global_adoption(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
    persist_gateway_port: bool,
) -> Result<PreparedGlobalAdoption, String> {
    let target_env = EnvironmentService::new(env, cwd).get(name)?;
    let global = read_global_service_binding(env)?;
    let target_config_path =
        display_path(&derive_env_paths(Path::new(&target_env.root)).config_path);
    let global_config_path = global.config_path.clone().ok_or_else(|| {
        "global OpenClaw service does not expose OPENCLAW_CONFIG_PATH for adoption".to_string()
    })?;
    if global_config_path != target_config_path {
        return Err(format!(
            "global OpenClaw service points at a different env; expected {} but found {}",
            target_config_path, global_config_path
        ));
    }

    Ok(PreparedGlobalAdoption {
        prepared: prepare_service_with_allowed_busy_port(
            name,
            env,
            cwd,
            global.gateway_port,
            persist_gateway_port,
        )?,
        backup_plist_path: global_plist_backup_path(env, cwd)?,
        global,
    })
}

fn prepare_global_restore(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PreparedGlobalRestore, String> {
    let service = EnvironmentService::new(env, cwd);
    let env_meta = service.apply_effective_gateway_port(service.get(name)?)?;
    let global_plist_path = global_plist_path(env);
    if global_plist_path.exists() {
        return Err(
            "global OpenClaw service is already installed; remove it before restoring from backup"
                .to_string(),
        );
    }

    let backup = latest_matching_global_plist_backup(&env_meta, env, cwd)?;
    let managed_label = managed_service_label(&env_meta.name, env, cwd)?;
    let managed_plist_path = managed_plist_path(&env_meta.name, env, cwd)?;
    let mut warnings = Vec::new();
    if !managed_plist_path.exists() {
        warnings.push(format!(
            "managed service plist is absent for env \"{}\"; restoring the global plist anyway",
            env_meta.name
        ));
    }

    Ok(PreparedGlobalRestore {
        gateway_port: backup
            .gateway_port
            .or(env_meta.gateway_port)
            .unwrap_or_default(),
        env_meta,
        global_plist_path,
        backup_plist_path: backup.plist_path,
        managed_label,
        managed_plist_path,
        warnings,
    })
}

fn build_service_environment(
    env_meta: &EnvMeta,
    launchd_label: &str,
    process_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut base_env = BTreeMap::new();
    if let Some(home) = process_env
        .get("HOME")
        .filter(|value| !value.trim().is_empty())
    {
        base_env.insert("HOME".to_string(), home.trim().to_string());
    }
    if let Some(path) = process_env
        .get("PATH")
        .filter(|value| !value.trim().is_empty())
    {
        base_env.insert("PATH".to_string(), path.trim().to_string());
    } else {
        base_env.insert("PATH".to_string(), DEFAULT_SERVICE_PATH.to_string());
    }
    if let Some(tmpdir) = process_env
        .get("TMPDIR")
        .filter(|value| !value.trim().is_empty())
    {
        base_env.insert("TMPDIR".to_string(), tmpdir.trim().to_string());
    }
    for key in SERVICE_PROXY_ENV_KEYS {
        if let Some(value) = process_env
            .get(key)
            .filter(|value| !value.trim().is_empty())
        {
            base_env.insert(key.to_string(), value.trim().to_string());
        }
    }
    for key in SERVICE_EXTRA_ENV_KEYS {
        if let Some(value) = process_env
            .get(key)
            .filter(|value| !value.trim().is_empty())
        {
            base_env.insert(key.to_string(), value.trim().to_string());
        }
    }

    let mut service_env = build_openclaw_env(env_meta, &base_env);
    service_env.insert("OPENCLAW_PROFILE".to_string(), String::new());
    service_env.insert(
        "OPENCLAW_SERVICE_LABEL".to_string(),
        launchd_label.to_string(),
    );
    match service_manager_kind(process_env) {
        ServiceManagerKind::Launchd => {
            service_env.insert(
                "OPENCLAW_LAUNCHD_LABEL".to_string(),
                launchd_label.to_string(),
            );
        }
        ServiceManagerKind::SystemdUser => {
            service_env.insert(
                "OPENCLAW_SYSTEMD_UNIT".to_string(),
                launchd_label.to_string(),
            );
        }
        ServiceManagerKind::Unsupported => {}
    }
    service_env
}

fn ensure_secure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())?;
    set_mode(path, LAUNCH_AGENT_DIR_MODE)
}

fn set_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
        Ok(())
    }
}

fn backup_global_plist(source_path: &Path, backup_path: &Path) -> Result<(), String> {
    let Some(parent) = backup_path.parent() else {
        return Err("failed to resolve backup directory for global service plist".to_string());
    };
    ensure_secure_dir(parent)?;
    fs::copy(source_path, backup_path).map_err(|error| error.to_string())?;
    set_mode(backup_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn rollback_failed_global_adoption(
    adoption: &PreparedGlobalAdoption,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let _ = stop_managed_service(&adoption.prepared.managed_label, env);
    if adoption.prepared.managed_plist_path.exists() {
        fs::remove_file(&adoption.prepared.managed_plist_path).map_err(|error| {
            format!(
                "failed to roll back managed plist {}: {error}",
                display_path(&adoption.prepared.managed_plist_path)
            )
        })?;
    }
    activate_managed_service(GLOBAL_GATEWAY_LABEL, &adoption.global.plist_path, env)
}

fn rollback_failed_global_restore(
    restore: &PreparedGlobalRestore,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let _ = stop_managed_service(GLOBAL_GATEWAY_LABEL, env);
    if restore.global_plist_path.exists() {
        fs::remove_file(&restore.global_plist_path).map_err(|error| {
            format!(
                "failed to remove restored global plist {}: {error}",
                display_path(&restore.global_plist_path)
            )
        })?;
    }
    if restore.managed_plist_path.exists() {
        activate_managed_service(&restore.managed_label, &restore.managed_plist_path, env)
    } else {
        Err(format!(
            "failed to roll back restore because managed service definition is absent: {}",
            display_path(&restore.managed_plist_path)
        ))
    }
}

fn restore_global_plist(backup_path: &Path, global_path: &Path) -> Result<(), String> {
    let Some(parent) = global_path.parent() else {
        return Err(
            "failed to resolve LaunchAgents directory for global service restore".to_string(),
        );
    };
    ensure_secure_dir(parent)?;
    fs::copy(backup_path, global_path).map_err(|error| error.to_string())?;
    set_mode(global_path, LAUNCH_AGENT_PLIST_MODE)?;
    Ok(())
}

fn read_global_service_binding(
    env: &BTreeMap<String, String>,
) -> Result<GlobalServiceBinding, String> {
    let plist_path = global_plist_path(env);
    if !plist_path.exists() {
        return Err("global OpenClaw service is not installed".to_string());
    }

    read_service_binding(&plist_path)
}

fn global_plist_backup_path(env: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf, String> {
    let backup_root = global_plist_backup_dir(env, cwd)?;
    let timestamp = now_utc().unix_timestamp_nanos();
    Ok(backup_root.join(format!("{GLOBAL_GATEWAY_LABEL}.{timestamp}.plist")))
}

fn global_plist_backup_dir(env: &BTreeMap<String, String>, cwd: &Path) -> Result<PathBuf, String> {
    Ok(resolve_ocm_home(env, cwd)?.join("services").join("backups"))
}

fn latest_matching_global_plist_backup(
    env_meta: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<GlobalServiceBinding, String> {
    let backup_dir = global_plist_backup_dir(env, cwd)?;
    if !backup_dir.exists() {
        return Err(format!(
            "no global service backup exists for env \"{}\"",
            env_meta.name
        ));
    }

    let target_config_path = display_path(&derive_env_paths(Path::new(&env_meta.root)).config_path);
    let mut bindings = Vec::new();
    for entry in fs::read_dir(&backup_dir).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !file_name.starts_with(&format!("{GLOBAL_GATEWAY_LABEL}."))
            || !file_name.ends_with(".plist")
        {
            continue;
        }
        let binding = read_service_binding(&path)?;
        if binding.config_path.as_deref() == Some(target_config_path.as_str()) {
            bindings.push(binding);
        }
    }

    bindings.sort_by(|left, right| {
        left.plist_path
            .file_name()
            .cmp(&right.plist_path.file_name())
    });
    bindings
        .pop()
        .ok_or_else(|| format!("no global service backup matches env \"{}\"", env_meta.name))
}

fn read_service_binding(plist_path: &Path) -> Result<GlobalServiceBinding, String> {
    Ok(GlobalServiceBinding {
        config_path: read_launch_agent_environment_value(plist_path, "OPENCLAW_CONFIG_PATH")?,
        gateway_port: read_launch_agent_environment_value(plist_path, "OPENCLAW_GATEWAY_PORT")?
            .and_then(|value| value.parse::<u32>().ok()),
        plist_path: plist_path.to_path_buf(),
    })
}

fn refresh_managed_service(
    prepared: &PreparedService,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    write_service_definition(prepared, env)?;
    activate_managed_service(&prepared.managed_label, &prepared.managed_plist_path, env)
}

fn read_launch_agent_environment_value(
    plist_path: &Path,
    key: &str,
) -> Result<Option<String>, String> {
    let raw = fs::read_to_string(plist_path).map_err(|error| error.to_string())?;
    let Some(env_section_start) = raw.find("<key>EnvironmentVariables</key>") else {
        return Ok(None);
    };
    let env_section = &raw[env_section_start..];
    let Some(dict_start_offset) = env_section.find("<dict>") else {
        return Ok(None);
    };
    let env_section = &env_section[dict_start_offset + "<dict>".len()..];
    let Some(dict_end_offset) = env_section.find("</dict>") else {
        return Ok(None);
    };
    let env_section = &env_section[..dict_end_offset];
    let key_marker = format!("<key>{key}</key>");
    let Some(key_offset) = env_section.find(&key_marker) else {
        return Ok(None);
    };
    let entry = &env_section[key_offset + key_marker.len()..];
    let Some(string_start_offset) = entry.find("<string>") else {
        return Ok(None);
    };
    let entry = &entry[string_start_offset + "<string>".len()..];
    let Some(string_end_offset) = entry.find("</string>") else {
        return Ok(None);
    };
    Ok(Some(plist_unescape(&entry[..string_end_offset])))
}

fn plist_unescape(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn journalctl_binary(env: &BTreeMap<String, String>) -> String {
    env.get("OCM_INTERNAL_JOURNALCTL_BIN")
        .cloned()
        .unwrap_or_else(|| "journalctl".to_string())
}

fn read_systemd_service_logs(
    label: &str,
    tail_lines: Option<usize>,
    env: &BTreeMap<String, String>,
) -> Result<String, String> {
    let mut command = Command::new(journalctl_binary(env));
    command.args(["--user", "--unit", label, "--no-pager", "-o", "cat"]);
    if let Some(tail_lines) = tail_lines {
        command.args(["-n", &tail_lines.to_string()]);
    }
    let output = command
        .output()
        .map_err(|error| format!("failed to run \"journalctl\": {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "journalctl --user failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn ensure_launchd_only(action: &str, env: &BTreeMap<String, String>) -> Result<(), String> {
    if service_manager_kind(env) == ServiceManagerKind::Launchd {
        return Ok(());
    }
    Err(format!(
        "{action} is currently only supported for launchd-managed machine-wide services"
    ))
}

fn ensure_service_backend_ready(env: &BTreeMap<String, String>) -> Result<(), String> {
    if let Some(error) = service_backend_support_error(env) {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        plist_unescape, read_launch_agent_environment_value, service_install_warnings, tail_text,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn install_warnings_explain_port_persistence_and_reassignment() {
        assert_eq!(
            service_install_warnings("test", None, 18790, true),
            vec![
                "assigned gateway port 18790 to env \"test\" and saved it to env metadata for service stability"
            ]
        );
        assert_eq!(
            service_install_warnings("test", Some(18789), 18790, true),
            vec![
                "gateway port 18789 was unavailable; assigned 18790 to env \"test\" and saved it to env metadata"
            ]
        );
    }

    #[test]
    fn tail_text_keeps_the_requested_number_of_lines() {
        assert_eq!(tail_text("a\nb\nc\n", 2), "b\nc\n");
        assert_eq!(tail_text("a\nb\nc", 1), "c");
        assert_eq!(tail_text("a\nb\n", 5), "a\nb\n");
        assert_eq!(tail_text("a\nb\n", 0), "");
    }

    #[test]
    fn plist_environment_values_round_trip() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ocm-service-global-plist-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let plist_path = root.join("ai.openclaw.gateway.plist");
        fs::write(
            &plist_path,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>/tmp/demo/openclaw.json</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>18790</string>
    </dict>
  </dict>
</plist>
"#,
        )
        .unwrap();
        assert_eq!(
            read_launch_agent_environment_value(&plist_path, "OPENCLAW_CONFIG_PATH").unwrap(),
            Some("/tmp/demo/openclaw.json".to_string())
        );
        assert_eq!(
            read_launch_agent_environment_value(&plist_path, "OPENCLAW_GATEWAY_PORT").unwrap(),
            Some("18790".to_string())
        );
        assert_eq!(plist_unescape("&lt;a&amp;b&gt;"), "<a&b>");
        fs::remove_dir_all(&root).unwrap();
    }
}
