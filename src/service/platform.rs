use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

use crate::store::{display_path, resolve_ocm_home, resolve_user_home};

use super::inspect::GLOBAL_GATEWAY_LABEL;

pub(crate) const OCM_GATEWAY_LABEL_PREFIX: &str = "ai.openclaw.gateway.ocm.";
const SERVICE_MANAGER_OVERRIDE: &str = "OCM_INTERNAL_SERVICE_MANAGER";
const LAUNCHCTL_BIN_OVERRIDE: &str = "OCM_INTERNAL_LAUNCHCTL_BIN";
const SYSTEMCTL_BIN_OVERRIDE: &str = "OCM_INTERNAL_SYSTEMCTL_BIN";
const STORE_HASH_LEN: usize = 10;
const SERVICE_DIR_MODE: u32 = 0o755;
const SERVICE_FILE_MODE: u32 = 0o644;
const LAUNCH_AGENT_THROTTLE_INTERVAL_SECONDS: u32 = 1;
const LAUNCH_AGENT_UMASK_DECIMAL: u32 = 0o077;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedServiceIdentity {
    pub(crate) store_hash: String,
    pub(crate) label: String,
    pub(crate) definition_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedServiceDefinition {
    pub(crate) label: String,
    pub(crate) description: String,
    pub(crate) definition_path: PathBuf,
    pub(crate) program_arguments: Vec<String>,
    pub(crate) working_directory: PathBuf,
    pub(crate) stdout_path: PathBuf,
    pub(crate) stderr_path: PathBuf,
    pub(crate) environment: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServiceManagerKind {
    Launchd,
    SystemdUser,
    Unsupported,
}

pub(crate) fn unsupported_service_manager_message() -> &'static str {
    "managed services are not supported on this platform yet; run OpenClaw directly inside the env for now"
}

pub(crate) fn service_backend_support_error(env: &BTreeMap<String, String>) -> Option<String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
            let binary = service_manager_binary(env, ServiceManagerKind::Launchd);
            if !binary_available(binary, env) {
                Some(
                    "managed services require launchctl on this machine; run OpenClaw directly inside the env for now"
                        .to_string(),
                )
            } else if launchd_available(env) {
                None
            } else {
                Some(
                    "managed services require a usable launchctl session on this machine; run OpenClaw directly inside the env for now"
                        .to_string(),
                )
            }
        }
        ServiceManagerKind::SystemdUser => {
            let binary = service_manager_binary(env, ServiceManagerKind::SystemdUser);
            if !binary_available(binary, env) {
                Some(
                    "managed services require systemctl --user on this machine; run OpenClaw directly inside the env for now"
                        .to_string(),
                )
            } else if systemd_user_available(env) {
                None
            } else {
                Some(
                    "managed services require a usable systemctl --user session on this machine; run OpenClaw directly inside the env for now"
                        .to_string(),
                )
            }
        }
        ServiceManagerKind::Unsupported => Some(unsupported_service_manager_message().to_string()),
    }
}

pub(crate) fn service_manager_kind(env: &BTreeMap<String, String>) -> ServiceManagerKind {
    if let Some(value) = env.get(SERVICE_MANAGER_OVERRIDE) {
        match value.trim().to_ascii_lowercase().as_str() {
            "launchd" => return ServiceManagerKind::Launchd,
            "systemd" | "systemd-user" | "systemd_user" => {
                return ServiceManagerKind::SystemdUser;
            }
            "unsupported" => return ServiceManagerKind::Unsupported,
            _ => {}
        }
    }

    if cfg!(target_os = "linux") {
        ServiceManagerKind::SystemdUser
    } else if cfg!(target_os = "macos") {
        ServiceManagerKind::Launchd
    } else {
        ServiceManagerKind::Unsupported
    }
}

fn service_manager_binary(env: &BTreeMap<String, String>, kind: ServiceManagerKind) -> &str {
    match kind {
        ServiceManagerKind::Launchd => env
            .get(LAUNCHCTL_BIN_OVERRIDE)
            .map(String::as_str)
            .unwrap_or("launchctl"),
        ServiceManagerKind::SystemdUser => env
            .get(SYSTEMCTL_BIN_OVERRIDE)
            .map(String::as_str)
            .unwrap_or("systemctl"),
        ServiceManagerKind::Unsupported => "",
    }
}

fn binary_available(binary: &str, env: &BTreeMap<String, String>) -> bool {
    if binary.trim().is_empty() {
        return false;
    }
    let path = Path::new(binary);
    if path.is_absolute() || binary.contains(std::path::MAIN_SEPARATOR) {
        return path.is_file();
    }
    let Some(path_value) = env.get("PATH") else {
        return false;
    };
    std::env::split_paths(path_value)
        .map(|dir| dir.join(binary))
        .any(|candidate| candidate.is_file())
}

fn launchd_available(env: &BTreeMap<String, String>) -> bool {
    Command::new(service_manager_binary(env, ServiceManagerKind::Launchd))
        .arg("managername")
        .envs(env)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn systemd_user_available(env: &BTreeMap<String, String>) -> bool {
    Command::new(service_manager_binary(env, ServiceManagerKind::SystemdUser))
        .args(["--user", "show-environment"])
        .envs(env)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn service_store_hash(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<String, String> {
    let store = resolve_ocm_home(env, cwd)?;
    let mut hasher = Sha256::new();
    hasher.update(display_path(&store).as_bytes());
    let digest = hasher.finalize();
    let hex = format!("{digest:x}");
    Ok(hex[..STORE_HASH_LEN].to_string())
}

pub(crate) fn managed_service_label(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<String, String> {
    Ok(format!(
        "{OCM_GATEWAY_LABEL_PREFIX}{}.{}",
        service_store_hash(env, cwd)?,
        name
    ))
}

pub(crate) fn managed_service_identity(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ManagedServiceIdentity, String> {
    let label = managed_service_label(name, env, cwd)?;
    Ok(ManagedServiceIdentity {
        store_hash: service_store_hash(env, cwd)?,
        definition_path: service_definition_dir(env).join(format!(
            "{}.{}",
            label,
            service_definition_extension(service_manager_kind(env))
        )),
        label,
    })
}

pub(crate) fn global_service_definition_path(env: &BTreeMap<String, String>) -> PathBuf {
    service_definition_dir(env).join(format!(
        "{}.{}",
        GLOBAL_GATEWAY_LABEL,
        service_definition_extension(service_manager_kind(env))
    ))
}

pub(crate) fn service_definition_dir(env: &BTreeMap<String, String>) -> PathBuf {
    let home = resolve_user_home(env);
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => home.join("Library").join("LaunchAgents"),
        ServiceManagerKind::SystemdUser => home.join(".config").join("systemd").join("user"),
        ServiceManagerKind::Unsupported => home.join(".ocm").join("unsupported-services"),
    }
}

pub(crate) fn service_definition_extension(kind: ServiceManagerKind) -> &'static str {
    match kind {
        ServiceManagerKind::Launchd => "plist",
        ServiceManagerKind::SystemdUser => "service",
        ServiceManagerKind::Unsupported => "service",
    }
}

pub(crate) fn write_managed_service_definition(
    definition: &ManagedServiceDefinition,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    ensure_secure_dir(
        &definition
            .stdout_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                format!(
                    "failed to resolve stdout log directory for {}",
                    display_path(&definition.stdout_path)
                )
            })?,
    )?;
    ensure_secure_dir(
        &definition
            .stderr_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                format!(
                    "failed to resolve stderr log directory for {}",
                    display_path(&definition.stderr_path)
                )
            })?,
    )?;
    let Some(parent) = definition.definition_path.parent() else {
        return Err(format!(
            "failed to resolve service definition directory for {}",
            display_path(&definition.definition_path)
        ));
    };
    ensure_secure_dir(parent)?;

    let raw = match service_manager_kind(env) {
        ServiceManagerKind::Launchd => build_launch_agent_plist(
            &definition.label,
            &definition.description,
            &definition.program_arguments,
            &definition.working_directory,
            &definition.stdout_path,
            &definition.stderr_path,
            &definition.environment,
        ),
        ServiceManagerKind::SystemdUser => build_systemd_unit(
            &definition.description,
            &definition.program_arguments,
            &definition.working_directory,
            &definition.environment,
        ),
        ServiceManagerKind::Unsupported => {
            return Err(unsupported_service_manager_message().to_string());
        }
    };
    fs::write(&definition.definition_path, raw).map_err(|error| error.to_string())?;
    set_mode(&definition.definition_path, SERVICE_FILE_MODE)
}

pub(crate) fn activate_managed_service(
    label: &str,
    definition_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => activate_launchd_service(label, definition_path, env),
        ServiceManagerKind::SystemdUser => activate_systemd_user_service(label, env),
        ServiceManagerKind::Unsupported => Err(unsupported_service_manager_message().to_string()),
    }
}

pub(crate) fn stop_managed_service(
    label: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
            let target = format!("{}/{}", gui_domain(), label);
            let bootout = run_launchctl(env, ["bootout", target.as_str()])?;
            if !bootout.status.success() && !launchctl_not_loaded(&bootout) {
                return Err(format!(
                    "launchctl bootout failed: {}",
                    launchctl_detail(&bootout)
                ));
            }
            Ok(())
        }
        ServiceManagerKind::SystemdUser => {
            let stop = run_systemctl(env, ["--user", "stop", label])?;
            if !stop.status.success() && !systemctl_not_loaded(&stop) {
                return Err(format!(
                    "systemctl --user stop failed: {}",
                    systemctl_detail(&stop)
                ));
            }
            Ok(())
        }
        ServiceManagerKind::Unsupported => Err(unsupported_service_manager_message().to_string()),
    }
}

pub(crate) fn uninstall_managed_service(
    label: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => {
            let target = format!("{}/{}", gui_domain(), label);
            let _ = run_launchctl(env, ["bootout", target.as_str()]);
            Ok(())
        }
        ServiceManagerKind::SystemdUser => {
            let _ = run_systemctl(env, ["--user", "disable", "--now", label]);
            let reload = run_systemctl(env, ["--user", "daemon-reload"])?;
            if !reload.status.success() {
                return Err(format!(
                    "systemctl --user daemon-reload failed: {}",
                    systemctl_detail(&reload)
                ));
            }
            Ok(())
        }
        ServiceManagerKind::Unsupported => Err(unsupported_service_manager_message().to_string()),
    }
}

fn build_launch_agent_plist(
    label: &str,
    comment: &str,
    program_arguments: &[String],
    working_directory: &Path,
    stdout_path: &Path,
    stderr_path: &Path,
    environment: &BTreeMap<String, String>,
) -> String {
    let args_xml = program_arguments
        .iter()
        .map(|arg| format!("\n      <string>{}</string>", plist_escape(arg)))
        .collect::<String>();
    let env_xml = if environment.is_empty() {
        String::new()
    } else {
        let items = environment
            .iter()
            .filter(|(_, value)| !value.trim().is_empty())
            .map(|(key, value)| {
                format!(
                    "\n      <key>{}</key>\n      <string>{}</string>",
                    plist_escape(key),
                    plist_escape(value)
                )
            })
            .collect::<String>();
        format!("\n    <key>EnvironmentVariables</key>\n    <dict>{items}\n    </dict>")
    };

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n  <dict>\n    <key>Label</key>\n    <string>{}</string>\n    <key>Comment</key>\n    <string>{}</string>\n    <key>RunAtLoad</key>\n    <true/>\n    <key>KeepAlive</key>\n    <true/>\n    <key>ThrottleInterval</key>\n    <integer>{}</integer>\n    <key>Umask</key>\n    <integer>{}</integer>\n    <key>ProgramArguments</key>\n    <array>{}\n    </array>\n    <key>WorkingDirectory</key>\n    <string>{}</string>\n    <key>StandardOutPath</key>\n    <string>{}</string>\n    <key>StandardErrorPath</key>\n    <string>{}</string>{}\n  </dict>\n</plist>\n",
        plist_escape(label),
        plist_escape(comment),
        LAUNCH_AGENT_THROTTLE_INTERVAL_SECONDS,
        LAUNCH_AGENT_UMASK_DECIMAL,
        args_xml,
        plist_escape(&display_path(working_directory)),
        plist_escape(&display_path(stdout_path)),
        plist_escape(&display_path(stderr_path)),
        env_xml,
    )
}

fn build_systemd_unit(
    description: &str,
    program_arguments: &[String],
    working_directory: &Path,
    environment: &BTreeMap<String, String>,
) -> String {
    let exec_start = program_arguments
        .iter()
        .map(|arg| systemd_quote(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let environment_lines = environment
        .iter()
        .filter(|(_, value)| !value.trim().is_empty())
        .map(|(key, value)| {
            format!(
                "Environment=\"{}={}\"",
                systemd_escape(key),
                systemd_escape(value)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let environment_block = if environment_lines.is_empty() {
        String::new()
    } else {
        format!("{environment_lines}\n")
    };

    format!(
        "[Unit]\nDescription={}\nAfter=network.target\n\n[Service]\nType=simple\nWorkingDirectory={}\nExecStart={}\n{}Restart=always\nRestartSec=1\nUMask=0077\n\n[Install]\nWantedBy=default.target\n",
        systemd_escape(description),
        systemd_quote(&display_path(working_directory)),
        exec_start,
        environment_block,
    )
}

fn plist_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn ensure_secure_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|error| error.to_string())?;
    set_mode(path, SERVICE_DIR_MODE)
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

fn activate_launchd_service(
    label: &str,
    definition_path: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let domain = gui_domain();
    let definition_path = display_path(definition_path);
    let _ = run_launchctl(env, ["bootout", domain.as_str(), definition_path.as_str()]);
    let _ = run_launchctl(env, ["unload", definition_path.as_str()]);
    let target = format!("{domain}/{label}");
    let _ = run_launchctl(env, ["enable", target.as_str()]);
    let bootstrap = run_launchctl(
        env,
        ["bootstrap", domain.as_str(), definition_path.as_str()],
    )?;
    if !bootstrap.status.success() {
        return Err(format!(
            "launchctl bootstrap failed: {}",
            launchctl_detail(&bootstrap)
        ));
    }
    Ok(())
}

fn activate_systemd_user_service(
    label: &str,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let reload = run_systemctl(env, ["--user", "daemon-reload"])?;
    if !reload.status.success() {
        return Err(format!(
            "systemctl --user daemon-reload failed: {}",
            systemctl_detail(&reload)
        ));
    }

    let enable = run_systemctl(env, ["--user", "enable", label])?;
    if !enable.status.success() {
        return Err(format!(
            "systemctl --user enable failed: {}",
            systemctl_detail(&enable)
        ));
    }

    let restart = run_systemctl(env, ["--user", "restart", label])?;
    if restart.status.success() {
        return Ok(());
    }

    let start = run_systemctl(env, ["--user", "start", label])?;
    if !start.status.success() {
        return Err(format!(
            "systemctl --user restart/start failed: {}; {}",
            systemctl_detail(&restart),
            systemctl_detail(&start)
        ));
    }

    Ok(())
}

fn gui_domain() -> String {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .map(|uid| format!("gui/{uid}"))
        .unwrap_or_else(|| "gui/501".to_string())
}

fn run_launchctl<const N: usize>(
    env: &BTreeMap<String, String>,
    args: [&str; N],
) -> Result<Output, String> {
    Command::new(service_manager_binary(env, ServiceManagerKind::Launchd))
        .args(args)
        .output()
        .map_err(|error| format!("failed to run \"launchctl\": {error}"))
}

fn run_systemctl<const N: usize>(
    env: &BTreeMap<String, String>,
    args: [&str; N],
) -> Result<Output, String> {
    Command::new(service_manager_binary(env, ServiceManagerKind::SystemdUser))
        .args(args)
        .output()
        .map_err(|error| format!("failed to run \"systemctl\": {error}"))
}

fn launchctl_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn launchctl_not_loaded(output: &Output) -> bool {
    let detail = launchctl_detail(output).to_ascii_lowercase();
    detail.contains("no such process")
        || detail.contains("could not find service")
        || detail.contains("not found")
}

fn systemctl_detail(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !stderr.is_empty() {
        return stderr;
    }
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn systemctl_not_loaded(output: &Output) -> bool {
    let detail = systemctl_detail(output).to_ascii_lowercase();
    detail.contains("not loaded") || detail.contains("not found") || detail.contains("no such file")
}

fn systemd_quote(value: &str) -> String {
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\\'))
    {
        format!("\"{}\"", systemd_escape(value))
    } else {
        systemd_escape(value)
    }
}

fn systemd_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        ManagedServiceDefinition, ManagedServiceIdentity, ServiceManagerKind,
        global_service_definition_path, managed_service_identity, managed_service_label,
        service_backend_support_error, service_definition_dir, service_manager_kind,
        write_managed_service_definition,
    };

    #[test]
    fn manager_override_supports_systemd_user() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );
        assert_eq!(service_manager_kind(&env), ServiceManagerKind::SystemdUser);
    }

    #[test]
    fn manager_override_supports_unsupported_backends() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "unsupported".to_string(),
        );
        assert_eq!(service_manager_kind(&env), ServiceManagerKind::Unsupported);
    }

    #[test]
    fn backend_support_error_reports_missing_launchctl_binary() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "launchd".to_string(),
        );
        env.insert(
            "OCM_INTERNAL_LAUNCHCTL_BIN".to_string(),
            "/tmp/ocm-tests/missing-launchctl".to_string(),
        );

        assert_eq!(
            service_backend_support_error(&env),
            Some(
                "managed services require launchctl on this machine; run OpenClaw directly inside the env for now"
                    .to_string()
            )
        );
    }

    #[test]
    fn backend_support_error_reports_missing_systemctl_binary() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );
        env.insert(
            "OCM_INTERNAL_SYSTEMCTL_BIN".to_string(),
            "/tmp/ocm-tests/missing-systemctl".to_string(),
        );

        assert_eq!(
            service_backend_support_error(&env),
            Some(
                "managed services require systemctl --user on this machine; run OpenClaw directly inside the env for now"
                    .to_string()
            )
        );
    }

    #[test]
    fn backend_support_error_reports_unusable_launchctl_session() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "launchd".to_string(),
        );
        env.insert(
            "OCM_INTERNAL_LAUNCHCTL_BIN".to_string(),
            "/bin/sh".to_string(),
        );
        env.insert("HOME".to_string(), "/tmp".to_string());

        assert_eq!(
            service_backend_support_error(&env),
            Some(
                "managed services require a usable launchctl session on this machine; run OpenClaw directly inside the env for now"
                    .to_string()
            )
        );
    }

    #[test]
    fn backend_support_error_reports_unusable_systemd_session() {
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );
        env.insert(
            "OCM_INTERNAL_SYSTEMCTL_BIN".to_string(),
            "/bin/sh".to_string(),
        );
        env.insert("HOME".to_string(), "/tmp".to_string());

        assert_eq!(
            service_backend_support_error(&env),
            Some(
                "managed services require a usable systemctl --user session on this machine; run OpenClaw directly inside the env for now"
                    .to_string()
            )
        );
    }

    #[test]
    fn service_paths_follow_the_selected_backend() {
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/tmp/home".to_string());
        env.insert("OCM_HOME".to_string(), "/tmp/store".to_string());
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );

        assert_eq!(
            service_definition_dir(&env).display().to_string(),
            "/tmp/home/.config/systemd/user"
        );
        assert_eq!(
            managed_service_identity("demo", &env, Path::new("/tmp"))
                .unwrap()
                .definition_path
                .display()
                .to_string(),
            format!(
                "/tmp/home/.config/systemd/user/{}.service",
                managed_service_label("demo", &env, Path::new("/tmp")).unwrap()
            )
        );
        assert_eq!(
            global_service_definition_path(&env).display().to_string(),
            "/tmp/home/.config/systemd/user/ai.openclaw.gateway.service"
        );
    }

    #[test]
    fn managed_service_labels_are_store_scoped() {
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/tmp/home".to_string());
        env.insert("OCM_HOME".to_string(), "/tmp/store".to_string());

        let label = managed_service_label("demo", &env, Path::new("/tmp")).unwrap();
        assert!(label.starts_with("ai.openclaw.gateway.ocm."));
        assert!(label.ends_with(".demo"));
        assert!(matches!(
            managed_service_identity("demo", &env, Path::new("/tmp")).unwrap(),
            ManagedServiceIdentity {
                label,
                definition_path,
                ..
            } if definition_path.display().to_string().contains(&label)
        ));
    }

    #[test]
    fn launch_agent_definition_includes_program_arguments_and_environment() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ocm-platform-launchd-{unique}"));
        fs::create_dir_all(root.join("home")).unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), display_path(&root.join("home")));
        env.insert("OCM_HOME".to_string(), display_path(&root.join("store")));
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "launchd".to_string(),
        );

        let definition = ManagedServiceDefinition {
            label: "ai.openclaw.gateway.ocm.test".to_string(),
            description: "test".to_string(),
            definition_path: root.join("home/Library/LaunchAgents/test.plist"),
            program_arguments: vec![
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "openclaw gateway run".to_string(),
            ],
            working_directory: Path::new("/tmp/work").to_path_buf(),
            stdout_path: root.join("logs/stdout.log"),
            stderr_path: root.join("logs/stderr.log"),
            environment: BTreeMap::from([("PATH".to_string(), "/usr/bin".to_string())]),
        };

        write_managed_service_definition(&definition, &env).unwrap();
        let plist = fs::read_to_string(&definition.definition_path).unwrap();
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("ai.openclaw.gateway.ocm.test"));
        assert!(plist.contains("openclaw gateway run"));
        assert!(plist.contains("<key>EnvironmentVariables</key>"));
        assert!(plist.contains(&display_path(&definition.stdout_path)));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn systemd_definition_includes_exec_start_and_environment() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("ocm-platform-systemd-{unique}"));
        fs::create_dir_all(root.join("home")).unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), display_path(&root.join("home")));
        env.insert("OCM_HOME".to_string(), display_path(&root.join("store")));
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );

        let definition = ManagedServiceDefinition {
            label: "ai.openclaw.gateway.ocm.test".to_string(),
            description: "test".to_string(),
            definition_path: root.join("home/.config/systemd/user/test.service"),
            program_arguments: vec![
                "/bin/sh".to_string(),
                "-lc".to_string(),
                "openclaw gateway run --port 18789".to_string(),
            ],
            working_directory: Path::new("/tmp/work").to_path_buf(),
            stdout_path: root.join("logs/stdout.log"),
            stderr_path: root.join("logs/stderr.log"),
            environment: BTreeMap::from([
                ("PATH".to_string(), "/usr/bin".to_string()),
                ("OPENCLAW_HOME".to_string(), "/tmp/demo".to_string()),
            ]),
        };

        write_managed_service_definition(&definition, &env).unwrap();
        let unit = fs::read_to_string(&definition.definition_path).unwrap();
        assert!(unit.contains("[Service]"));
        assert!(unit.contains("ExecStart=/bin/sh -lc"));
        assert!(unit.contains("WorkingDirectory=/tmp/work"));
        assert!(unit.contains("Environment=\"OPENCLAW_HOME=/tmp/demo\""));

        fs::remove_dir_all(&root).unwrap();
    }
}
