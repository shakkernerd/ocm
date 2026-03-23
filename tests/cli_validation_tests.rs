mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr};

#[test]
fn env_create_rejects_invalid_names() {
    let root = TestDir::new("cli-invalid-env-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "bad/name"]);
    assert_eq!(create.status.code(), Some(1));
    assert!(
        stderr(&create).contains("Environment name must use letters, numbers, '.', '_', or '-'")
    );
}

#[test]
fn launcher_add_rejects_invalid_names() {
    let root = TestDir::new("cli-invalid-launcher-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "bad/name", "--command", "sh"],
    );
    assert_eq!(add.status.code(), Some(1));
    assert!(stderr(&add).contains("Launcher name must use letters, numbers, '.', '_', or '-'"));
}

#[test]
fn env_create_rejects_invalid_and_empty_port_values() {
    let root = TestDir::new("cli-invalid-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let zero = run_ocm(&cwd, &env, &["env", "create", "demo", "--port", "0"]);
    assert_eq!(zero.status.code(), Some(1));
    assert!(stderr(&zero).contains("--port must be a positive integer"));

    let empty = run_ocm(&cwd, &env, &["env", "create", "demo", "--port="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--port requires a value"));
}

#[test]
fn env_create_rejects_empty_and_unknown_launcher_values() {
    let root = TestDir::new("cli-create-launcher-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let empty = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--launcher requires a value"));

    let missing = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "missing"],
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("launcher \"missing\" does not exist"));
}

#[test]
fn env_run_rejects_empty_and_unknown_launcher_overrides() {
    let root = TestDir::new("cli-run-launcher-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let empty = run_ocm(&cwd, &env, &["env", "run", "demo", "--launcher="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--launcher requires a value"));

    let missing = run_ocm(&cwd, &env, &["env", "run", "demo", "--launcher", "missing"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("launcher \"missing\" does not exist"));
}
