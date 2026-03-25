use std::fs;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::execution::{
    ExecutionBinding, build_launcher_command, resolve_execution_binding, resolve_launcher_run_dir,
    resolve_runtime_run_dir,
};
use crate::paths::{derive_env_paths, display_path};
use crate::store::{
    clone_environment, create_env_snapshot, create_environment, export_environment,
    get_env_snapshot, get_environment, get_launcher, get_runtime, get_runtime_verified,
    import_environment, list_all_env_snapshots, list_env_snapshots, list_environments, now_utc,
    remove_environment, remove_env_snapshot, repair_environment_marker, restore_env_snapshot,
    runtime_integrity_issue, save_environment, select_prune_candidates,
    select_snapshot_prune_candidates, summarize_snapshot,
};
use crate::types::{
    EnvCleanupActionSummary, EnvCleanupSummary, EnvDoctorSummary, EnvMarkerRepairSummary,
    EnvStatusSummary,
};
use crate::types::{
    CloneEnvironmentOptions, CreateEnvSnapshotOptions, CreateEnvironmentOptions, EnvExportSummary,
    EnvImportSummary, EnvMeta, EnvSnapshotRemoveSummary, EnvSnapshotRestoreSummary,
    EnvSnapshotSummary, ExecutionSummary, ExportEnvironmentOptions, ImportEnvironmentOptions,
    RemoveEnvSnapshotOptions, RestoreEnvSnapshotOptions, EnvMarker,
};

pub enum ResolvedExecution {
    Launcher {
        env: EnvMeta,
        launcher_name: String,
        command: String,
        run_dir: PathBuf,
    },
    Runtime {
        env: EnvMeta,
        runtime_name: String,
        binary_path: String,
        args: Vec<String>,
        run_dir: PathBuf,
    },
}

pub struct EnvironmentService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

#[derive(Clone, Debug)]
struct PlannedCleanupAction {
    kind: &'static str,
    description: String,
}

impl<'a> EnvironmentService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn create(&self, options: CreateEnvironmentOptions) -> Result<EnvMeta, String> {
        if let Some(runtime_name) = options.default_runtime.as_deref() {
            get_runtime_verified(runtime_name, self.env, self.cwd)?;
        }
        if let Some(launcher_name) = options.default_launcher.as_deref() {
            get_launcher(launcher_name, self.env, self.cwd)?;
        }
        create_environment(options, self.env, self.cwd)
    }

    pub fn clone(&self, options: CloneEnvironmentOptions) -> Result<EnvMeta, String> {
        clone_environment(options, self.env, self.cwd)
    }

    pub fn export(&self, options: ExportEnvironmentOptions) -> Result<EnvExportSummary, String> {
        export_environment(options, self.env, self.cwd)
    }

    pub fn import(&self, options: ImportEnvironmentOptions) -> Result<EnvImportSummary, String> {
        import_environment(options, self.env, self.cwd)
    }

    pub fn create_snapshot(
        &self,
        options: CreateEnvSnapshotOptions,
    ) -> Result<EnvSnapshotSummary, String> {
        let meta = create_env_snapshot(options, self.env, self.cwd)?;
        Ok(summarize_snapshot(&meta))
    }

    pub fn list_snapshots(
        &self,
        env_name: Option<&str>,
    ) -> Result<Vec<EnvSnapshotSummary>, String> {
        let snapshots = match env_name {
            Some(env_name) => list_env_snapshots(env_name, self.env, self.cwd)?,
            None => list_all_env_snapshots(self.env, self.cwd)?,
        };
        Ok(snapshots.iter().map(summarize_snapshot).collect())
    }

    pub fn get_snapshot(
        &self,
        env_name: &str,
        snapshot_id: &str,
    ) -> Result<EnvSnapshotSummary, String> {
        let snapshot = get_env_snapshot(env_name, snapshot_id, self.env, self.cwd)?;
        Ok(summarize_snapshot(&snapshot))
    }

    pub fn restore_snapshot(
        &self,
        options: RestoreEnvSnapshotOptions,
    ) -> Result<EnvSnapshotRestoreSummary, String> {
        restore_env_snapshot(options, self.env, self.cwd)
    }

    pub fn remove_snapshot(
        &self,
        options: RemoveEnvSnapshotOptions,
    ) -> Result<EnvSnapshotRemoveSummary, String> {
        remove_env_snapshot(options, self.env, self.cwd)
    }

    pub fn prune_snapshot_candidates(
        &self,
        env_name: Option<&str>,
        keep: Option<usize>,
        older_than_days: Option<i64>,
    ) -> Result<Vec<EnvSnapshotSummary>, String> {
        let snapshots = match env_name {
            Some(env_name) => list_env_snapshots(env_name, self.env, self.cwd)?,
            None => list_all_env_snapshots(self.env, self.cwd)?,
        };
        let candidates =
            select_snapshot_prune_candidates(&snapshots, keep, older_than_days, now_utc());
        Ok(candidates.iter().map(summarize_snapshot).collect())
    }

    pub fn prune_snapshots(
        &self,
        env_name: Option<&str>,
        keep: Option<usize>,
        older_than_days: Option<i64>,
    ) -> Result<Vec<EnvSnapshotRemoveSummary>, String> {
        let candidates = self.prune_snapshot_candidates(env_name, keep, older_than_days)?;
        let mut removed = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            removed.push(remove_env_snapshot(
                RemoveEnvSnapshotOptions {
                    env_name: candidate.env_name,
                    snapshot_id: candidate.id,
                },
                self.env,
                self.cwd,
            )?);
        }
        Ok(removed)
    }

    pub fn list(&self) -> Result<Vec<EnvMeta>, String> {
        list_environments(self.env, self.cwd)
    }

    pub fn get(&self, name: &str) -> Result<EnvMeta, String> {
        get_environment(name, self.env, self.cwd)
    }

    pub fn touch(&self, name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        meta.last_used_at = Some(now_utc());
        save_environment(meta, self.env, self.cwd)
    }

    pub fn set_launcher(&self, name: &str, launcher_name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if launcher_name.eq_ignore_ascii_case("none") {
            meta.default_launcher = None;
        } else {
            get_launcher(launcher_name, self.env, self.cwd)?;
            meta.default_launcher = Some(launcher_name.to_string());
        }
        save_environment(meta, self.env, self.cwd)
    }

    pub fn set_runtime(&self, name: &str, runtime_name: &str) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        if runtime_name.eq_ignore_ascii_case("none") {
            meta.default_runtime = None;
        } else {
            get_runtime_verified(runtime_name, self.env, self.cwd)?;
            meta.default_runtime = Some(runtime_name.to_string());
        }
        save_environment(meta, self.env, self.cwd)
    }

    pub fn set_protected(&self, name: &str, protected: bool) -> Result<EnvMeta, String> {
        let mut meta = get_environment(name, self.env, self.cwd)?;
        meta.protected = protected;
        save_environment(meta, self.env, self.cwd)
    }

    pub fn remove(&self, name: &str, force: bool) -> Result<EnvMeta, String> {
        remove_environment(name, force, self.env, self.cwd)
    }

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

        if actions
            .iter()
            .any(|action| matches!(action.kind, "clear-missing-runtime" | "clear-missing-launcher"))
        {
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

    pub fn prune_candidates(&self, older_than_days: i64) -> Result<Vec<EnvMeta>, String> {
        let envs = list_environments(self.env, self.cwd)?;
        Ok(select_prune_candidates(&envs, older_than_days))
    }

    pub fn prune(&self, older_than_days: i64) -> Result<Vec<EnvMeta>, String> {
        let candidates = self.prune_candidates(older_than_days)?;
        let mut removed = Vec::with_capacity(candidates.len());
        for meta in candidates {
            removed.push(remove_environment(&meta.name, false, self.env, self.cwd)?);
        }
        Ok(removed)
    }

    pub fn status(&self, name: &str) -> Result<EnvStatusSummary, String> {
        let env = self.get(name)?;
        let mut summary = EnvStatusSummary {
            env_name: env.name.clone(),
            root: env.root.clone(),
            default_runtime: env.default_runtime.clone(),
            default_launcher: env.default_launcher.clone(),
            resolved_kind: None,
            resolved_name: None,
            binary_path: None,
            command: None,
            run_dir: None,
            runtime_source_kind: None,
            runtime_release_version: None,
            runtime_release_channel: None,
            runtime_health: None,
            issue: None,
        };

        match resolve_execution_binding(&env, None, None) {
            Ok(ExecutionBinding::Runtime(runtime_name)) => {
                summary.resolved_kind = Some("runtime".to_string());
                summary.resolved_name = Some(runtime_name.clone());
                match crate::store::get_runtime(&runtime_name, self.env, self.cwd) {
                    Ok(runtime) => {
                        summary.binary_path = Some(runtime.binary_path.clone());
                        summary.run_dir =
                            Some(resolve_runtime_run_dir(self.cwd).display().to_string());
                        summary.runtime_source_kind =
                            Some(runtime.source_kind.as_str().to_string());
                        summary.runtime_release_version = runtime.release_version.clone();
                        summary.runtime_release_channel = runtime.release_channel.clone();
                        match runtime_integrity_issue(&runtime) {
                            None => summary.runtime_health = Some("ok".to_string()),
                            Some(error) => {
                                summary.runtime_health = Some("broken".to_string());
                                summary.issue =
                                    Some(format!("runtime \"{}\" {error}", runtime.name));
                            }
                        }
                    }
                    Err(error) => {
                        summary.runtime_health = Some("missing".to_string());
                        summary.issue = Some(error);
                    }
                }
            }
            Ok(ExecutionBinding::Launcher(launcher_name)) => {
                summary.resolved_kind = Some("launcher".to_string());
                summary.resolved_name = Some(launcher_name.clone());
                match get_launcher(&launcher_name, self.env, self.cwd) {
                    Ok(launcher) => {
                        summary.command = Some(launcher.command.clone());
                        summary.run_dir = Some(
                            resolve_launcher_run_dir(&launcher, self.cwd)
                                .display()
                                .to_string(),
                        );
                    }
                    Err(error) => summary.issue = Some(error),
                }
            }
            Err(error) => summary.issue = Some(error),
        }

        Ok(summary)
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

    pub fn resolve(
        &self,
        name: &str,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        let env = self.get(name)?;
        self.resolve_execution(env, runtime_override, launcher_override, args)
    }

    pub fn resolve_run(
        &self,
        name: &str,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        let env = self.touch(name)?;
        self.resolve_execution(env, runtime_override, launcher_override, args)
    }

    fn resolve_execution(
        &self,
        env: EnvMeta,
        runtime_override: Option<String>,
        launcher_override: Option<String>,
        args: &[String],
    ) -> Result<ResolvedExecution, String> {
        match resolve_execution_binding(&env, runtime_override, launcher_override)? {
            ExecutionBinding::Launcher(launcher_name) => {
                let launcher = get_launcher(&launcher_name, self.env, self.cwd)?;
                Ok(ResolvedExecution::Launcher {
                    launcher_name,
                    command: build_launcher_command(&launcher, args),
                    run_dir: resolve_launcher_run_dir(&launcher, self.cwd),
                    env,
                })
            }
            ExecutionBinding::Runtime(runtime_name) => {
                let runtime = get_runtime_verified(&runtime_name, self.env, self.cwd)?;
                Ok(ResolvedExecution::Runtime {
                    runtime_name,
                    binary_path: runtime.binary_path,
                    args: args.to_vec(),
                    run_dir: resolve_runtime_run_dir(self.cwd),
                    env,
                })
            }
        }
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

impl ResolvedExecution {
    pub fn into_summary(self) -> ExecutionSummary {
        match self {
            Self::Launcher {
                env,
                launcher_name,
                command,
                run_dir,
            } => ExecutionSummary {
                env_name: env.name,
                binding_kind: "launcher".to_string(),
                binding_name: launcher_name,
                command: Some(command),
                binary_path: None,
                forwarded_args: Vec::new(),
                run_dir: run_dir.display().to_string(),
            },
            Self::Runtime {
                env,
                runtime_name,
                binary_path,
                args,
                run_dir,
            } => ExecutionSummary {
                env_name: env.name,
                binding_kind: "runtime".to_string(),
                binding_name: runtime_name,
                command: None,
                binary_path: Some(binary_path),
                forwarded_args: args,
                run_dir: run_dir.display().to_string(),
            },
        }
    }
}
