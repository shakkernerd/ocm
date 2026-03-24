mod support;

use std::fs;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

#[test]
fn runtime_verify_reports_a_healthy_runtime() {
    let root = TestDir::new("runtime-verify-healthy");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(&bin_dir.join("stable"), "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let verify = run_ocm(&cwd, &env, &["runtime", "verify", "stable"]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    let output = stdout(&verify);
    assert!(output.contains("name: stable"));
    assert!(output.contains("healthy: true"));
    assert!(output.contains("sourceKind: registered"));
}

#[test]
fn runtime_verify_uses_exit_code_one_for_broken_runtimes() {
    let root = TestDir::new("runtime-verify-broken");
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
    fs::remove_file(&runtime_path).unwrap();

    let verify = run_ocm(&cwd, &env, &["runtime", "verify", "stable", "--json"]);
    assert_eq!(verify.status.code(), Some(1));
    let value: Value = serde_json::from_str(&stdout(&verify)).unwrap();
    assert_eq!(value["name"], "stable");
    assert_eq!(value["healthy"], false);
    assert!(
        value["issue"]
            .as_str()
            .unwrap()
            .contains("binary path does not exist:")
    );
}

#[test]
fn runtime_verify_all_reports_mixed_runtime_health() {
    let root = TestDir::new("runtime-verify-all");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let stable_path = bin_dir.join("stable");
    let broken_path = bin_dir.join("broken");
    write_executable_script(&stable_path, "#!/bin/sh\nexit 0\n");
    write_executable_script(&broken_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add_stable = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add_stable.status.success(), "{}", stderr(&add_stable));
    let add_broken = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "broken", "--path", "./bin/broken"],
    );
    assert!(add_broken.status.success(), "{}", stderr(&add_broken));
    fs::remove_file(&broken_path).unwrap();

    let verify = run_ocm(&cwd, &env, &["runtime", "verify", "--all"]);
    assert_eq!(verify.status.code(), Some(1));
    let output = stdout(&verify);
    assert!(output.contains("stable"));
    assert!(output.contains("healthy=true"));
    assert!(output.contains("broken"));
    assert!(output.contains("healthy=false"));
    assert!(output.contains("issue=binary path does not exist:"));

    let verify_json = run_ocm(&cwd, &env, &["runtime", "verify", "--all", "--json"]);
    assert_eq!(verify_json.status.code(), Some(1));
    let value: Value = serde_json::from_str(&stdout(&verify_json)).unwrap();
    let array = value.as_array().unwrap();
    assert_eq!(array.len(), 2);
    assert!(array.iter().any(|item| {
        item["name"] == "stable" && item["healthy"] == true && item["sourceKind"] == "registered"
    }));
    assert!(array.iter().any(|item| {
        item["name"] == "broken"
            && item["healthy"] == false
            && item["issue"]
                .as_str()
                .unwrap()
                .contains("binary path does not exist:")
    }));
}
