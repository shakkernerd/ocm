use std::path::Path;

use ocm::execution::{
    ExecutionBinding, build_launcher_command, resolve_execution_binding, resolve_launcher_name,
    resolve_launcher_run_dir, resolve_runtime_run_dir,
};
use ocm::types::{EnvMeta, LauncherMeta};
use time::OffsetDateTime;

fn sample_env(default_runtime: Option<&str>, default_launcher: Option<&str>) -> EnvMeta {
    EnvMeta {
        kind: "ocm-env".to_string(),
        name: "demo".to_string(),
        root: "/tmp/demo".to_string(),
        gateway_port: None,
        default_runtime: default_runtime.map(str::to_string),
        default_launcher: default_launcher.map(str::to_string),
        protected: false,
        created_at: OffsetDateTime::UNIX_EPOCH,
        updated_at: OffsetDateTime::UNIX_EPOCH,
        last_used_at: None,
    }
}

fn sample_launcher(cwd: Option<&str>) -> LauncherMeta {
    LauncherMeta {
        kind: "ocm-launcher".to_string(),
        name: "stable".to_string(),
        command: "openclaw".to_string(),
        cwd: cwd.map(str::to_string),
        description: None,
        created_at: OffsetDateTime::UNIX_EPOCH,
        updated_at: OffsetDateTime::UNIX_EPOCH,
    }
}

#[test]
fn resolve_launcher_name_prefers_the_explicit_override() {
    let env = sample_env(None, Some("stable"));

    let resolved = resolve_launcher_name(&env, Some("nightly".to_string())).unwrap();
    assert_eq!(resolved, "nightly");
}

#[test]
fn resolve_launcher_name_falls_back_to_the_environment_default() {
    let env = sample_env(None, Some("stable"));

    let resolved = resolve_launcher_name(&env, None).unwrap();
    assert_eq!(resolved, "stable");
}

#[test]
fn resolve_launcher_name_uses_launcher_wording_when_unbound() {
    let env = sample_env(None, None);

    let error = resolve_launcher_name(&env, None).unwrap_err();
    assert!(error.contains("has no default launcher"));
    assert!(error.contains("env set-launcher"));
    assert!(error.contains("--launcher"));
}

#[test]
fn build_launcher_command_quotes_forwarded_arguments() {
    let launcher = sample_launcher(None);
    let args = vec!["gateway run".to_string(), "it's-live".to_string()];

    let command = build_launcher_command(&launcher, &args);
    assert_eq!(command, "openclaw 'gateway run' 'it'\"'\"'s-live'");
}

#[test]
fn resolve_launcher_run_dir_prefers_the_launcher_cwd() {
    let launcher = sample_launcher(Some("/tmp/launcher"));

    let run_dir = resolve_launcher_run_dir(&launcher, Path::new("/tmp/fallback"));
    assert_eq!(run_dir, Path::new("/tmp/launcher"));
}

#[test]
fn resolve_launcher_run_dir_falls_back_to_the_calling_cwd() {
    let launcher = sample_launcher(None);

    let run_dir = resolve_launcher_run_dir(&launcher, Path::new("/tmp/fallback"));
    assert_eq!(run_dir, Path::new("/tmp/fallback"));
}

#[test]
fn resolve_execution_binding_prefers_runtime_override() {
    let env = sample_env(Some("stable"), Some("launcher"));

    let resolved = resolve_execution_binding(&env, Some("nightly".to_string()), None).unwrap();
    assert!(matches!(resolved, ExecutionBinding::Runtime(name) if name == "nightly"));
}

#[test]
fn resolve_execution_binding_prefers_runtime_default_before_launcher_default() {
    let env = sample_env(Some("stable"), Some("launcher"));

    let resolved = resolve_execution_binding(&env, None, None).unwrap();
    assert!(matches!(resolved, ExecutionBinding::Runtime(name) if name == "stable"));
}

#[test]
fn resolve_execution_binding_falls_back_to_launcher_default() {
    let env = sample_env(None, Some("launcher"));

    let resolved = resolve_execution_binding(&env, None, None).unwrap();
    assert!(matches!(resolved, ExecutionBinding::Launcher(name) if name == "launcher"));
}

#[test]
fn resolve_execution_binding_rejects_conflicting_overrides() {
    let env = sample_env(Some("stable"), Some("launcher"));

    let error = resolve_execution_binding(
        &env,
        Some("stable".to_string()),
        Some("launcher".to_string()),
    )
    .unwrap_err();
    assert!(error.contains("only one of --runtime or --launcher"));
}

#[test]
fn resolve_execution_binding_uses_runtime_and_launcher_wording_when_unbound() {
    let env = sample_env(None, None);

    let error = resolve_execution_binding(&env, None, None).unwrap_err();
    assert!(error.contains("has no default runtime or launcher"));
    assert!(error.contains("env set-runtime"));
    assert!(error.contains("env set-launcher"));
}

#[test]
fn resolve_runtime_run_dir_uses_the_calling_cwd() {
    let run_dir = resolve_runtime_run_dir(Path::new("/tmp/fallback"));
    assert_eq!(run_dir, Path::new("/tmp/fallback"));
}
