use serde::Serialize;

use crate::env::EnvMeta;

use super::OcmManifest;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ManifestApplyPlan {
    pub env_name: String,
    pub create_env: bool,
    pub desired_runtime: Option<String>,
    pub desired_launcher: Option<String>,
    pub runtime_changed: bool,
    pub launcher_changed: bool,
    pub desired_service_install: Option<bool>,
    pub service_changed: bool,
}

pub fn plan_manifest_application(
    manifest: &OcmManifest,
    current: Option<&EnvMeta>,
) -> ManifestApplyPlan {
    plan_manifest_application_with_service(manifest, current, None)
}

pub fn plan_manifest_application_with_service(
    manifest: &OcmManifest,
    current: Option<&EnvMeta>,
    current_service_installed: Option<bool>,
) -> ManifestApplyPlan {
    let desired_runtime = manifest.runtime.as_ref().and_then(|runtime| {
        runtime
            .name
            .clone()
            .or(runtime.version.clone())
            .or(runtime.channel.clone())
    });
    let desired_launcher = manifest
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.name.clone());
    let current_runtime = current.and_then(|meta| meta.default_runtime.clone());
    let current_launcher = current.and_then(|meta| meta.default_launcher.clone());
    let desired_service_install = manifest
        .service
        .as_ref()
        .and_then(|service| service.install);
    let service_changed = match desired_service_install {
        Some(desired) => current_service_installed != Some(desired),
        None => false,
    };

    ManifestApplyPlan {
        env_name: manifest.env.name.clone(),
        create_env: current.is_none(),
        runtime_changed: desired_runtime != current_runtime,
        launcher_changed: desired_launcher != current_launcher,
        desired_runtime,
        desired_launcher,
        desired_service_install,
        service_changed,
    }
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use crate::env::EnvMeta;
    use crate::manifest::{
        ManifestEnv, ManifestLauncher, ManifestRuntime, ManifestService, OcmManifest,
        plan_manifest_application, plan_manifest_application_with_service,
    };

    fn manifest_with_launcher() -> OcmManifest {
        OcmManifest {
            schema: "ocm/v1".to_string(),
            env: ManifestEnv {
                name: "mira".to_string(),
            },
            runtime: None,
            launcher: Some(ManifestLauncher {
                name: Some("dev".to_string()),
            }),
            service: Some(ManifestService {
                install: Some(true),
            }),
        }
    }

    fn manifest_with_runtime() -> OcmManifest {
        OcmManifest {
            schema: "ocm/v1".to_string(),
            env: ManifestEnv {
                name: "mira".to_string(),
            },
            runtime: Some(ManifestRuntime {
                channel: Some("stable".to_string()),
                version: None,
                name: None,
            }),
            launcher: None,
            service: Some(ManifestService {
                install: Some(true),
            }),
        }
    }

    fn env_meta() -> EnvMeta {
        EnvMeta {
            kind: "ocm-env".to_string(),
            name: "mira".to_string(),
            root: "/tmp/mira".to_string(),
            gateway_port: None,
            default_runtime: None,
            default_launcher: Some("dev".to_string()),
            protected: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_used_at: None,
        }
    }

    #[test]
    fn plan_manifest_application_marks_missing_envs_for_creation() {
        let plan = plan_manifest_application(&manifest_with_launcher(), None);
        assert_eq!(plan.env_name, "mira");
        assert!(plan.create_env);
        assert!(plan.launcher_changed);
        assert!(!plan.runtime_changed);
        assert_eq!(plan.desired_launcher.as_deref(), Some("dev"));
        assert_eq!(plan.desired_service_install, Some(true));
        assert!(plan.service_changed);
    }

    #[test]
    fn plan_manifest_application_detects_matching_launcher_bindings() {
        let current = env_meta();
        let plan = plan_manifest_application_with_service(
            &manifest_with_launcher(),
            Some(&current),
            Some(true),
        );
        assert!(!plan.create_env);
        assert!(!plan.launcher_changed);
        assert!(!plan.runtime_changed);
        assert!(!plan.service_changed);
    }

    #[test]
    fn plan_manifest_application_detects_runtime_binding_changes() {
        let current = env_meta();
        let plan = plan_manifest_application(&manifest_with_runtime(), Some(&current));
        assert!(!plan.create_env);
        assert!(plan.runtime_changed);
        assert!(plan.launcher_changed);
        assert_eq!(plan.desired_runtime.as_deref(), Some("stable"));
        assert_eq!(plan.desired_launcher, None);
        assert!(plan.service_changed);
    }

    #[test]
    fn plan_manifest_application_tracks_matching_service_installs() {
        let current = env_meta();
        let plan = plan_manifest_application_with_service(
            &manifest_with_launcher(),
            Some(&current),
            Some(true),
        );
        assert!(!plan.service_changed);
    }
}
