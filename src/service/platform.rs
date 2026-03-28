use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::store::resolve_user_home;

use super::inspect::GLOBAL_GATEWAY_LABEL;

pub(crate) const OCM_GATEWAY_LABEL_PREFIX: &str = "ai.openclaw.gateway.ocm.";
const SERVICE_MANAGER_OVERRIDE: &str = "OCM_INTERNAL_SERVICE_MANAGER";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServiceManagerKind {
    Launchd,
    SystemdUser,
}

pub(crate) fn service_manager_kind(env: &BTreeMap<String, String>) -> ServiceManagerKind {
    if let Some(value) = env.get(SERVICE_MANAGER_OVERRIDE) {
        match value.trim().to_ascii_lowercase().as_str() {
            "launchd" => return ServiceManagerKind::Launchd,
            "systemd" | "systemd-user" | "systemd_user" => {
                return ServiceManagerKind::SystemdUser;
            }
            _ => {}
        }
    }

    if cfg!(target_os = "linux") {
        ServiceManagerKind::SystemdUser
    } else {
        ServiceManagerKind::Launchd
    }
}

pub(crate) fn managed_service_label(name: &str) -> String {
    format!("{OCM_GATEWAY_LABEL_PREFIX}{name}")
}

pub(crate) fn managed_service_definition_path(
    name: &str,
    env: &BTreeMap<String, String>,
) -> PathBuf {
    service_definition_dir(env).join(format!(
        "{}.{}",
        managed_service_label(name),
        service_definition_extension(service_manager_kind(env))
    ))
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
    }
}

pub(crate) fn service_definition_extension(kind: ServiceManagerKind) -> &'static str {
    match kind {
        ServiceManagerKind::Launchd => "plist",
        ServiceManagerKind::SystemdUser => "service",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        ServiceManagerKind, global_service_definition_path, managed_service_definition_path,
        managed_service_label, service_definition_dir, service_manager_kind,
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
    fn service_paths_follow_the_selected_backend() {
        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), "/tmp/home".to_string());
        env.insert(
            "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
            "systemd-user".to_string(),
        );

        assert_eq!(
            service_definition_dir(&env).display().to_string(),
            "/tmp/home/.config/systemd/user"
        );
        assert_eq!(
            managed_service_definition_path("demo", &env)
                .display()
                .to_string(),
            "/tmp/home/.config/systemd/user/ai.openclaw.gateway.ocm.demo.service"
        );
        assert_eq!(
            global_service_definition_path(&env).display().to_string(),
            "/tmp/home/.config/systemd/user/ai.openclaw.gateway.service"
        );
    }

    #[test]
    fn managed_service_labels_stay_env_scoped() {
        assert_eq!(
            managed_service_label("demo"),
            "ai.openclaw.gateway.ocm.demo"
        );
    }
}
