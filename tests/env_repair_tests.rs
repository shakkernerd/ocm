mod support;

use std::fs;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn env_repair_marker_rewrites_a_mismatched_marker() {
    let root = TestDir::new("env-repair-marker");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let marker_path = root.child("ocm-home/envs/demo/.ocm-env.json");
    fs::write(
        &marker_path,
        "{\n  \"kind\": \"ocm-env-marker\",\n  \"name\": \"other\",\n  \"createdAt\": \"2026-03-25T00:00:00Z\"\n}\n",
    )
    .unwrap();

    let repair = run_ocm(&cwd, &env, &["env", "repair-marker", "demo"]);
    assert!(repair.status.success(), "{}", stderr(&repair));
    let output = stdout(&repair);
    assert!(output.contains("Repaired marker for env demo"));
    assert!(output.contains("marker:"));

    let marker_raw = fs::read_to_string(marker_path).unwrap();
    assert!(marker_raw.contains("\"name\": \"demo\""));
}

#[test]
fn env_repair_marker_json_reports_the_rewritten_marker_path() {
    let root = TestDir::new("env-repair-marker-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/envs/demo/.ocm-env.json")).unwrap();

    let repair = run_ocm(&cwd, &env, &["env", "repair-marker", "demo", "--json"]);
    assert!(repair.status.success(), "{}", stderr(&repair));
    let value: Value = serde_json::from_str(&stdout(&repair)).unwrap();
    assert_eq!(value["envName"], "demo");
    assert_eq!(value["root"], root.child("ocm-home/envs/demo").display().to_string());
    assert_eq!(
        value["markerPath"],
        root.child("ocm-home/envs/demo/.ocm-env.json")
            .display()
            .to_string()
    );
}
