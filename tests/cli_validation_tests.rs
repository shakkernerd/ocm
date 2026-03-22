mod support;

use std::fs;

use crate::support::{ocm_env, run_ocm, stderr, TestDir};

#[test]
fn env_create_rejects_invalid_names() {
    let root = TestDir::new("cli-invalid-env-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "bad/name"]);
    assert_eq!(create.status.code(), Some(1));
    assert!(stderr(&create).contains("Environment name must use letters, numbers, '.', '_', or '-'"));
}

#[test]
fn version_add_rejects_invalid_names() {
    let root = TestDir::new("cli-invalid-version-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(&cwd, &env, &["version", "add", "bad/name", "--command", "sh"]);
    assert_eq!(add.status.code(), Some(1));
    assert!(stderr(&add).contains("Version name must use letters, numbers, '.', '_', or '-'"));
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
fn env_create_rejects_empty_and_unknown_version_values() {
    let root = TestDir::new("cli-create-version-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let empty = run_ocm(&cwd, &env, &["env", "create", "demo", "--version="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--version requires a value"));

    let missing = run_ocm(&cwd, &env, &["env", "create", "demo", "--version", "missing"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("version \"missing\" does not exist"));
}

#[test]
fn env_run_rejects_empty_and_unknown_version_overrides() {
    let root = TestDir::new("cli-run-version-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let empty = run_ocm(&cwd, &env, &["env", "run", "demo", "--version="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--version requires a value"));

    let missing = run_ocm(&cwd, &env, &["env", "run", "demo", "--version", "missing"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("version \"missing\" does not exist"));
}
