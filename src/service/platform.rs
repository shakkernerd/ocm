use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::store::{display_path, resolve_ocm_home, resolve_user_home};

use super::inspect::GLOBAL_GATEWAY_LABEL;

pub(crate) const OCM_GATEWAY_LABEL_PREFIX: &str = "ai.openclaw.gateway.ocm.";
const SERVICE_MANAGER_OVERRIDE: &str = "OCM_INTERNAL_SERVICE_MANAGER";
const LAUNCHCTL_BIN_OVERRIDE: &str = "OCM_INTERNAL_LAUNCHCTL_BIN";
const SYSTEMCTL_BIN_OVERRIDE: &str = "OCM_INTERNAL_SYSTEMCTL_BIN";
const STORE_HASH_LEN: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedServiceIdentity {
    pub(crate) store_hash: String,
    pub(crate) label: String,
    pub(crate) definition_path: PathBuf,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use super::{
        ManagedServiceIdentity, ServiceManagerKind, global_service_definition_path,
        managed_service_identity, managed_service_label, service_backend_support_error,
        service_definition_dir, service_manager_kind,
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
}
