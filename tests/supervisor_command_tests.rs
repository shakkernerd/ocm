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
