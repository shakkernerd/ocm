mod support;

use std::fs;

use serde_json::Value;

use crate::support::{
    TestDir, ocm_env, path_string, run_ocm, stderr, stdout, write_executable_script,
};

#[test]
fn service_list_reports_launcher_and_runtime_bindings_in_json() {
    let root = TestDir::new("service-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let runtime_path = root.child("bin/openclaw");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "managed",
            "--path",
            &path_string(&runtime_path),
        ],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let demo = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(demo.status.success(), "{}", stderr(&demo));

    let prod = run_ocm(&cwd, &env, &["env", "create", "prod", "--runtime", "managed"]);
    assert!(prod.status.success(), "{}", stderr(&prod));

    let output = run_ocm(&cwd, &env, &["service", "list", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["globalLabel"], "ai.openclaw.gateway");
    assert_eq!(summary["globalInstalled"], false);
    assert_eq!(summary["globalLoaded"], false);
    assert_eq!(summary["globalRunning"], false);

    let services = summary["services"].as_array().unwrap();
    assert_eq!(services.len(), 2);

    let demo = services
        .iter()
        .find(|service| service["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingKind"], "launcher");
    assert_eq!(demo["bindingName"], "stable");
    assert_eq!(demo["gatewayPort"], 18789);
    assert_eq!(demo["managedLabel"], "ai.openclaw.gateway.ocm.demo");
    assert_eq!(demo["managedPlistPath"], format!(
        "{}/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist",
        root.child("home").display()
    ));
    assert_eq!(demo["runDir"], path_string(&root.child("ocm-home/envs/demo")));
    assert!(
        demo["command"]
            .as_str()
            .unwrap()
            .contains("'gateway' 'run' '--port' '18789'")
    );
    assert_eq!(demo["installed"], false);
    assert_eq!(demo["globalMatchesEnv"], false);

    let prod = services
        .iter()
        .find(|service| service["envName"] == "prod")
        .unwrap();
    assert_eq!(prod["bindingKind"], "runtime");
    assert_eq!(prod["bindingName"], "managed");
    assert_eq!(prod["binaryPath"], path_string(&runtime_path));
    assert_eq!(prod["gatewayPort"], 18790);
    assert_eq!(prod["runDir"], path_string(&root.child("ocm-home/envs/prod")));
    assert_eq!(
        prod["args"],
        serde_json::json!(["gateway", "run", "--port", "18790"])
    );
    assert_eq!(prod["issue"], Value::Null);
}

#[test]
fn service_status_reports_missing_binding_issue() {
    let root = TestDir::new("service-status-unbound");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "bare"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "bare", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["envName"], "bare");
    assert_eq!(summary["gatewayPort"], 18789);
    assert_eq!(summary["bindingKind"], Value::Null);
    assert_eq!(summary["bindingName"], Value::Null);
    assert_eq!(summary["issue"], "environment \"bare\" has no default runtime or launcher; use env set-runtime, env set-launcher, or pass --runtime/--launcher");
}

#[test]
fn service_status_requires_target_or_all() {
    let root = TestDir::new("service-status-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service status requires <env> or --all"));
}

#[test]
fn unknown_service_commands_use_service_specific_errors() {
    let root = TestDir::new("service-unknown-command");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "restart"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown service command: restart"));
}
