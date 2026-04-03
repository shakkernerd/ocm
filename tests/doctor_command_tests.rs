mod support;

use std::fs;

use serde_json::Value;

use crate::support::{
    TestDir, install_fake_git_package_manager, install_fake_node_and_npm, ocm_env, path_string,
    run_ocm, stderr, stdout,
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
    assert!(doctor.status.success(), "{}", stderr(&doctor));

    let value: Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(value["healthy"], Value::Bool(true));
    assert_eq!(value["officialReleaseReady"], Value::Bool(true));
    assert_eq!(value["requiredIssues"], Value::from(0));

    let checks = value["checks"].as_array().unwrap();
    let expected_recommended_gaps = checks
        .iter()
        .filter(|check| {
            check["level"] == "recommended"
                && check["status"] != "ok"
                && check["status"] != "unsupported"
        })
        .count() as u64;
    assert_eq!(
        value["recommendedGaps"],
        Value::from(expected_recommended_gaps)
    );
    assert!(checks.iter().any(|check| {
        check["name"] == "Node.js"
            && check["level"] == "recommended"
            && check["status"] == "missing"
            && check["detail"]
                .as_str()
                .unwrap_or("")
                .contains("private Node.js toolchain")
    }));
    assert!(checks.iter().any(|check| {
        check["name"] == "npm"
            && check["level"] == "recommended"
            && check["status"] == "missing"
            && check["detail"]
                .as_str()
                .unwrap_or("")
                .contains("private Node.js + npm toolchain")
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
        check["name"] == "Node.js"
            && check["level"] == "recommended"
            && check["status"] == "ok"
            && check["available"] == true
    }));
    assert!(checks.iter().any(|check| {
        check["name"] == "npm"
            && check["level"] == "recommended"
            && check["status"] == "ok"
            && check["available"] == true
    }));
}

#[test]
fn doctor_host_can_fix_git_with_a_supported_package_manager() {
    let root = TestDir::new("doctor-host-fix-git");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    let empty_path = root.child("empty-path");
    fs::create_dir_all(&empty_path).unwrap();
    env.insert("PATH".to_string(), path_string(&empty_path));
    env.insert(
        "OCM_INTERNAL_HOST_PLATFORM".to_string(),
        "linux".to_string(),
    );
    env.insert(
        "OCM_INTERNAL_HOST_PACKAGE_MANAGER".to_string(),
        "apt-get".to_string(),
    );
    env.insert("OCM_INTERNAL_HOST_IS_ROOT".to_string(), "false".to_string());
    let log_path = install_fake_git_package_manager(&root, &mut env, "apt-get");

    let fix = run_ocm(
        &cwd,
        &env,
        &["doctor", "host", "--fix", "git", "--yes", "--json"],
    );
    assert!(fix.status.success(), "{}", stderr(&fix));

    let value: Value = serde_json::from_str(&stdout(&fix)).unwrap();
    assert_eq!(value["tool"], "git");
    assert_eq!(value["ready"], Value::Bool(true));
    assert_eq!(value["changed"], Value::Bool(true));
    assert_eq!(value["manager"], "apt-get");
    assert!(
        value["version"]
            .as_str()
            .unwrap_or("")
            .contains("git version 2.51.0")
    );

    let log = fs::read_to_string(log_path).unwrap();
    assert!(log.contains("update"));
    assert!(log.contains("install -y git"));

    let doctor = run_ocm(&cwd, &env, &["doctor", "host", "--json"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let doctor_json: Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    let checks = doctor_json["checks"].as_array().unwrap();
    assert!(checks.iter().any(|check| {
        check["name"] == "git" && check["status"] == "ok" && check["available"] == true
    }));
}

#[test]
fn doctor_host_fix_requires_yes() {
    let root = TestDir::new("doctor-host-fix-yes");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let fix = run_ocm(&cwd, &env, &["doctor", "host", "--fix", "git"]);
    assert_eq!(fix.status.code(), Some(1));
    assert!(stderr(&fix).contains("doctor host --fix git requires --yes"));
}
