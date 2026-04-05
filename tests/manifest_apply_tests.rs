mod support;

use std::fs;

use ocm::manifest::{
    apply_manifest_launcher_binding, apply_manifest_runtime_binding,
    apply_manifest_service_install, ensure_manifest_env, parse_manifest,
};

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

fn install_fake_launchctl(root: &TestDir, env: &mut std::collections::BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let script = "#!/bin/sh\nexit 0\n";
    write_executable_script(&bin_dir.join("launchctl"), script);

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        bin_dir.display().to_string()
    } else {
        format!("{}:{existing_path}", bin_dir.display())
    };
    env.insert("PATH".to_string(), combined_path);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
}

#[test]
fn ensure_manifest_env_creates_a_missing_environment() {
    let root = TestDir::new("manifest-apply-create");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let manifest = parse_manifest("schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

    let summary = ensure_manifest_env(&manifest, &env, &cwd).unwrap();
    assert!(summary.created);
    assert_eq!(summary.env.name, "mira");

    let show = run_ocm(&cwd, &env, &["env", "show", "mira", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
}

#[test]
fn ensure_manifest_env_reuses_an_existing_environment() {
    let root = TestDir::new("manifest-apply-reuse");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let manifest = parse_manifest("schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

    let summary = ensure_manifest_env(&manifest, &env, &cwd).unwrap();
    assert!(!summary.created);
    assert_eq!(summary.env.name, "mira");
}

#[test]
fn apply_manifest_runtime_binding_sets_a_registered_runtime() {
    let root = TestDir::new("manifest-apply-runtime");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(&bin_dir.join("stable"), "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add_runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add_runtime.status.success(), "{}", stderr(&add_runtime));

    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let manifest =
        parse_manifest("schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  name: stable\n").unwrap();
    let current = ensure_manifest_env(&manifest, &env, &cwd).unwrap().env;

    let summary = apply_manifest_runtime_binding(&manifest, &current, &env, &cwd).unwrap();
    assert!(summary.changed);
    assert_eq!(summary.desired_runtime.as_deref(), Some("stable"));
    assert_eq!(summary.env.default_runtime.as_deref(), Some("stable"));
    assert_eq!(summary.env.default_launcher, None);

    let show = run_ocm(&cwd, &env, &["env", "show", "mira"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultRuntime: stable"));
}

#[test]
fn apply_manifest_runtime_binding_reuses_a_matching_runtime_binding() {
    let root = TestDir::new("manifest-apply-runtime-reuse");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(&bin_dir.join("stable"), "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add_runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add_runtime.status.success(), "{}", stderr(&add_runtime));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "mira", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let manifest =
        parse_manifest("schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  name: stable\n").unwrap();
    let current = ensure_manifest_env(&manifest, &env, &cwd).unwrap().env;

    let summary = apply_manifest_runtime_binding(&manifest, &current, &env, &cwd).unwrap();
    assert!(!summary.changed);
    assert_eq!(summary.desired_runtime.as_deref(), Some("stable"));
    assert_eq!(summary.env.default_runtime.as_deref(), Some("stable"));
}

#[test]
fn apply_manifest_launcher_binding_sets_a_registered_launcher() {
    let root = TestDir::new("manifest-apply-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let manifest =
        parse_manifest("schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n").unwrap();
    let current = ensure_manifest_env(&manifest, &env, &cwd).unwrap().env;

    let summary = apply_manifest_launcher_binding(&manifest, &current, &env, &cwd).unwrap();
    assert!(summary.changed);
    assert_eq!(summary.desired_launcher.as_deref(), Some("dev"));
    assert_eq!(summary.env.default_launcher.as_deref(), Some("dev"));
    assert_eq!(summary.env.default_runtime, None);

    let show = run_ocm(&cwd, &env, &["env", "show", "mira"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultLauncher: dev"));
}

#[test]
fn apply_manifest_launcher_binding_reuses_a_matching_launcher_binding() {
    let root = TestDir::new("manifest-apply-launcher-reuse");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(&cwd, &env, &["env", "create", "mira", "--launcher", "dev"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let manifest =
        parse_manifest("schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n").unwrap();
    let current = ensure_manifest_env(&manifest, &env, &cwd).unwrap().env;

    let summary = apply_manifest_launcher_binding(&manifest, &current, &env, &cwd).unwrap();
    assert!(!summary.changed);
    assert_eq!(summary.desired_launcher.as_deref(), Some("dev"));
    assert_eq!(summary.env.default_launcher.as_deref(), Some("dev"));
}

#[test]
fn apply_manifest_service_install_installs_a_missing_service() {
    let root = TestDir::new("manifest-apply-service");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(&cwd, &env, &["env", "create", "mira", "--launcher", "dev"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let manifest = parse_manifest(
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();
    let current = ensure_manifest_env(&manifest, &env, &cwd).unwrap().env;

    let summary = apply_manifest_service_install(&manifest, &current, &env, &cwd).unwrap();
    assert!(summary.changed);
    assert_eq!(summary.desired_service_install, Some(true));
    assert!(summary.service.installed);
}

#[test]
fn apply_manifest_service_install_reuses_an_installed_service() {
    let root = TestDir::new("manifest-apply-service-reuse");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(&cwd, &env, &["env", "create", "mira", "--launcher", "dev"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let install = run_ocm(&cwd, &env, &["service", "install", "mira"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let manifest = parse_manifest(
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();
    let current = ensure_manifest_env(&manifest, &env, &cwd).unwrap().env;

    let summary = apply_manifest_service_install(&manifest, &current, &env, &cwd).unwrap();
    assert!(!summary.changed);
    assert_eq!(summary.desired_service_install, Some(true));
    assert!(summary.service.installed);
}
