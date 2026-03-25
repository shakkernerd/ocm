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
fn runtime_add_rejects_invalid_names() {
    let root = TestDir::new("cli-invalid-runtime-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "bad/name", "--path", "./bin/openclaw"],
    );
    assert_eq!(add.status.code(), Some(1));
    assert!(stderr(&add).contains("Runtime name must use letters, numbers, '.', '_', or '-'"));
}

#[test]
fn runtime_install_rejects_invalid_names() {
    let root = TestDir::new("cli-invalid-installed-runtime-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "bad/name", "--path", "./bin/openclaw"],
    );
    assert_eq!(install.status.code(), Some(1));
    assert!(stderr(&install).contains("Runtime name must use letters, numbers, '.', '_', or '-'"));
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

#[test]
fn env_create_rejects_empty_and_unknown_runtime_values() {
    let root = TestDir::new("cli-create-runtime-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let empty = run_ocm(&cwd, &env, &["env", "create", "demo", "--runtime="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--runtime requires a value"));

    let missing = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "missing"],
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("runtime \"missing\" does not exist"));
}

#[test]
fn env_run_rejects_conflicting_runtime_and_launcher_overrides() {
    let root = TestDir::new("cli-run-conflicting-overrides");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("stable"), "#!/bin/sh\n").unwrap();
    let env = ocm_env(&root);

    let add_runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add_runtime.status.success(), "{}", stderr(&add_runtime));

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "sh"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let run = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "run",
            "demo",
            "--runtime",
            "stable",
            "--launcher",
            "stable",
            "--",
            "onboard",
        ],
    );
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("env run accepts only one of --runtime or --launcher"));
}

#[test]
fn env_run_rejects_empty_and_unknown_runtime_overrides() {
    let root = TestDir::new("cli-run-runtime-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let empty = run_ocm(&cwd, &env, &["env", "run", "demo", "--runtime="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--runtime requires a value"));

    let missing = run_ocm(&cwd, &env, &["env", "run", "demo", "--runtime", "missing"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("runtime \"missing\" does not exist"));
}

#[test]
fn runtime_add_rejects_empty_and_missing_paths() {
    let root = TestDir::new("cli-runtime-path-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let empty = run_ocm(&cwd, &env, &["runtime", "add", "stable", "--path="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--path requires a value"));

    let missing = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./missing"],
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("runtime path does not exist:"));
}

#[test]
fn runtime_install_rejects_empty_and_missing_paths() {
    let root = TestDir::new("cli-runtime-install-path-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let empty = run_ocm(&cwd, &env, &["runtime", "install", "stable", "--path="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--path requires a value"));

    let missing = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "stable", "--path", "./missing"],
    );
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("runtime path does not exist:"));
}

#[test]
fn runtime_install_rejects_empty_and_conflicting_urls() {
    let root = TestDir::new("cli-runtime-install-url-validation");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("stable"), "#!/bin/sh\n").unwrap();
    let env = ocm_env(&root);

    let empty = run_ocm(&cwd, &env, &["runtime", "install", "stable", "--url="]);
    assert_eq!(empty.status.code(), Some(1));
    assert!(stderr(&empty).contains("--url requires a value"));

    let missing = run_ocm(&cwd, &env, &["runtime", "install", "stable"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(stderr(&missing).contains("runtime install requires --path, --url, or --manifest-url"));

    let conflicting = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--path",
            "./bin/stable",
            "--url",
            "http://127.0.0.1/stable",
        ],
    );
    assert_eq!(conflicting.status.code(), Some(1));
    assert!(
        stderr(&conflicting)
            .contains("runtime install accepts only one of --path, --url, or --manifest-url")
    );
}

#[test]
fn runtime_install_manifest_requires_a_selector_and_rejects_conflicting_sources() {
    let root = TestDir::new("cli-runtime-install-manifest-validation");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("stable"), "#!/bin/sh\n").unwrap();
    let env = ocm_env(&root);

    let missing_selector = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            "https://example.test/releases.json",
        ],
    );
    assert!(
        stderr(&missing_selector)
            .contains("runtime install with --manifest-url requires --version or --channel")
    );
    assert_eq!(missing_selector.status.code(), Some(1));

    let conflicting_selectors = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            "https://example.test/releases.json",
            "--version",
            "0.2.0",
            "--channel",
            "stable",
        ],
    );
    assert_eq!(conflicting_selectors.status.code(), Some(1));
    assert!(stderr(&conflicting_selectors).contains(
        "runtime install with --manifest-url accepts only one of --version or --channel"
    ));

    let conflicting = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--path",
            "./bin/stable",
            "--manifest-url",
            "https://example.test/releases.json",
            "--version",
            "0.2.0",
        ],
    );
    assert_eq!(conflicting.status.code(), Some(1));
    assert!(
        stderr(&conflicting)
            .contains("runtime install accepts only one of --path, --url, or --manifest-url")
    );
}
