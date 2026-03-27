mod support;

use std::fs;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

#[test]
fn env_doctor_reports_a_healthy_launcher_bound_environment() {
    let root = TestDir::new("env-doctor-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "shell", "--command", "printf launcher"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "shell"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let doctor = run_ocm(&cwd, &env, &["env", "doctor", "demo"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let output = stdout(&doctor);
    assert!(output.contains("healthy: true"));
    assert!(output.contains("rootStatus: ok"));
    assert!(output.contains("markerStatus: ok"));
    assert!(output.contains("launcherStatus: ok"));
    assert!(output.contains("resolutionStatus: ok"));
    assert!(output.contains("resolvedKind: launcher"));
    assert!(output.contains("resolvedName: shell"));
}

#[test]
fn env_doctor_json_reports_marker_and_runtime_health_issues() {
    let root = TestDir::new("env-doctor-runtime-issues");
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

    fs::write(
        root.child("ocm-home/envs/demo/.ocm-env.json"),
        "{\n  \"kind\": \"ocm-env-marker\",\n  \"name\": \"other\",\n  \"createdAt\": \"2026-03-25T00:00:00Z\"\n}\n",
    )
    .unwrap();
    fs::remove_file(&runtime_path).unwrap();

    let doctor = run_ocm(&cwd, &env, &["env", "doctor", "demo", "--json"]);
    assert!(doctor.status.success(), "{}", stderr(&doctor));
    let value: Value = serde_json::from_str(&stdout(&doctor)).unwrap();
    assert_eq!(value["healthy"], false);
    assert_eq!(value["rootStatus"], "ok");
    assert_eq!(value["markerStatus"], "mismatch");
    assert_eq!(value["runtimeStatus"], "broken");
    assert_eq!(value["launcherStatus"], "unbound");
    assert_eq!(value["resolutionStatus"], "error");
    assert_eq!(value["resolvedKind"], "runtime");
    assert_eq!(value["resolvedName"], "stable");
    let issues = value["issues"].as_array().unwrap();
    assert_eq!(issues.len(), 2);
    assert!(
        issues[0]
            .as_str()
            .unwrap()
            .contains("environment marker name mismatch")
    );
    assert!(
        issues[1]
            .as_str()
            .unwrap()
            .contains("runtime \"stable\" binary path does not exist")
    );
}
