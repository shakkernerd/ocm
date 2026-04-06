mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn migrate_group_help_is_available() {
    let root = TestDir::new("migrate-help-group");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "migrate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Migration commands"));
    assert!(body.contains("ocm migrate import --name mira"));
    assert!(body.contains("ocm migrate import --name mira --manifest ./ocm.yaml"));
    assert!(body.contains("ocm migrate inspect"));
    assert!(body.contains("ocm migrate plan --name mira"));
}

#[test]
fn migrate_inspect_defaults_to_the_plain_openclaw_home() {
    let root = TestDir::new("migrate-inspect-default");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["migrate", "inspect", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"sourceHome\":"));
    assert!(body.contains(".openclaw"));
    assert!(body.contains("\"exists\": false"));
}

#[test]
fn migrate_inspect_can_use_an_explicit_source_home() {
    let root = TestDir::new("migrate-inspect-explicit");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-openclaw");
    fs::create_dir_all(source_home.join("workspace")).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(source_home.join("openclaw.json"), "{}\n").unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "inspect",
            source_home.to_string_lossy().as_ref(),
            "--raw",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("exists: true"));
    assert!(body.contains("configExists: true"));
    assert!(body.contains("workspaceExists: true"));
}

#[test]
fn help_migrate_inspect_is_available() {
    let root = TestDir::new("migrate-help-inspect");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "migrate", "inspect"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Inspect a migration source"));
    assert!(body.contains("ocm migrate inspect [<source-home>] [--raw] [--json]"));
}

#[test]
fn help_migrate_import_is_available() {
    let root = TestDir::new("migrate-help-import");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "migrate", "import"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Import a plain OpenClaw home"));
    assert!(body.contains(
        "ocm migrate import --name <env> [<source-home>] [--root <path>] [--manifest <path>] [--raw] [--json]"
    ));
    assert!(body.contains("--manifest <path>"));
}

#[test]
fn migrate_plan_reports_the_target_env_and_root() {
    let root = TestDir::new("migrate-plan");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["migrate", "plan", "--name", "mira", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"envName\": \"mira\""));
    assert!(body.contains("\"envExists\": false"));
    assert!(body.contains("\"targetRoot\":"));
}

#[test]
fn migrate_plan_accepts_an_explicit_target_root() {
    let root = TestDir::new("migrate-plan-root");
    let cwd = root.child("workspace");
    let target_root = root.child("custom-root");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "plan",
            "--name",
            "mira",
            "--root",
            target_root.to_string_lossy().as_ref(),
            "--raw",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("env: mira"));
    assert!(body.contains(&format!("targetRoot: {}", target_root.to_string_lossy())));
}

#[test]
fn migrate_plan_can_preview_a_manifest_write() {
    let root = TestDir::new("migrate-plan-manifest");
    let cwd = root.child("workspace");
    let manifest_path = cwd.join("ocm.yaml");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "plan",
            "--name",
            "mira",
            "--manifest",
            "ocm.yaml",
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"manifestPath\":"));
    assert!(body.contains(&manifest_path.to_string_lossy().to_string()));
    assert!(body.contains("\"manifestPreview\":"));
    assert!(body.contains("schema: ocm/v1"));
    assert!(body.contains("name: mira"));
}

#[test]
fn help_migrate_plan_is_available() {
    let root = TestDir::new("migrate-help-plan");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "migrate", "plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Plan a migration target"));
    assert!(body.contains(
        "ocm migrate plan --name <env> [<source-home>] [--root <path>] [--raw] [--json]"
    ));
}

#[test]
fn migrate_import_creates_a_clean_env_from_plain_openclaw_home() {
    let root = TestDir::new("migrate-import");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    fs::create_dir_all(source_home.join("workspace")).unwrap();
    fs::create_dir_all(source_home.join("logs")).unwrap();
    fs::create_dir_all(source_home.join("agents/main/agent")).unwrap();
    fs::create_dir_all(source_home.join("agents/main/sessions")).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        source_home.join("openclaw.json"),
        format!(
            "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}\n",
            source_home.join("workspace").display()
        ),
    )
    .unwrap();
    fs::write(source_home.join("workspace/notes.txt"), "hello\n").unwrap();
    fs::write(source_home.join("logs/app.log"), "runtime residue\n").unwrap();
    fs::write(
        source_home.join("agents/main/agent/auth-profiles.json"),
        "{}\n",
    )
    .unwrap();
    fs::write(
        source_home.join("agents/main/sessions/main.jsonl"),
        "stale session\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "import",
            "--name",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"name\": \"mira\""));
    assert!(body.contains("\"sourceName\": \"plain-openclaw\""));

    let imported_state = root.child("ocm-home/envs/mira/.openclaw");
    assert!(imported_state.join("workspace/notes.txt").exists());
    assert!(
        imported_state
            .join("agents/main/agent/auth-profiles.json")
            .exists()
    );
    assert!(!imported_state.join("logs").exists());
    assert!(
        !imported_state
            .join("agents/main/sessions/main.jsonl")
            .exists()
    );

    let config_text = fs::read_to_string(imported_state.join("openclaw.json")).unwrap();
    assert!(config_text.contains(&imported_state.join("workspace").display().to_string()));
    assert!(!config_text.contains(&source_home.display().to_string()));
}

#[test]
fn migrate_import_requires_name() {
    let root = TestDir::new("migrate-import-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["migrate", "import"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("--name is required"));
}

#[test]
fn migrate_import_can_write_a_manifest() {
    let root = TestDir::new("migrate-import-manifest");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    let manifest_path = cwd.join("ocm.yaml");
    fs::create_dir_all(source_home.join("workspace")).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(source_home.join("openclaw.json"), "{}\n").unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "import",
            "--name",
            "mira",
            "--manifest",
            manifest_path.to_string_lossy().as_ref(),
            source_home.to_string_lossy().as_ref(),
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"manifestPath\":"));
    assert!(manifest_path.exists());
    let manifest_raw = fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_raw.contains("schema: ocm/v1"));
    assert!(manifest_raw.contains("name: mira"));
}

#[test]
fn migrate_import_resolves_relative_manifest_paths_from_cwd() {
    let root = TestDir::new("migrate-import-manifest-relative");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    let manifest_path = cwd.join("ocm.yaml");
    fs::create_dir_all(source_home.join("workspace")).unwrap();
    fs::create_dir_all(&cwd).unwrap();
    fs::write(source_home.join("openclaw.json"), "{}\n").unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "import",
            "--name",
            "mira",
            "--manifest",
            "ocm.yaml",
            source_home.to_string_lossy().as_ref(),
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(manifest_path.exists());
    let body = stdout(&output);
    assert!(body.contains("\"manifestPath\":"));
}
