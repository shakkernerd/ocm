mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, write_executable_script};

#[test]
fn runtime_install_stays_quiet_when_stderr_is_captured() {
    let root = TestDir::new("progress-runtime-install");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let runtime_path = bin_dir.join("stable");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "stable", "--path", "./bin/stable"],
    );
    assert!(install.status.success(), "{}", stderr(&install));
    assert_eq!(stderr(&install), "");
}

#[test]
fn env_snapshot_create_stays_quiet_when_stderr_is_captured() {
    let root = TestDir::new("progress-env-snapshot");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "demo"]);
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));
    assert_eq!(stderr(&snapshot), "");
}
