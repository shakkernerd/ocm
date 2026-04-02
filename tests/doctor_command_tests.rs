mod support;

use std::fs;

use serde_json::Value;

use crate::support::{
    TestDir, install_fake_node_and_npm, ocm_env, path_string, run_ocm, stderr, stdout,
};

#[test]
fn doctor_host_reports_missing_required_tools() {
    let root = TestDir::new("doctor-host-missing-required");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    let empty_path = root.child("empty-path");
    fs::create_dir_all(&empty_path).unwrap();
    env.insert("PATH".to_string(), path_string(&empty_path));

    let doctor = run_ocm(&cwd, &env, &["doctor", "host", "--json"]);
    assert_eq!(doctor.status.code(), Some(1), "{}", stderr(&doctor));

    let value: Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(value["healthy"], Value::Bool(false));
    assert_eq!(value["officialReleaseReady"], Value::Bool(false));
    assert_eq!(value["requiredIssues"], Value::from(2));

    let checks = value["checks"].as_array().unwrap();
    assert!(checks.iter().any(|check| {
        check["name"] == "Node.js" && check["level"] == "required" && check["status"] == "missing"
    }));
    assert!(checks.iter().any(|check| {
        check["name"] == "npm" && check["level"] == "required" && check["status"] == "missing"
    }));
}

#[test]
fn doctor_host_reports_ready_official_release_requirements() {
    let root = TestDir::new("doctor-host-ready");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.14.0");

    let doctor = run_ocm(&cwd, &env, &["doctor", "host", "--json"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));

    let value: Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(value["healthy"], Value::Bool(true));
    assert_eq!(value["officialReleaseReady"], Value::Bool(true));
    assert_eq!(value["requiredIssues"], Value::from(0));

    let checks = value["checks"].as_array().unwrap();
    assert!(checks.iter().any(|check| {
        check["name"] == "Node.js" && check["status"] == "ok" && check["available"] == true
    }));
    assert!(checks.iter().any(|check| {
        check["name"] == "npm" && check["status"] == "ok" && check["available"] == true
    }));
}
