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
