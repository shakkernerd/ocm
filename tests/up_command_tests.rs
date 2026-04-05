mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

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

    let show = run_ocm(&cwd, &env, &["env", "show", "mira", "--json"]);
    assert!(!show.status.success());
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
fn help_up_is_available() {
    let root = TestDir::new("up-help");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "up"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Apply a manifest"));
    assert!(body.contains("ocm up [path] [--dry-run] [--raw] [--json]"));
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
