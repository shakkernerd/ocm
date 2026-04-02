mod support;

use std::fs;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn env_cleanup_preview_reports_safe_repairs_without_applying_them() {
    let root = TestDir::new("env-cleanup-preview");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/envs/demo/.ocm-env.json")).unwrap();

    let meta_path = root.child("ocm-home/envs/demo.json");
    let updated = "{\n  \"kind\": \"ocm-env\",\n  \"name\": \"demo\",\n  \"root\": \"REPLACE_ROOT\",\n  \"gatewayPort\": null,\n  \"defaultRuntime\": \"ghost-runtime\",\n  \"defaultLauncher\": \"ghost-launcher\",\n  \"protected\": false,\n  \"createdAt\": \"2026-03-25T00:00:00Z\",\n  \"updatedAt\": \"2026-03-25T00:00:00Z\",\n  \"lastUsedAt\": null\n}\n"
        .replace(
            "REPLACE_ROOT",
            &root.child("ocm-home/envs/demo").display().to_string(),
        );
    fs::write(&meta_path, updated).unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "demo"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let output = stdout(&cleanup);
    assert!(output.contains("Cleanup preview for env demo"));
    assert!(output.contains("safe fixes: 3"));
    assert!(output.contains("repair-marker: rewrite the environment marker file"));
    assert!(
        output.contains("clear-missing-runtime: clear missing runtime binding \"ghost-runtime\"")
    );
    assert!(
        output
            .contains("clear-missing-launcher: clear missing launcher binding \"ghost-launcher\"")
    );
    assert!(output.contains("re-run with --yes to apply them"));

    let persisted = fs::read_to_string(meta_path).unwrap();
    assert!(persisted.contains("\"defaultRuntime\": \"ghost-runtime\""));
    assert!(!root.child("ocm-home/envs/demo/.ocm-env.json").exists());
}

#[test]
fn env_cleanup_preview_json_reports_actions_and_current_issues() {
    let root = TestDir::new("env-cleanup-preview-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/envs/demo/.ocm-env.json")).unwrap();

    let meta_path = root.child("ocm-home/envs/demo.json");
    let updated = "{\n  \"kind\": \"ocm-env\",\n  \"name\": \"demo\",\n  \"root\": \"REPLACE_ROOT\",\n  \"gatewayPort\": null,\n  \"defaultRuntime\": null,\n  \"defaultLauncher\": \"ghost-launcher\",\n  \"protected\": false,\n  \"createdAt\": \"2026-03-25T00:00:00Z\",\n  \"updatedAt\": \"2026-03-25T00:00:00Z\",\n  \"lastUsedAt\": null\n}\n"
        .replace(
            "REPLACE_ROOT",
            &root.child("ocm-home/envs/demo").display().to_string(),
        );
    fs::write(&meta_path, updated).unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "demo", "--json"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let value: Value = serde_json::from_str(&stdout(&cleanup)).unwrap();
    assert_eq!(value["apply"], false);
    assert_eq!(value["envName"], "demo");
    assert_eq!(value["actions"].as_array().unwrap().len(), 2);
    assert_eq!(value["actions"][0]["kind"], "repair-marker");
    assert_eq!(value["actions"][0]["applied"], false);
    assert_eq!(value["actions"][1]["kind"], "clear-missing-launcher");
    assert!(
        value["issuesBefore"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap()
                .contains("environment marker is missing"))
    );
}

#[test]
fn env_cleanup_yes_applies_safe_repairs() {
    let root = TestDir::new("env-cleanup-apply");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::write(
        root.child("ocm-home/envs/demo/.ocm-env.json"),
        "{\n  \"kind\": \"ocm-env-marker\",\n  \"name\": \"other\",\n  \"createdAt\": \"2026-03-25T00:00:00Z\"\n}\n",
    )
    .unwrap();

    let meta_path = root.child("ocm-home/envs/demo.json");
    let updated = "{\n  \"kind\": \"ocm-env\",\n  \"name\": \"demo\",\n  \"root\": \"REPLACE_ROOT\",\n  \"gatewayPort\": null,\n  \"defaultRuntime\": null,\n  \"defaultLauncher\": \"ghost-launcher\",\n  \"protected\": false,\n  \"createdAt\": \"2026-03-25T00:00:00Z\",\n  \"updatedAt\": \"2026-03-25T00:00:00Z\",\n  \"lastUsedAt\": null\n}\n"
        .replace(
            "REPLACE_ROOT",
            &root.child("ocm-home/envs/demo").display().to_string(),
        );
    fs::write(&meta_path, updated).unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "demo", "--yes"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let output = stdout(&cleanup);
    assert!(output.contains("Applied cleanup for env demo"));
    assert!(output.contains("applied fixes: 2"));
    assert!(output.contains("repair-marker: rewrite the environment marker file"));
    assert!(
        output
            .contains("clear-missing-launcher: clear missing launcher binding \"ghost-launcher\"")
    );

    let persisted = fs::read_to_string(meta_path).unwrap();
    assert!(persisted.contains("\"defaultLauncher\": null"));

    let marker_raw = fs::read_to_string(root.child("ocm-home/envs/demo/.ocm-env.json")).unwrap();
    assert!(marker_raw.contains("\"name\": \"demo\""));
}

#[test]
fn env_cleanup_yes_json_reports_applied_actions_and_remaining_issues() {
    let root = TestDir::new("env-cleanup-apply-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/envs/demo/.ocm-env.json")).unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "demo", "--yes", "--json"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let value: Value = serde_json::from_str(&stdout(&cleanup)).unwrap();
    assert_eq!(value["apply"], true);
    assert_eq!(value["actions"].as_array().unwrap().len(), 1);
    assert_eq!(value["actions"][0]["kind"], "repair-marker");
    assert_eq!(value["actions"][0]["applied"], true);
    assert_eq!(value["healthyAfter"], false);
    assert!(
        value["issuesAfter"]
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue
                .as_str()
                .unwrap()
                .contains("has no default runtime or launcher"))
    );
}

#[test]
fn env_cleanup_all_preview_reports_only_actionable_envs() {
    let root = TestDir::new("env-cleanup-all-preview");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let broken = run_ocm(&cwd, &env, &["env", "create", "broken"]);
    assert!(broken.status.success(), "{}", stderr(&broken));
    let healthy = run_ocm(&cwd, &env, &["env", "create", "healthy"]);
    assert!(healthy.status.success(), "{}", stderr(&healthy));

    fs::remove_file(root.child("ocm-home/envs/broken/.ocm-env.json")).unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "--all"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let output = stdout(&cleanup);
    assert!(output.contains("Cleanup preview (--all): 1 env(s)"));
    assert!(output.contains("  broken"));
    assert!(!output.contains("  healthy"));
}

#[test]
fn env_cleanup_all_yes_json_applies_repairs_across_envs() {
    let root = TestDir::new("env-cleanup-all-apply-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let first = run_ocm(&cwd, &env, &["env", "create", "first"]);
    assert!(first.status.success(), "{}", stderr(&first));
    let second = run_ocm(&cwd, &env, &["env", "create", "second"]);
    assert!(second.status.success(), "{}", stderr(&second));

    fs::remove_file(root.child("ocm-home/envs/first/.ocm-env.json")).unwrap();
    fs::remove_file(root.child("ocm-home/envs/second/.ocm-env.json")).unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "--all", "--yes", "--json"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let value: Value = serde_json::from_str(&stdout(&cleanup)).unwrap();
    assert_eq!(value["apply"], true);
    assert_eq!(value["count"], 2);
    assert_eq!(value["results"].as_array().unwrap().len(), 2);
    assert!(root.child("ocm-home/envs/first/.ocm-env.json").exists());
    assert!(root.child("ocm-home/envs/second/.ocm-env.json").exists());
}

#[test]
fn env_cleanup_yes_repairs_env_scoped_config_drift() {
    let root = TestDir::new("env-cleanup-config-repair");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let source = run_ocm(&cwd, &env, &["env", "create", "source", "--port", "19789"]);
    assert!(source.status.success(), "{}", stderr(&source));
    let target = run_ocm(&cwd, &env, &["env", "create", "target", "--port", "19790"]);
    assert!(target.status.success(), "{}", stderr(&target));

    let source_root = root.child("ocm-home/envs/source");
    let target_config = root.child("ocm-home/envs/target/.openclaw/openclaw.json");
    fs::write(
        &target_config,
        format!(
            "{{\n  \"agents\": {{\n    \"defaults\": {{\n      \"workspace\": \"{}\"\n    }}\n  }},\n  \"gateway\": {{\n    \"port\": 19789\n  }}\n}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
    )
    .unwrap();

    let cleanup = run_ocm(&cwd, &env, &["env", "cleanup", "target", "--yes"]);
    assert!(cleanup.status.success(), "{}", stderr(&cleanup));
    let output = stdout(&cleanup);
    assert!(
        output
            .contains("repair-openclaw-config: rewrite env-scoped OpenClaw config paths and ports")
    );

    let repaired = fs::read_to_string(target_config).unwrap();
    assert!(
        repaired.contains(
            &root
                .child("ocm-home/envs/target/.openclaw/workspace")
                .display()
                .to_string()
        )
    );
    assert!(repaired.contains("\"port\": 19790"));
}
