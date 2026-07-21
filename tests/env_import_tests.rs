mod support;

use std::{fs, path::Path};

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_text};

#[test]
fn env_import_restores_an_archive_with_a_new_name_and_root() {
    let root = TestDir::new("env-import");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "source", "--port", "19789", "--protect"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    write_text(
        &root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"),
        "hello import",
    );
    let source_state = root.child("ocm-home/envs/source/.openclaw");
    write_text(
        &source_state.join("openclaw.json"),
        &format!(
            r#"{{"agents":{{"list":[{{"id":"main","default":true}},{{"id":"ops","workspace":"{}"}}]}}}}"#,
            source_state.join("team/ops").display()
        ),
    );
    write_text(
        &source_state.join("team/ops/custom.txt"),
        "hello custom import",
    );

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./archives/source-backup.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./archives/source-backup.tar",
            "--name",
            "target",
            "--root",
            "./imports/target-root",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));
    let output = stdout(&import);
    assert!(output.contains("Imported env target from source"));

    let show = run_ocm(&cwd, &env, &["env", "show", "target", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_output = stdout(&show);
    assert!(show_output.contains("\"name\": \"target\""));
    let show_json: Value = serde_json::from_str(&show_output).unwrap();
    assert_ne!(show_json["gatewayPort"].as_u64(), Some(19_789));
    assert_eq!(show_json["serviceEnabled"], false);
    assert_eq!(show_json["serviceRunning"], false);
    assert!(show_output.contains("\"protected\": true"));

    assert_eq!(
        fs::read_to_string(
            root.child("workspace/imports/target-root/.openclaw/workspace/notes.txt")
        )
        .unwrap(),
        "hello import"
    );
    let target_state = root.child("workspace/imports/target-root/.openclaw");
    assert_eq!(
        fs::read_to_string(target_state.join("team/ops/custom.txt")).unwrap(),
        "hello custom import"
    );
    let config: Value =
        serde_json::from_str(&fs::read_to_string(target_state.join("openclaw.json")).unwrap())
            .unwrap();
    let actual_workspace = fs::canonicalize(Path::new(
        config["agents"]["list"][1]["workspace"].as_str().unwrap(),
    ))
    .unwrap();
    let expected_workspace = fs::canonicalize(target_state.join("team/ops")).unwrap();
    assert_eq!(actual_workspace, expected_workspace);
}

#[test]
fn env_import_json_reports_the_archive_and_source_name() {
    let root = TestDir::new("env-import-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let export = run_ocm(&cwd, &env, &["env", "export", "source"]);
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./source.ocm-env.tar",
            "--name",
            "target",
            "--json",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));
    let output = stdout(&import);
    assert!(output.contains("\"name\": \"target\""));
    assert!(output.contains("\"sourceName\": \"source\""));
    assert!(output.contains("\"archivePath\":"));
}

#[test]
fn env_import_rewrites_openclaw_config_for_the_new_root() {
    let root = TestDir::new("env-import-config-rewrite");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source", "--port", "19789"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let source_root = root.child("ocm-home/envs/source");
    fs::write(
        source_root.join(".openclaw/openclaw.json"),
        format!(
            "{{\n  \"agents\": {{\n    \"defaults\": {{\n      \"workspace\": \"{}\"\n    }}\n  }},\n  \"gateway\": {{\n    \"port\": 19789\n  }}\n}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
    )
    .unwrap();

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./archives/source-config.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./archives/source-config.tar",
            "--name",
            "target",
            "--root",
            "./imports/target-root",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));

    let raw =
        fs::read_to_string(root.child("workspace/imports/target-root/.openclaw/openclaw.json"))
            .unwrap();
    let config: Value = serde_json::from_str(&raw).unwrap();
    let actual_workspace = fs::canonicalize(Path::new(
        config["agents"]["defaults"]["workspace"].as_str().unwrap(),
    ))
    .unwrap();
    let expected_workspace = fs::canonicalize(root.child("workspace/imports/target-root"))
        .unwrap()
        .join(".openclaw/workspace");
    assert_eq!(actual_workspace, expected_workspace);
    assert_ne!(config["gateway"]["port"].as_u64(), Some(19_789));
}

#[test]
fn env_import_resets_or_replaces_a_copied_public_sandbox_origin() {
    let root = TestDir::new("env-import-sandbox-origin");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source", "--port", "19789"]);
    assert!(create.status.success(), "{}", stderr(&create));
    write_text(
        &root.child("ocm-home/envs/source/.openclaw/openclaw.json"),
        concat!(
            "{\n",
            "  \"gateway\": { \"port\": 19789 },\n",
            "  \"mcp\": {\n",
            "    \"apps\": {\n",
            "      \"enabled\": true,\n",
            "      \"sandboxPort\": 19790,\n",
            "      \"sandboxOrigin\": \"https://source.example.test\"\n",
            "    }\n",
            "  }\n",
            "}\n"
        ),
    );

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./source-sandbox.ocm-env.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let reset = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./source-sandbox.ocm-env.tar",
            "--name",
            "target-reset",
        ],
    );
    assert!(reset.status.success(), "{}", stderr(&reset));
    assert!(
        stderr(&reset).contains("removed copied MCP app sandbox origin from env target-reset"),
        "{}",
        stderr(&reset)
    );
    assert!(!stderr(&reset).contains("source.example.test"));
    let reset_config: Value = serde_json::from_str(
        &fs::read_to_string(root.child("ocm-home/envs/target-reset/.openclaw/openclaw.json"))
            .unwrap(),
    )
    .unwrap();
    assert!(reset_config["mcp"]["apps"]["sandboxOrigin"].is_null());

    let replaced = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./source-sandbox.ocm-env.tar",
            "--name",
            "target-explicit",
            "--sandbox-origin",
            "https://target.example.test",
        ],
    );
    assert!(replaced.status.success(), "{}", stderr(&replaced));
    assert!(!stderr(&replaced).contains("removed copied MCP app sandbox origin"));
    let replaced_config: Value = serde_json::from_str(
        &fs::read_to_string(root.child("ocm-home/envs/target-explicit/.openclaw/openclaw.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        replaced_config["mcp"]["apps"]["sandboxOrigin"].as_str(),
        Some("https://target.example.test")
    );
}

#[test]
fn env_import_validates_the_target_origin_before_reading_the_archive() {
    let root = TestDir::new("env-import-invalid-origin-preflight");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./missing.ocm-env.tar",
            "--sandbox-origin",
            "https://target.example.test/apps",
        ],
    );
    assert_eq!(import.status.code(), Some(1));
    assert!(stderr(&import).contains(
        "--sandbox-origin must be an HTTP(S) origin without a path, query, or credentials"
    ));
    assert!(!stderr(&import).contains("missing.ocm-env.tar"));
}

#[test]
fn env_import_rejects_include_owned_sandbox_configuration_before_creating_the_target() {
    let root = TestDir::new("env-import-include-owned-sandbox-origin");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));
    write_text(
        &root.child("ocm-home/envs/source/.openclaw/openclaw.json"),
        "{\n  \"$include\": \"./base.json5\"\n}\n",
    );
    write_text(
        &root.child("ocm-home/envs/source/.openclaw/base.json5"),
        "{ mcp: { apps: { sandboxOrigin: 'https://source.example.test' } } }\n",
    );
    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./include-owned.ocm-env.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./include-owned.ocm-env.tar",
            "--name",
            "target",
        ],
    );
    assert_eq!(import.status.code(), Some(1));
    assert!(
        stderr(&import).contains(
            "cannot safely reset mcp.apps.sandboxOrigin because OpenClaw config uses $include at the config root"
        ),
        "{}",
        stderr(&import)
    );
    assert!(!root.child("ocm-home/envs/target").exists());
}

#[test]
fn env_import_keeps_agent_auth_but_drops_live_runtime_state() {
    let root = TestDir::new("env-import-runtime-cleanup");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source", "--port", "19789"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let source_state = root.child("ocm-home/envs/source/.openclaw");
    fs::create_dir_all(source_state.join("agents/main/agent")).unwrap();
    fs::create_dir_all(source_state.join("agents/main/sessions")).unwrap();
    fs::create_dir_all(source_state.join("logs")).unwrap();
    write_text(
        &source_state.join("agents/main/agent/auth-profiles.json"),
        "{\"default\":\"ok\"}\n",
    );
    write_text(
        &source_state.join("agents/main/agent/models.json"),
        "{\"primary\":\"gpt-5.4\"}\n",
    );
    write_text(
        &source_state.join("agents/main/sessions/main.jsonl"),
        "{\"type\":\"session\"}\n",
    );
    write_text(&source_state.join("logs/gateway.log"), "copied log\n");
    write_text(&source_state.join("openclaw.json.bak"), "{}\n");

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./archives/source-runtime.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./archives/source-runtime.tar",
            "--name",
            "target",
            "--root",
            "./imports/target-root",
        ],
    );
    assert!(import.status.success(), "{}", stderr(&import));

    let target_state = root.child("workspace/imports/target-root/.openclaw");
    assert!(
        target_state
            .join("agents/main/agent/auth-profiles.json")
            .exists()
    );
    assert!(target_state.join("agents/main/agent/models.json").exists());
    assert!(!target_state.join("agents/main/sessions").exists());
    assert!(!target_state.join("logs").exists());
    assert!(!target_state.join("openclaw.json.bak").exists());
}
