mod support;

use std::fs;

use ocm::infra::download::file_sha256;
use serde_json::Value;

use crate::support::{
    TestDir, TestHttpServer, ocm_env, run_ocm, stderr, stdout, write_executable_script,
};

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
    assert!(output.contains("gatewayPort: 18789"));
    assert!(output.contains("gatewayPortSource: computed"));
    assert!(output.contains("resolvedKind: launcher"));
    assert!(output.contains("resolvedName: fallback"));
    assert!(output.contains("command: printf launcher"));
    assert!(output.contains("managedServiceState: absent"));
    assert!(output.contains("openclawState: stopped"));
    assert!(output.contains("globalServiceState: absent"));
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
    assert!(output.contains("gatewayPort: 18789"));
    assert!(output.contains("gatewayPortSource: computed"));
    assert!(output.contains("managedServiceState: absent"));
    assert!(output.contains("openclawState: stopped"));
    assert!(output.contains("globalServiceState: absent"));
    assert!(output.contains("issue: environment \"demo\" has no default runtime or launcher"));
}

#[test]
fn env_status_json_reports_runtime_health_and_binding_shape() {
    let root = TestDir::new("env-status-json-runtime");
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

    let status = run_ocm(&cwd, &env, &["env", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let value: Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(value["envName"], "demo");
    assert_eq!(value["gatewayPort"], 18789);
    assert_eq!(value["gatewayPortSource"], "computed");
    assert_eq!(value["resolvedKind"], "runtime");
    assert_eq!(value["resolvedName"], "stable");
    assert_eq!(value["runtimeHealth"], "ok");
    assert_eq!(value["runtimeSourceKind"], "registered");
    assert_eq!(value["managedServiceState"], "absent");
    assert_eq!(value["openclawState"], "stopped");
    assert_eq!(value["globalServiceState"], "absent");
    assert!(value["issue"].is_null());
}

#[test]
fn env_status_reports_release_backed_runtime_details() {
    let root = TestDir::new("env-status-release-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let artifact_body = b"release-runtime";
    let digest_path = root.child("sha256/openclaw-stable");
    fs::create_dir_all(digest_path.parent().unwrap()).unwrap();
    fs::write(&digest_path, artifact_body).unwrap();
    let sha256 = file_sha256(&digest_path).unwrap();

    let artifact_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-stable",
        "application/octet-stream",
        artifact_body,
    );
    let manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        artifact_server.url(),
        sha256
    );
    let manifest_server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        manifest_body.as_bytes(),
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            &manifest_server.url(),
            "--channel",
            "stable",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let status = run_ocm(&cwd, &env, &["env", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let value: Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(value["resolvedKind"], "runtime");
    assert_eq!(value["runtimeSourceKind"], "installed");
    assert_eq!(value["runtimeReleaseVersion"], "0.2.0");
    assert_eq!(value["runtimeReleaseChannel"], "stable");
    assert_eq!(value["runtimeHealth"], "ok");
    assert_eq!(value["gatewayPort"], 18789);
    assert_eq!(value["gatewayPortSource"], "computed");
}

#[test]
fn env_status_reports_the_config_derived_gateway_port_after_onboarding_writes_it() {
    let root = TestDir::new("env-status-config-port");
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

    let env_root = root.child("ocm-home/envs/demo");
    let config_path = env_root.join(".openclaw/openclaw.json");
    fs::write(&config_path, "{\"gateway\":{\"port\":18888}}").unwrap();

    let status = run_ocm(&cwd, &env, &["env", "status", "demo"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let output = stdout(&status);
    assert!(output.contains("gatewayPort: 18888"));
    assert!(output.contains("gatewayPortSource: config"));
}
