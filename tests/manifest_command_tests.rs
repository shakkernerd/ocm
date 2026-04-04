mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn manifest_path_finds_the_nearest_manifest_from_the_current_directory() {
    let root = TestDir::new("manifest-path-current");
    let cwd = root.child("workspace").join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "path", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": true"));
    assert!(
        stdout.contains(
            &root
                .child("workspace")
                .join("ocm.yaml")
                .to_string_lossy()
                .into_owned()
        )
    );
}

#[test]
fn manifest_path_can_search_from_an_explicit_path() {
    let root = TestDir::new("manifest-path-explicit");
    let cwd = root.child("workspace");
    let nested = root.child("project").join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&nested).unwrap();
    fs::write(
        root.child("project").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "manifest",
            "path",
            nested.to_string_lossy().as_ref(),
            "--raw",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("found: true"));
    assert!(
        stdout.contains(
            &root
                .child("project")
                .join("ocm.yaml")
                .to_string_lossy()
                .into_owned()
        )
    );
}

#[test]
fn manifest_path_reports_when_no_manifest_exists() {
    let root = TestDir::new("manifest-path-missing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "path", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": false"));
    assert!(stdout.contains("\"path\": null"));
}

#[test]
fn manifest_group_help_is_available() {
    let root = TestDir::new("manifest-help-group");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "manifest"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("Manifest commands"));
    assert!(stdout.contains("ocm manifest path"));
    assert!(stdout.contains("ocm manifest show"));
}

#[test]
fn manifest_show_prints_the_discovered_manifest() {
    let root = TestDir::new("manifest-show");
    let cwd = root.child("workspace").join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\nservice:\n  install: true\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "show", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": true"));
    assert!(stdout.contains("\"schema\": \"ocm/v1\""));
    assert!(stdout.contains("\"name\": \"mira\""));
    assert!(stdout.contains("\"channel\": \"stable\""));
}

#[test]
fn manifest_show_reports_when_no_manifest_exists() {
    let root = TestDir::new("manifest-show-missing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "show", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": false"));
    assert!(stdout.contains("\"manifest\": null"));
}

#[test]
fn manifest_resolve_reports_the_target_env_and_current_state() {
    let root = TestDir::new("manifest-resolve");
    let cwd = root.child("workspace").join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  channel: stable\nservice:\n  install: true\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let output = run_ocm(&cwd, &env, &["manifest", "resolve", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": true"));
    assert!(stdout.contains("\"env_name\": \"mira\""));
    assert!(stdout.contains("\"env_exists\": true"));
    assert!(stdout.contains("\"desired_runtime\": \"stable\""));
    assert!(stdout.contains("\"desired_launcher\": null"));
}

#[test]
fn manifest_resolve_reports_when_no_manifest_exists() {
    let root = TestDir::new("manifest-resolve-missing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "resolve", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": false"));
    assert!(stdout.contains("\"env_name\": null"));
}

#[test]
fn manifest_drift_reports_missing_envs() {
    let root = TestDir::new("manifest-drift-missing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "drift", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": true"));
    assert!(stdout.contains("\"env_exists\": false"));
    assert!(stdout.contains("\"aligned\": false"));
    assert!(stdout.contains("env is missing"));
}

#[test]
fn manifest_drift_reports_alignment_for_matching_bindings() {
    let root = TestDir::new("manifest-drift-aligned");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let set_launcher = run_ocm(&cwd, &env, &["env", "set-launcher", "mira", "dev"]);
    assert!(set_launcher.status.success(), "{}", stderr(&set_launcher));

    let output = run_ocm(&cwd, &env, &["manifest", "drift", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"aligned\": true"));
    assert!(stdout.contains("\"issues\": []"));
    assert!(stdout.contains("\"desired_runtime\": null"));
}
