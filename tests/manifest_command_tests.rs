mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

fn install_fake_launchctl(root: &TestDir, env: &mut std::collections::BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("launchctl"), "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(bin_dir.join("launchctl"))
            .unwrap()
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(bin_dir.join("launchctl"), permissions).unwrap();
    }

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        bin_dir.display().to_string()
    } else {
        format!("{}:{existing_path}", bin_dir.display())
    };
    env.insert("PATH".to_string(), combined_path);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
}

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
    assert!(stdout.contains("\"current_service_installed\": false"));
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

#[test]
fn manifest_drift_reports_service_install_mismatch() {
    let root = TestDir::new("manifest-drift-service");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: false\n",
    )
    .unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let create = run_ocm(&cwd, &env, &["env", "create", "mira", "--launcher", "dev"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let install = run_ocm(&cwd, &env, &["service", "install", "mira"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let output = run_ocm(&cwd, &env, &["manifest", "drift", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"current_service_installed\": true"));
    assert!(body.contains("service differs (desired absent, current installed)"));
}

#[test]
fn manifest_plan_reports_create_work_for_missing_envs() {
    let root = TestDir::new("manifest-plan-missing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["manifest", "plan", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"found\": true"));
    assert!(stdout.contains("\"create_env\": true"));
    assert!(stdout.contains("\"desired_launcher\": \"dev\""));
    assert!(stdout.contains("\"desired_service_install\": true"));
}

#[test]
fn manifest_plan_reports_no_binding_change_when_launcher_matches() {
    let root = TestDir::new("manifest-plan-matching");
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

    let output = run_ocm(&cwd, &env, &["manifest", "plan", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("\"create_env\": false"));
    assert!(stdout.contains("\"launcher_changed\": false"));
}
