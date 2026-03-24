mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

#[test]
fn env_status_reports_the_resolved_launcher() {
    let root = TestDir::new("env-status-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "fallback",
            "--command",
            "printf launcher",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "fallback"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let status = run_ocm(&cwd, &env, &["env", "status", "demo"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let output = stdout(&status);
    assert!(output.contains("resolvedKind: launcher"));
    assert!(output.contains("resolvedName: fallback"));
    assert!(output.contains("command: printf launcher"));
}

#[test]
fn env_status_reports_a_broken_runtime_without_failing() {
    let root = TestDir::new("env-status-broken-runtime");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let runtime_path = bin_dir.join("stable");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(&runtime_path).unwrap();

    let status = run_ocm(&cwd, &env, &["env", "status", "demo"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let output = stdout(&status);
    assert!(output.contains("resolvedKind: runtime"));
    assert!(output.contains("resolvedName: stable"));
    assert!(output.contains("runtimeHealth: broken"));
    assert!(output.contains("issue: runtime \"stable\" binary path does not exist:"));
}

#[test]
fn env_status_reports_when_an_environment_has_no_binding() {
    let root = TestDir::new("env-status-unbound");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let status = run_ocm(&cwd, &env, &["env", "status", "demo"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let output = stdout(&status);
    assert!(output.contains("envName: demo"));
    assert!(output.contains("issue: environment \"demo\" has no default runtime or launcher"));
}
