mod support;

use std::fs;

use crate::support::{
    TestDir, install_fake_service_manager, ocm_env, run_ocm, stderr, stdout,
    write_executable_script,
};

#[test]
fn up_dry_run_reports_the_manifest_plan_without_creating_the_env() {
    let root = TestDir::new("up-dry-run");
    let cwd = root.child("workspace").join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        root.child("workspace").join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["up", "--dry-run", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"dry_run\": true"));
    assert!(body.contains("\"create_env\": true"));
    assert!(body.contains("\"desired_launcher\": \"dev\""));
    assert!(body.contains("\"service_changed\": true"));

    let show = run_ocm(&cwd, &env, &["env", "show", "mira", "--json"]);
    assert!(!show.status.success());
}

#[test]
fn up_dry_run_accepts_an_explicit_manifest_path() {
    let root = TestDir::new("up-dry-run-manifest-path");
    let repo = root.child("workspace");
    let cwd = repo.join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        repo.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &["up", "--manifest", "../ocm.yaml", "--dry-run", "--json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"dry_run\": true"));
    assert!(body.contains("\"path\":"));
    assert!(body.contains("ocm.yaml"));
}

#[test]
fn up_dry_run_accepts_an_explicit_custom_manifest_file() {
    let root = TestDir::new("up-dry-run-manifest-file");
    let repo = root.child("workspace");
    let cwd = repo.join("deep");
    let manifest_path = repo.join("mira.yaml");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        &manifest_path,
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "up",
            "--manifest",
            manifest_path.to_string_lossy().as_ref(),
            "--dry-run",
            "--json",
        ],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"dry_run\": true"));
    assert!(body.contains(&manifest_path.to_string_lossy().to_string()));
    assert!(body.contains("\"desired_launcher\": \"dev\""));
}

#[test]
fn up_dry_run_reports_no_service_change_when_install_state_matches() {
    let root = TestDir::new("up-dry-run-service-matching");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();
    let mut env = ocm_env(&root);
    install_fake_service_manager(&root, &mut env);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));
    let create = run_ocm(&cwd, &env, &["env", "create", "mira", "--launcher", "dev"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let install = run_ocm(&cwd, &env, &["service", "install", "mira"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let output = run_ocm(&cwd, &env, &["up", "--dry-run", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"service_changed\": false"));
}

#[test]
fn up_rejects_path_and_manifest_together() {
    let root = TestDir::new("up-manifest-conflict");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["up", ".", "--manifest", "./ocm.yaml"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("up accepts only one of [path] or --manifest <path>"));
}

#[test]
fn up_reports_missing_explicit_manifest_files() {
    let root = TestDir::new("up-missing-manifest-file");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["up", "--manifest", "./missing.yaml"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("manifest file does not exist:"));
    assert!(stderr(&output).contains("missing.yaml"));
}

#[test]
fn up_creates_the_env_and_applies_the_launcher_binding() {
    let root = TestDir::new("up-apply-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let output = run_ocm(&cwd, &env, &["up", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"dry_run\": false"));
    assert!(body.contains("\"env_created\": true"));
    assert!(body.contains("\"launcher_changed\": true"));

    let show = run_ocm(&cwd, &env, &["env", "show", "mira"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultLauncher: dev"));
}

#[test]
fn up_rolls_back_partial_changes_when_later_reconcile_steps_fail() {
    let root = TestDir::new("up-rollback");
    let cwd = root.child("workspace");
    let fake_home = root.child("fake-home-file");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(&fake_home, "not-a-directory").unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\nservice:\n  install: true\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "printf launcher"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let mut failing_env = env.clone();
    failing_env.insert("HOME".to_string(), fake_home.display().to_string());
    failing_env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );

    let output = run_ocm(&cwd, &failing_env, &["up", "--json"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("rolled back env \"mira\" from snapshot"));

    let show = run_ocm(&cwd, &env, &["env", "show", "mira", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("\"defaultLauncher\": null"));

    let status = run_ocm(&cwd, &env, &["service", "status", "mira", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    assert!(stdout(&status).contains("\"installed\": false"));
}

#[test]
fn help_up_is_available() {
    let root = TestDir::new("up-help");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "up"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Apply a manifest"));
    assert!(body.contains("ocm up [path] [--manifest <path>] [--dry-run] [--raw] [--json]"));
    assert!(body.contains("--manifest <path>"));
    assert!(body.contains("snapshots that env first and rolls it back"));
    assert!(body.contains(
        "Relative manifest file paths passed through `--manifest` are resolved from the current working directory."
    ));
}

#[test]
fn up_can_bind_a_registered_runtime() {
    let root = TestDir::new("up-apply-runtime");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nruntime:\n  name: stable\n",
    )
    .unwrap();
    write_executable_script(&bin_dir.join("stable"), "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add_runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add_runtime.status.success(), "{}", stderr(&add_runtime));

    let output = run_ocm(&cwd, &env, &["up", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"runtime_changed\": true"));

    let show = run_ocm(&cwd, &env, &["env", "show", "mira"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultRuntime: stable"));
}
