mod support;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_text};

fn install_fake_openclaw_on_path(
    root: &TestDir,
    env: &mut std::collections::BTreeMap<String, String>,
) {
    let bin_dir = root.child("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let openclaw = bin_dir.join("openclaw");
    fs::write(&openclaw, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(&openclaw).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&openclaw, permissions).unwrap();
    }
    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let path = if existing_path.is_empty() {
        bin_dir.display().to_string()
    } else {
        format!("{}:{existing_path}", bin_dir.display())
    };
    env.insert("PATH".to_string(), path);
}

#[test]
fn migrate_help_is_available() {
    let root = TestDir::new("migrate-help");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "migrate"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Migrate an existing OpenClaw home"));
    assert!(body.contains("ocm migrate <env> [<source-home>]"));
    assert!(body.contains("ocm migrate mira"));
    assert!(body.contains("ocm migrate mira --manifest ./ocm.yaml"));
    assert!(body.contains("migrated launcher"));
    assert!(body.contains("Use `adopt inspect` or `adopt plan`"));
}

#[test]
fn adopt_group_help_is_available() {
    let root = TestDir::new("adopt-help-group");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "adopt"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Adoption commands"));
    assert!(body.contains("ocm adopt inspect"));
    assert!(body.contains("ocm adopt plan --name mira"));
    assert!(body.contains("ocm adopt import --name mira"));
}

#[test]
fn adopt_inspect_defaults_to_the_plain_openclaw_home() {
    let root = TestDir::new("adopt-inspect-default");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["adopt", "inspect", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"sourceHome\":"));
    assert!(body.contains(".openclaw"));
    assert!(body.contains("\"exists\": false"));
}

#[test]
fn adopt_inspect_can_use_an_explicit_source_home() {
    let root = TestDir::new("adopt-inspect-explicit");
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
            "adopt",
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
fn help_adopt_inspect_is_available() {
    let root = TestDir::new("adopt-help-inspect");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "adopt", "inspect"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Inspect a migration source"));
    assert!(body.contains("ocm adopt inspect [<source-home>] [--raw] [--json]"));
}

#[test]
fn help_adopt_import_is_available() {
    let root = TestDir::new("adopt-help-import");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "adopt", "import"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Import a plain OpenClaw home"));
    assert!(body.contains(
        "preserve config, auth, sessions, and logs, and clear only live runtime residue like locks, pid files, and sockets."
    ));
    assert!(body.contains(
        "ocm adopt import --name <env> [<source-home>] [--root <path>] [--manifest <path>] [--raw] [--json]"
    ));
    assert!(body.contains("--manifest <path>"));
    assert!(body.contains("migrated launcher"));
    assert!(body.contains(
        "Relative manifest file paths passed through `--manifest` are resolved from the current working directory."
    ));
}

#[test]
fn adopt_plan_reports_the_target_env_and_root() {
    let root = TestDir::new("adopt-plan");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["adopt", "plan", "--name", "mira", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"envName\": \"mira\""));
    assert!(body.contains("\"envExists\": false"));
    assert!(body.contains("\"targetRoot\":"));
}

#[test]
fn adopt_plan_accepts_an_explicit_target_root() {
    let root = TestDir::new("adopt-plan-root");
    let cwd = root.child("workspace");
    let target_root = root.child("custom-root");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "adopt",
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
fn adopt_plan_can_preview_a_manifest_write() {
    let root = TestDir::new("adopt-plan-manifest");
    let cwd = root.child("workspace");
    let manifest_path = cwd.join("ocm.yaml");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "adopt",
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
fn help_adopt_plan_is_available() {
    let root = TestDir::new("adopt-help-plan");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "adopt", "plan"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Plan a migration target"));
    assert!(body.contains(
        "ocm adopt plan --name <env> [<source-home>] [--root <path>] [--manifest <path>] [--raw] [--json]"
    ));
    assert!(body.contains("--manifest <path>"));
    assert!(body.contains(
        "Relative manifest file paths passed through `--manifest` are resolved from the current working directory."
    ));
}

fn seed_plain_openclaw_home(source_home: &std::path::Path) {
    fs::create_dir_all(source_home.join("workspace")).unwrap();
    fs::create_dir_all(source_home.join("logs")).unwrap();
    fs::create_dir_all(source_home.join("run")).unwrap();
    fs::create_dir_all(source_home.join("agents/main/agent")).unwrap();
    fs::create_dir_all(source_home.join("agents/main/sessions")).unwrap();
    fs::write(
        source_home.join("openclaw.json"),
        format!(
            "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}\n",
            source_home.join("workspace").display()
        ),
    )
    .unwrap();
    fs::write(source_home.join("workspace/notes.txt"), "hello\n").unwrap();
    fs::write(
        source_home.join("logs/app.log"),
        format!("cwd={}\n", source_home.join("workspace").display()),
    )
    .unwrap();
    fs::write(
        source_home.join("agents/main/agent/auth-profiles.json"),
        "{}\n",
    )
    .unwrap();
    fs::write(
        source_home.join("agents/main/sessions/main.jsonl"),
        format!(
            "{{\"cwd\":\"{}\",\"log\":\"{}\"}}\n",
            source_home.join("workspace").display(),
            source_home.join("logs/app.log").display()
        ),
    )
    .unwrap();
    fs::write(
        source_home.join("openclaw.json.bak"),
        format!("backup={}\n", source_home.display()),
    )
    .unwrap();
    fs::write(source_home.join("gateway.pid"), "4242\n").unwrap();
    fs::write(source_home.join("run/live.sock"), "sock\n").unwrap();
}

fn assert_imported_plain_openclaw_home(
    imported_state: &std::path::Path,
    source_home: &std::path::Path,
) {
    assert!(imported_state.join("workspace/notes.txt").exists());
    assert!(
        imported_state
            .join("agents/main/agent/auth-profiles.json")
            .exists()
    );
    assert!(imported_state.join("logs/app.log").exists());
    assert!(
        imported_state
            .join("agents/main/sessions/main.jsonl")
            .exists()
    );
    assert!(imported_state.join("openclaw.json.bak").exists());
    assert!(!imported_state.join("gateway.pid").exists());
    assert!(!imported_state.join("run/live.sock").exists());

    let config_text = fs::read_to_string(imported_state.join("openclaw.json")).unwrap();
    assert!(config_text.contains(&imported_state.join("workspace").display().to_string()));
    assert!(!config_text.contains(&source_home.display().to_string()));

    let session_text =
        fs::read_to_string(imported_state.join("agents/main/sessions/main.jsonl")).unwrap();
    let log_text = fs::read_to_string(imported_state.join("logs/app.log")).unwrap();
    let backup_text = fs::read_to_string(imported_state.join("openclaw.json.bak")).unwrap();
    assert!(session_text.contains(&imported_state.join("workspace").display().to_string()));
    assert!(session_text.contains(&imported_state.join("logs/app.log").display().to_string()));
    assert!(!session_text.contains(&source_home.display().to_string()));
    assert!(log_text.contains(&imported_state.join("workspace").display().to_string()));
    assert!(!log_text.contains(&source_home.display().to_string()));
    assert!(backup_text.contains(&imported_state.display().to_string()));
    assert!(!backup_text.contains(&source_home.display().to_string()));
}

#[test]
fn migrate_preserves_history_and_logs_from_plain_openclaw_home() {
    let root = TestDir::new("migrate-direct");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    fs::create_dir_all(&cwd).unwrap();
    seed_plain_openclaw_home(&source_home);
    let mut env = ocm_env(&root);
    install_fake_openclaw_on_path(&root, &mut env);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"name\": \"mira\""));
    assert!(body.contains("\"sourceName\": \"plain-openclaw\""));
    assert!(body.contains("\"defaultLauncher\": \"mira.migrated\""));

    let imported_state = root.child("ocm-home/envs/mira/.openclaw");
    assert_imported_plain_openclaw_home(&imported_state, &source_home);
}

#[test]
fn adopt_import_preserves_history_and_logs_from_plain_openclaw_home() {
    let root = TestDir::new("adopt-import");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    fs::create_dir_all(&cwd).unwrap();
    seed_plain_openclaw_home(&source_home);
    let mut env = ocm_env(&root);
    install_fake_openclaw_on_path(&root, &mut env);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "adopt",
            "import",
            "--name",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("\"defaultLauncher\": \"mira.migrated\""));

    let imported_state = root.child("ocm-home/envs/mira/.openclaw");
    assert_imported_plain_openclaw_home(&imported_state, &source_home);
}

#[test]
fn migrate_requires_name() {
    let root = TestDir::new("migrate-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["migrate"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("migrate requires <env> or --name <env>"));
}

#[test]
fn migrate_rejects_multiple_env_names() {
    let root = TestDir::new("migrate-name-conflict");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["migrate", "mira", "--name", "ember"]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("migrate accepts only one env name from <env> or --name <env>")
    );
}

#[test]
fn migrate_can_write_a_manifest() {
    let root = TestDir::new("migrate-direct-manifest");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    let manifest_path = cwd.join("ocm.yaml");
    fs::create_dir_all(&cwd).unwrap();
    seed_plain_openclaw_home(&source_home);
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--manifest",
            manifest_path.to_string_lossy().as_ref(),
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
fn migrate_can_write_a_nested_manifest_path() {
    let root = TestDir::new("migrate-direct-nested-manifest");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    let manifest_path = cwd.join("nested/project/ocm.yaml");
    fs::create_dir_all(&cwd).unwrap();
    seed_plain_openclaw_home(&source_home);
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--manifest",
            "./nested/project/ocm.yaml",
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"manifestPath\":"));
    assert!(body.contains(&manifest_path.to_string_lossy().to_string()));

    let manifest_raw = fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_raw.contains("schema: ocm/v1"));
    assert!(manifest_raw.contains("name: mira"));
}

#[test]
fn migrate_resolves_relative_manifest_paths_from_cwd() {
    let root = TestDir::new("migrate-direct-manifest-relative");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    let manifest_path = cwd.join("ocm.yaml");
    fs::create_dir_all(&cwd).unwrap();
    seed_plain_openclaw_home(&source_home);
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--manifest",
            "ocm.yaml",
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(manifest_path.exists());
    let body = stdout(&output);
    assert!(body.contains("\"manifestPath\":"));
}

#[test]
fn migrate_rejects_manifest_targets_under_regular_files_before_importing() {
    let root = TestDir::new("migrate-direct-manifest-parent-file");
    let cwd = root.child("workspace");
    let source_home = root.child("legacy-home/.openclaw");
    fs::create_dir_all(&cwd).unwrap();
    seed_plain_openclaw_home(&source_home);
    write_text(&cwd.join("occupied"), "not a directory\n");
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "migrate",
            "mira",
            source_home.to_string_lossy().as_ref(),
            "--manifest",
            "occupied/ocm.yaml",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("manifest parent is not a directory"),
        "{}",
        stderr(&output)
    );

    let env_show = run_ocm(&cwd, &env, &["env", "show", "mira"]);
    assert_eq!(env_show.status.code(), Some(1));
    assert!(stderr(&env_show).contains("environment \"mira\" does not exist"));
}
