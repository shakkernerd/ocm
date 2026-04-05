mod support;

use std::fs;

use ocm::manifest::{
    ManifestReconcileOptions, parse_manifest, reconcile_manifest, reconcile_manifest_with_options,
};

use crate::support::{TestDir, ocm_env, run_ocm, stderr, write_executable_script};

fn install_fake_launchctl(root: &TestDir, env: &mut std::collections::BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(&bin_dir.join("launchctl"), "#!/bin/sh\nexit 0\n");

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
fn reconcile_manifest_creates_binds_launcher_and_installs_service() {
    let root = TestDir::new("manifest-reconcile-launcher");
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

    let manifest_path = cwd.join("ocm.yaml");
    let manifest = parse_manifest(
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();

    let summary = reconcile_manifest(&manifest_path, &manifest, &env, &cwd).unwrap();
    assert!(summary.env_created);
    assert!(!summary.runtime_changed);
    assert!(summary.launcher_changed);
    assert!(summary.service_changed);
    assert_eq!(summary.desired_launcher.as_deref(), Some("dev"));
    assert_eq!(summary.desired_service_install, Some(true));
    assert!(summary.service_installed);
}

#[test]
fn reconcile_manifest_reuses_existing_runtime_binding() {
    let root = TestDir::new("manifest-reconcile-runtime");
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

    let manifest_path = cwd.join("ocm.yaml");
    let manifest =
        parse_manifest("schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  name: stable\n").unwrap();

    let summary = reconcile_manifest(&manifest_path, &manifest, &env, &cwd).unwrap();
    assert!(!summary.env_created);
    assert!(!summary.runtime_changed);
    assert!(!summary.launcher_changed);
    assert_eq!(summary.desired_runtime.as_deref(), Some("stable"));
}

#[test]
fn reconcile_manifest_can_snapshot_an_existing_env_before_apply() {
    let root = TestDir::new("manifest-reconcile-snapshot");
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

    let manifest_path = cwd.join("ocm.yaml");
    let manifest =
        parse_manifest("schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  name: stable\n").unwrap();

    let summary = reconcile_manifest_with_options(
        &manifest_path,
        &manifest,
        &env,
        &cwd,
        ManifestReconcileOptions {
            snapshot_existing_env: true,
            rollback_on_failure: false,
        },
    )
    .unwrap();

    let snapshot_id = summary.snapshot_id.as_deref().unwrap();
    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "mira", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(crate::support::stdout(&list).contains(snapshot_id));
}
