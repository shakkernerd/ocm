mod support;

use std::fs;

use ocm::manifest::{apply_manifest_runtime_binding, ensure_manifest_env, parse_manifest};

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

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
