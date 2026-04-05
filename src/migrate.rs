use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::env::{CreateEnvironmentOptions, EnvImportSummary, EnvironmentService};
use crate::manifest::{ManifestEnv, OcmManifest};
use crate::store::{
    clear_nonportable_runtime_state, copy_dir_recursive, default_env_root, derive_env_paths,
    display_path, get_environment, resolve_absolute_path, resolve_user_home,
    rewrite_openclaw_config_for_target, validate_name,
};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSourceSummary {
    pub source_home: String,
    pub config_path: String,
    pub workspace_dir: String,
    pub exists: bool,
    pub config_exists: bool,
    pub workspace_exists: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlanSummary {
    pub source: MigrationSourceSummary,
    pub env_name: String,
    pub env_exists: bool,
    pub target_root: String,
}

#[derive(Clone, Debug)]
pub struct MigrateHomeOptions {
    pub source_home: Option<String>,
    pub name: String,
    pub root: Option<String>,
}

pub fn default_migration_source_home(env: &BTreeMap<String, String>) -> PathBuf {
    resolve_user_home(env).join(".openclaw")
}

pub fn resolve_migration_source_home(
    explicit: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> PathBuf {
    explicit
        .map(Path::to_path_buf)
        .unwrap_or_else(|| default_migration_source_home(env))
}

pub fn inspect_migration_source(
    explicit: Option<&Path>,
    env: &BTreeMap<String, String>,
) -> MigrationSourceSummary {
    let source_home = resolve_migration_source_home(explicit, env);
    let config_path = source_home.join("openclaw.json");
    let workspace_dir = source_home.join("workspace");

    MigrationSourceSummary {
        source_home: display_path(&source_home),
        config_path: display_path(&config_path),
        workspace_dir: display_path(&workspace_dir),
        exists: source_home.exists(),
        config_exists: config_path.exists(),
        workspace_exists: workspace_dir.exists(),
    }
}

pub fn plan_migration(
    explicit_source_home: Option<&Path>,
    env_name: &str,
    explicit_root: Option<&str>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<MigrationPlanSummary, String> {
    let env_name = validate_name(env_name, "Environment name")?;
    let target_root = if let Some(root) = explicit_root {
        resolve_absolute_path(root, env, cwd)?
    } else {
        default_env_root(&env_name, env, cwd)?
    };

    Ok(MigrationPlanSummary {
        source: inspect_migration_source(explicit_source_home, env),
        env_exists: get_environment(&env_name, env, cwd).is_ok(),
        env_name,
        target_root: display_path(&target_root),
    })
}

pub fn migrate_plain_openclaw_home(
    options: MigrateHomeOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvImportSummary, String> {
    let service = EnvironmentService::new(env, cwd);
    let source_home =
        resolve_migration_source_home(options.source_home.as_deref().map(Path::new), env);
    if !source_home.exists() {
        return Err(format!(
            "plain OpenClaw home does not exist: {}",
            display_path(&source_home)
        ));
    }

    let created = service.create(CreateEnvironmentOptions {
        name: options.name.clone(),
        root: options.root.clone(),
        gateway_port: None,
        default_runtime: None,
        default_launcher: None,
        protected: false,
    })?;
    let target_paths = derive_env_paths(Path::new(&created.root));

    if target_paths.state_dir.exists() {
        fs::remove_dir_all(&target_paths.state_dir).map_err(|error| error.to_string())?;
    }

    copy_dir_recursive(&source_home, &target_paths.state_dir)?;
    let legacy_root = source_home.parent().unwrap_or(source_home.as_path());
    rewrite_openclaw_config_for_target(&target_paths, Some(legacy_root), created.gateway_port)?;
    clear_nonportable_runtime_state(&target_paths)?;

    Ok(EnvImportSummary {
        name: created.name,
        source_name: "plain-openclaw".to_string(),
        root: created.root,
        archive_path: display_path(&source_home),
        default_runtime: created.default_runtime,
        default_launcher: created.default_launcher,
        protected: created.protected,
    })
}

pub fn manifest_for_migration_env(env_name: &str) -> OcmManifest {
    OcmManifest {
        schema: "ocm/v1".to_string(),
        env: ManifestEnv {
            name: env_name.to_string(),
        },
        runtime: None,
        launcher: None,
        service: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::store::derive_env_paths;

    use super::{
        MigrateHomeOptions, default_migration_source_home, inspect_migration_source,
        manifest_for_migration_env, migrate_plain_openclaw_home, plan_migration,
    };

    #[test]
    fn inspect_migration_source_defaults_to_user_openclaw_home() {
        let root = std::env::temp_dir().join("ocm-migrate-tests-default");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), root.display().to_string());

        let summary = inspect_migration_source(None, &env);
        assert_eq!(
            summary.source_home,
            default_migration_source_home(&env).display().to_string()
        );
        assert!(!summary.exists);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn inspect_migration_source_reports_existing_config_and_workspace() {
        let root = std::env::temp_dir().join("ocm-migrate-tests-explicit");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("workspace")).unwrap();
        fs::write(root.join("openclaw.json"), "{}\n").unwrap();

        let env = BTreeMap::new();
        let summary = inspect_migration_source(Some(Path::new(&root)), &env);

        assert!(summary.exists);
        assert!(summary.config_exists);
        assert!(summary.workspace_exists);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn plan_migration_defaults_to_the_standard_target_root() {
        let root = std::env::temp_dir().join("ocm-migrate-tests-plan");
        let cwd = root.join("cwd");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&cwd).unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), root.join("home").display().to_string());
        env.insert(
            "OCM_HOME".to_string(),
            root.join("ocm-home").display().to_string(),
        );

        let plan = plan_migration(None, "mira", None, &env, &cwd).unwrap();
        assert_eq!(plan.env_name, "mira");
        assert!(plan.target_root.ends_with("/ocm-home/envs/mira"));
        assert!(!plan.env_exists);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn migrate_plain_openclaw_home_copies_config_and_workspace_but_clears_runtime_residue() {
        let root = std::env::temp_dir().join("ocm-migrate-tests-apply");
        let cwd = root.join("cwd");
        let source_home = root.join("legacy-home");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(source_home.join("workspace")).unwrap();
        fs::create_dir_all(source_home.join("logs")).unwrap();
        fs::create_dir_all(source_home.join("agents/main/agent")).unwrap();
        fs::create_dir_all(source_home.join("agents/main/sessions")).unwrap();
        fs::create_dir_all(&cwd).unwrap();
        fs::write(
            source_home.join("openclaw.json"),
            format!(
                "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}\n",
                source_home.join("workspace").display()
            ),
        )
        .unwrap();
        fs::write(source_home.join("workspace/notes.txt"), "hello\n").unwrap();
        fs::write(source_home.join("logs/app.log"), "runtime residue\n").unwrap();
        fs::write(
            source_home.join("agents/main/agent/auth-profiles.json"),
            "{}\n",
        )
        .unwrap();
        fs::write(
            source_home.join("agents/main/sessions/main.jsonl"),
            "stale session\n",
        )
        .unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), root.join("home").display().to_string());
        env.insert(
            "OCM_HOME".to_string(),
            root.join("ocm-home").display().to_string(),
        );

        let summary = migrate_plain_openclaw_home(
            MigrateHomeOptions {
                source_home: Some(source_home.display().to_string()),
                name: "mira".to_string(),
                root: None,
            },
            &env,
            &cwd,
        )
        .unwrap();

        let target_root = PathBuf::from(summary.root);
        let target_paths = derive_env_paths(&target_root);
        assert!(target_paths.config_path.exists());
        assert!(target_paths.workspace_dir.join("notes.txt").exists());
        assert!(
            target_paths
                .state_dir
                .join("agents/main/agent/auth-profiles.json")
                .exists()
        );
        assert!(!target_paths.state_dir.join("logs").exists());
        assert!(
            !target_paths
                .state_dir
                .join("agents/main/sessions/main.jsonl")
                .exists()
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn manifest_for_migration_env_builds_a_minimal_manifest() {
        let manifest = manifest_for_migration_env("mira");

        assert_eq!(manifest.schema, "ocm/v1");
        assert_eq!(manifest.env.name, "mira");
        assert!(manifest.runtime.is_none());
        assert!(manifest.launcher.is_none());
        assert!(manifest.service.is_none());
    }
}
