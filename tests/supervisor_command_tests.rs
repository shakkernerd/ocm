mod support;

use std::fs;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, path_string, run_ocm, stderr, write_executable_script};

fn setup_supervisor_fixture(
    root: &TestDir,
) -> (
    std::path::PathBuf,
    std::collections::BTreeMap<String, String>,
) {
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "openclaw"],
    );
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

    let demo = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "dev"]);
    assert!(demo.status.success(), "{}", stderr(&demo));

    let prod = run_ocm(
        &cwd,
        &env,
        &["env", "create", "prod", "--runtime", "managed"],
    );
    assert!(prod.status.success(), "{}", stderr(&prod));

    let bare = run_ocm(&cwd, &env, &["env", "create", "bare"]);
    assert!(bare.status.success(), "{}", stderr(&bare));

    (cwd, env)
}

#[test]
fn supervisor_plan_reports_runnable_children_and_skipped_envs() {
    let root = TestDir::new("supervisor-plan");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let output = run_ocm(&cwd, &env, &["supervisor", "plan", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["persisted"], false);
    assert!(
        body["statePath"]
            .as_str()
            .unwrap()
            .ends_with("/supervisor/state.json")
    );

    let children = body["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);

    let demo = children
        .iter()
        .find(|child| child["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingKind"], "launcher");
    assert_eq!(demo["startMode"], "on-demand");
    assert_eq!(demo["childPort"], 18789);
    assert!(
        demo["envOverrides"]["OPENCLAW_HOME"]
            .as_str()
            .unwrap()
            .contains("/envs/demo")
    );

    let prod = children
        .iter()
        .find(|child| child["envName"] == "prod")
        .unwrap();
    assert_eq!(prod["bindingKind"], "runtime");
    assert_eq!(prod["bindingName"], "managed");
    assert!(
        prod["binaryPath"]
            .as_str()
            .unwrap()
            .ends_with("/bin/openclaw")
    );

    let skipped = body["skippedEnvs"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["envName"], "bare");
    assert!(
        skipped[0]["reason"]
            .as_str()
            .unwrap()
            .contains("has no default runtime or launcher")
    );
}

#[test]
fn supervisor_sync_persists_and_show_reads_the_state() {
    let root = TestDir::new("supervisor-sync");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync", "--json"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_body: Value = serde_json::from_slice(&sync.stdout).unwrap();
    assert_eq!(sync_body["persisted"], true);

    let state_path = sync_body["statePath"].as_str().unwrap();
    assert!(
        fs::metadata(state_path).is_ok(),
        "missing state file at {state_path}"
    );

    let show = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_body: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(show_body["persisted"], true);
    assert_eq!(show_body["statePath"], sync_body["statePath"]);
    assert_eq!(show_body["children"], sync_body["children"]);
    assert_eq!(show_body["skippedEnvs"], sync_body["skippedEnvs"]);
}

#[test]
fn supervisor_status_reports_missing_and_changed_state() {
    let root = TestDir::new("supervisor-status");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let before = run_ocm(&cwd, &env, &["supervisor", "status", "--json"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_body: Value = serde_json::from_slice(&before.stdout).unwrap();
    assert_eq!(before_body["statePresent"], false);
    assert_eq!(before_body["inSync"], false);
    assert_eq!(before_body["missingChildren"].as_array().unwrap().len(), 2);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let after_sync = run_ocm(&cwd, &env, &["supervisor", "status", "--json"]);
    assert!(after_sync.status.success(), "{}", stderr(&after_sync));
    let after_sync_body: Value = serde_json::from_slice(&after_sync.stdout).unwrap();
    assert_eq!(after_sync_body["statePresent"], true);
    assert_eq!(after_sync_body["inSync"], true);
    assert!(
        after_sync_body["changedChildren"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let removed_launcher = run_ocm(&cwd, &env, &["launcher", "remove", "dev"]);
    assert!(
        removed_launcher.status.success(),
        "{}",
        stderr(&removed_launcher)
    );

    let after_change = run_ocm(&cwd, &env, &["supervisor", "status", "--json"]);
    assert!(after_change.status.success(), "{}", stderr(&after_change));
    let after_change_body: Value = serde_json::from_slice(&after_change.stdout).unwrap();
    assert_eq!(after_change_body["inSync"], false);
    assert_eq!(
        after_change_body["extraChildren"],
        serde_json::json!(["demo"])
    );
    assert_eq!(
        after_change_body["skippedEnvChanges"],
        serde_json::json!(["demo"])
    );
}

#[test]
fn env_binding_changes_refresh_persisted_supervisor_state() {
    let root = TestDir::new("supervisor-auto-refresh-env-binding");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev-b", "--command", "openclaw --beta"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));
    let rebind = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev-b"]);
    assert!(rebind.status.success(), "{}", stderr(&rebind));

    let show = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_body: Value = serde_json::from_slice(&show.stdout).unwrap();
    let demo = show_body["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingName"], "dev-b");

    let status = run_ocm(&cwd, &env, &["supervisor", "status", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_body["inSync"], true);
}

#[test]
fn env_create_and_remove_refresh_persisted_supervisor_state() {
    let root = TestDir::new("supervisor-auto-refresh-env-shape");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let created = run_ocm(&cwd, &env, &["env", "create", "extra", "--launcher", "dev"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let show_after_create = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(
        show_after_create.status.success(),
        "{}",
        stderr(&show_after_create)
    );
    let created_body: Value = serde_json::from_slice(&show_after_create.stdout).unwrap();
    assert!(
        created_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "extra")
    );

    let removed = run_ocm(&cwd, &env, &["env", "remove", "extra", "--force"]);
    assert!(removed.status.success(), "{}", stderr(&removed));

    let show_after_remove = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(
        show_after_remove.status.success(),
        "{}",
        stderr(&show_after_remove)
    );
    let removed_body: Value = serde_json::from_slice(&show_after_remove.stdout).unwrap();
    assert!(
        !removed_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "extra")
    );
}
