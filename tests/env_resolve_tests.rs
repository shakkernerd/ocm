mod support;

use std::fs;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

#[test]
fn env_resolve_reports_the_bound_launcher_without_touching_last_used_at() {
    let root = TestDir::new("env-resolve-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
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
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "fallback"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let before_show = run_ocm(&cwd, &env, &["env", "show", "demo"]);
    assert!(before_show.status.success(), "{}", stderr(&before_show));
    assert!(!stdout(&before_show).contains("lastUsedAt:"));

    let resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo"]);
    assert!(resolve.status.success(), "{}", stderr(&resolve));
    let expected_cwd = fs::canonicalize(&cwd).unwrap();
    let resolved = stdout(&resolve);
    assert!(resolved.contains("envName: demo"));
    assert!(resolved.contains("bindingKind: launcher"));
    assert!(resolved.contains("bindingName: fallback"));
    assert!(resolved.contains("command: printf launcher"));
    assert!(resolved.contains(&format!("runDir: {}", expected_cwd.display())));

    let after_show = run_ocm(&cwd, &env, &["env", "show", "demo"]);
    assert!(after_show.status.success(), "{}", stderr(&after_show));
    assert!(!stdout(&after_show).contains("lastUsedAt:"));
}

#[test]
fn env_resolve_reports_the_bound_runtime_and_forwarded_args() {
    let root = TestDir::new("env-resolve-runtime");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let runtime_path = bin_dir.join("stable");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
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
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let resolve = run_ocm(
        &cwd,
        &env,
        &["env", "resolve", "demo", "--", "onboard", "status"],
    );
    assert!(resolve.status.success(), "{}", stderr(&resolve));
    let expected_binary_path = fs::canonicalize(&runtime_path).unwrap();
    let expected_cwd = fs::canonicalize(&cwd).unwrap();
    let resolved = stdout(&resolve);
    assert!(resolved.contains("bindingKind: runtime"));
    assert!(resolved.contains("bindingName: stable"));
    assert!(resolved.contains(&format!("binaryPath: {}", expected_binary_path.display())));
    assert!(resolved.contains("forwardedArgs: onboard status"));
    assert!(resolved.contains(&format!("runDir: {}", expected_cwd.display())));
}

#[test]
fn env_resolve_json_reports_runtime_resolution_shape() {
    let root = TestDir::new("env-resolve-json");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let runtime_path = bin_dir.join("stable");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
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
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let resolve = run_ocm(
        &cwd,
        &env,
        &["env", "resolve", "demo", "--json", "--", "onboard"],
    );
    assert!(resolve.status.success(), "{}", stderr(&resolve));

    let expected_binary_path = fs::canonicalize(&runtime_path).unwrap();
    let expected_cwd = fs::canonicalize(&cwd).unwrap();
    let resolved: Value = serde_json::from_str(&stdout(&resolve)).unwrap();
    assert_eq!(resolved["envName"], "demo");
    assert_eq!(resolved["bindingKind"], "runtime");
    assert_eq!(resolved["bindingName"], "stable");
    assert_eq!(
        resolved["binaryPath"],
        Value::String(expected_binary_path.display().to_string())
    );
    assert_eq!(
        resolved["forwardedArgs"],
        Value::Array(vec![Value::String("onboard".to_string())])
    );
    assert_eq!(
        resolved["runDir"],
        Value::String(expected_cwd.display().to_string())
    );
}
