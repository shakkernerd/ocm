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
    assert!(body.contains("ocm migrate inspect"));
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
