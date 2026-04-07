mod support;

use std::fs;

use ocm::manifest::{
    ManifestReconcileOptions, parse_manifest, reconcile_manifest, reconcile_manifest_with_options,
};
use ocm::store::get_environment;

use crate::support::{
    TestDir, install_fake_service_manager, ocm_env, run_ocm, stderr, write_executable_script,
};

#[test]
fn reconcile_manifest_creates_binds_launcher_and_installs_service() {
    let root = TestDir::new("manifest-reconcile-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_service_manager(&root, &mut env);

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

#[test]
fn reconcile_manifest_rolls_back_partial_changes_when_later_steps_fail() {
    let root = TestDir::new("manifest-reconcile-rollback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let fake_home = root.child("fake-home-file");
    fs::write(&fake_home, "not-a-directory").unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let mut failing_env = env.clone();
    failing_env.insert("HOME".to_string(), fake_home.display().to_string());
    failing_env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );

    let manifest_path = cwd.join("ocm.yaml");
    let manifest = parse_manifest(
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();

    let error = reconcile_manifest_with_options(
        &manifest_path,
        &manifest,
        &failing_env,
        &cwd,
        ManifestReconcileOptions {
            snapshot_existing_env: true,
            rollback_on_failure: true,
        },
    )
    .unwrap_err();

    assert!(error.contains("rolled back env \"mira\" from snapshot"));

    let restored = get_environment("mira", &env, &cwd).unwrap();
    assert_eq!(restored.default_launcher, None);

    let status = run_ocm(&cwd, &env, &["service", "status", "mira", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    assert!(crate::support::stdout(&status).contains("\"installed\": false"));
}

#[test]
fn reconcile_manifest_clears_existing_bindings_when_manifest_requests_none() {
    let root = TestDir::new("manifest-reconcile-clear-bindings");
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
    let manifest = parse_manifest("schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

    let summary = reconcile_manifest(&manifest_path, &manifest, &env, &cwd).unwrap();
    assert!(!summary.env_created);
    assert!(summary.runtime_changed);
    assert!(!summary.launcher_changed);
    assert_eq!(summary.desired_runtime, None);
    assert_eq!(summary.desired_launcher, None);

    let restored = get_environment("mira", &env, &cwd).unwrap();
    assert_eq!(restored.default_runtime, None);
    assert_eq!(restored.default_launcher, None);
}
