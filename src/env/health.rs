use std::fs;
use std::path::Path;

use serde::Serialize;

use super::{EnvMarker, EnvMeta, EnvironmentService, ExecutionBinding, resolve_execution_binding};
use crate::store::{derive_env_paths, display_path};
use crate::store::{
    get_launcher, get_runtime, repair_environment_marker, runtime_integrity_issue, save_environment,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvDoctorSummary {
    pub env_name: String,
    pub root: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub healthy: bool,
    pub root_status: String,
    pub marker_status: String,
    pub runtime_status: String,
    pub launcher_status: String,
    pub resolution_status: String,
    pub resolved_kind: Option<String>,
    pub resolved_name: Option<String>,
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvMarkerRepairSummary {
    pub env_name: String,
    pub root: String,
    pub marker_path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCleanupActionSummary {
    pub kind: String,
    pub description: String,
    pub applied: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCleanupSummary {
    pub env_name: String,
    pub root: String,
    pub apply: bool,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub healthy_before: bool,
    pub healthy_after: Option<bool>,
    pub actions: Vec<EnvCleanupActionSummary>,
    pub issues_before: Vec<String>,
    pub issues_after: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCleanupBatchSummary {
    pub apply: bool,
    pub count: usize,
    pub results: Vec<EnvCleanupSummary>,
}

#[derive(Clone, Debug)]
struct PlannedCleanupAction {
    kind: &'static str,
    description: String,
}

impl<'a> EnvironmentService<'a> {
    pub fn repair_marker(&self, name: &str) -> Result<EnvMarkerRepairSummary, String> {
        repair_environment_marker(name, self.env, self.cwd)
    }

    pub fn cleanup_preview(&self, name: &str) -> Result<EnvCleanupSummary, String> {
        let env = self.get(name)?;
        let doctor = self.doctor(name)?;
        let actions = cleanup_actions(&env, &doctor);
        Ok(build_cleanup_summary(&env, doctor, false, actions, None))
    }

    pub fn cleanup(&self, name: &str) -> Result<EnvCleanupSummary, String> {
        let mut env = self.get(name)?;
        let doctor_before = self.doctor(name)?;
        let actions = cleanup_actions(&env, &doctor_before);

        for action in &actions {
            match action.kind {
                "repair-marker" => {
                    repair_environment_marker(&env.name, self.env, self.cwd)?;
                }
                "clear-missing-runtime" => {
                    env.default_runtime = None;
                }
                "clear-missing-launcher" => {
                    env.default_launcher = None;
                }
                _ => {}
            }
        }

        if actions.iter().any(|action| {
            matches!(
                action.kind,
                "clear-missing-runtime" | "clear-missing-launcher"
            )
        }) {
            env = save_environment(env, self.env, self.cwd)?;
        } else {
            env = self.get(name)?;
        }

        let doctor_after = self.doctor(name)?;
        Ok(build_cleanup_summary(
            &env,
            doctor_before,
            true,
            actions,
            Some(doctor_after),
        ))
    }

    pub fn cleanup_all_preview(&self) -> Result<EnvCleanupBatchSummary, String> {
        self.cleanup_all_internal(false)
    }

    pub fn cleanup_all(&self) -> Result<EnvCleanupBatchSummary, String> {
        self.cleanup_all_internal(true)
    }

    pub fn doctor(&self, name: &str) -> Result<EnvDoctorSummary, String> {
        let env = self.get(name)?;
        let env_paths = derive_env_paths(Path::new(&env.root));
        let mut issues = Vec::new();

        let root_status = if env_paths.root.exists() {
            "ok".to_string()
        } else {
            push_issue(
                &mut issues,
                format!(
                    "environment root does not exist: {}",
                    display_path(&env_paths.root)
                ),
            );
            "missing".to_string()
        };

        let marker_status = if env_paths.marker_path.exists() {
            match fs::read_to_string(&env_paths.marker_path) {
                Ok(raw) => match serde_json::from_str::<EnvMarker>(&raw) {
                    Ok(marker) if marker.name == env.name => "ok".to_string(),
                    Ok(marker) => {
                        push_issue(
                            &mut issues,
                            format!(
                                "environment marker name mismatch: expected \"{}\", found \"{}\"",
                                env.name, marker.name
                            ),
                        );
                        "mismatch".to_string()
                    }
                    Err(error) => {
                        push_issue(
                            &mut issues,
                            format!(
                                "environment marker is unreadable: {} ({error})",
                                display_path(&env_paths.marker_path)
                            ),
                        );
                        "invalid".to_string()
                    }
                },
                Err(error) => {
                    push_issue(
                        &mut issues,
                        format!(
                            "environment marker is unreadable: {} ({error})",
                            display_path(&env_paths.marker_path)
                        ),
                    );
                    "invalid".to_string()
                }
            }
        } else {
            push_issue(
                &mut issues,
                format!(
                    "environment marker is missing: {}",
                    display_path(&env_paths.marker_path)
                ),
            );
            "missing".to_string()
        };

        let runtime_status = if let Some(runtime_name) = env.default_runtime.clone() {
            match get_runtime(&runtime_name, self.env, self.cwd) {
                Ok(runtime) => match runtime_integrity_issue(&runtime) {
                    Some(issue) => {
                        push_issue(&mut issues, format!("runtime \"{}\" {issue}", runtime.name));
                        "broken".to_string()
                    }
                    None => "ok".to_string(),
                },
                Err(error) => {
                    push_issue(&mut issues, error);
                    "missing".to_string()
                }
            }
        } else {
            "unbound".to_string()
        };

        let launcher_status = if let Some(launcher_name) = env.default_launcher.clone() {
            match get_launcher(&launcher_name, self.env, self.cwd) {
                Ok(_) => "ok".to_string(),
                Err(error) => {
                    push_issue(&mut issues, error);
                    "missing".to_string()
                }
            }
        } else {
            "unbound".to_string()
        };

        let (resolution_status, resolved_kind, resolved_name) =
            match resolve_execution_binding(&env, None, None) {
                Ok(ExecutionBinding::Runtime(runtime_name)) => {
                    let resolution_status = if runtime_status == "ok" {
                        "ok".to_string()
                    } else {
                        "error".to_string()
                    };
                    (
                        resolution_status,
                        Some("runtime".to_string()),
                        Some(runtime_name),
                    )
                }
                Ok(ExecutionBinding::Launcher(launcher_name)) => {
                    let resolution_status = if launcher_status == "ok" {
                        "ok".to_string()
                    } else {
                        "error".to_string()
                    };
                    (
                        resolution_status,
                        Some("launcher".to_string()),
                        Some(launcher_name),
                    )
                }
                Err(error) => {
                    push_issue(&mut issues, error);
                    ("unbound".to_string(), None, None)
                }
            };

        Ok(EnvDoctorSummary {
            env_name: env.name,
            root: env.root,
            default_runtime: env.default_runtime,
            default_launcher: env.default_launcher,
            healthy: issues.is_empty(),
            root_status,
            marker_status,
            runtime_status,
            launcher_status,
            resolution_status,
            resolved_kind,
            resolved_name,
            issues,
        })
    }

    fn cleanup_all_internal(&self, apply: bool) -> Result<EnvCleanupBatchSummary, String> {
        let env_names = self
            .list()?
            .into_iter()
            .map(|env| env.name)
            .collect::<Vec<_>>();
        let mut results = Vec::new();

        for env_name in env_names {
            let summary = if apply {
                self.cleanup(&env_name)?
            } else {
                self.cleanup_preview(&env_name)?
            };
            if !summary.actions.is_empty() {
                results.push(summary);
            }
        }

        Ok(EnvCleanupBatchSummary {
            apply,
            count: results.len(),
            results,
        })
    }
}

fn push_issue(issues: &mut Vec<String>, issue: String) {
    if !issues.iter().any(|current| current == &issue) {
        issues.push(issue);
    }
}

fn cleanup_actions(env: &EnvMeta, doctor: &EnvDoctorSummary) -> Vec<PlannedCleanupAction> {
    let mut actions = Vec::new();

    if doctor.root_status == "ok" && doctor.marker_status != "ok" {
        actions.push(PlannedCleanupAction {
            kind: "repair-marker",
            description: "rewrite the environment marker file".to_string(),
        });
    }

    if doctor.runtime_status == "missing" {
        if let Some(runtime_name) = env.default_runtime.as_deref() {
            actions.push(PlannedCleanupAction {
                kind: "clear-missing-runtime",
                description: format!("clear missing runtime binding \"{runtime_name}\""),
            });
        }
    }

    if doctor.launcher_status == "missing" {
        if let Some(launcher_name) = env.default_launcher.as_deref() {
            actions.push(PlannedCleanupAction {
                kind: "clear-missing-launcher",
                description: format!("clear missing launcher binding \"{launcher_name}\""),
            });
        }
    }

    actions
}

fn build_cleanup_summary(
    env: &EnvMeta,
    doctor_before: EnvDoctorSummary,
    apply: bool,
    actions: Vec<PlannedCleanupAction>,
    doctor_after: Option<EnvDoctorSummary>,
) -> EnvCleanupSummary {
    let actions = actions
        .into_iter()
        .map(|action| EnvCleanupActionSummary {
            kind: action.kind.to_string(),
            description: action.description,
            applied: apply,
        })
        .collect();

    EnvCleanupSummary {
        env_name: env.name.clone(),
        root: env.root.clone(),
        apply,
        default_runtime: env.default_runtime.clone(),
        default_launcher: env.default_launcher.clone(),
        healthy_before: doctor_before.healthy,
        healthy_after: doctor_after.as_ref().map(|doctor| doctor.healthy),
        actions,
        issues_before: doctor_before.issues,
        issues_after: doctor_after.map(|doctor| doctor.issues),
    }
}
