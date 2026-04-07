mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

#[test]
fn sync_dry_run_requires_an_existing_env() {
    let root = TestDir::new("sync-dry-run-missing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["sync", "--dry-run"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("manifest env \"mira\" does not exist yet"));
}

#[test]
fn sync_dry_run_reports_the_existing_env_plan() {
    let root = TestDir::new("sync-dry-run");
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
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let output = run_ocm(&cwd, &env, &["sync", "--dry-run", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"dry_run\": true"));
    assert!(body.contains("\"create_env\": false"));
    assert!(body.contains("\"desired_launcher\": \"dev\""));
}

#[test]
fn sync_dry_run_accepts_an_explicit_manifest_path() {
    let root = TestDir::new("sync-dry-run-manifest-path");
    let repo = root.child("workspace");
    let cwd = repo.join("deep");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        repo.join("ocm.yaml"),
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
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let output = run_ocm(
        &cwd,
        &env,
        &["sync", "--manifest", "./ocm.yaml", "--dry-run", "--json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"dry_run\": true"));
    assert!(body.contains("\"path\":"));
    assert!(body.contains("ocm.yaml"));
}

#[test]
fn sync_rejects_path_and_manifest_together() {
    let root = TestDir::new("sync-manifest-conflict");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    fs::write(
        cwd.join("ocm.yaml"),
        "schema: ocm/v1\nenv:\n  name: mira\nlauncher:\n  name: dev\n",
    )
    .unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["sync", ".", "--manifest", "./ocm.yaml"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("sync accepts only one of [path] or --manifest <path>"));
}

#[test]
fn sync_applies_runtime_binding_to_an_existing_env() {
    let root = TestDir::new("sync-runtime");
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
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let output = run_ocm(&cwd, &env, &["sync", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("\"runtime_changed\": true"));

    let show = run_ocm(&cwd, &env, &["env", "show", "mira"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultRuntime: stable"));
}

#[test]
fn help_sync_is_available() {
    let root = TestDir::new("sync-help");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["help", "sync"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = stdout(&output);
    assert!(body.contains("Synchronize an existing env from a manifest"));
    assert!(body.contains("ocm sync [path] [--manifest <path>] [--dry-run] [--raw] [--json]"));
    assert!(body.contains("--manifest <path>"));
    assert!(body.contains("snapshots that env first and rolls it back"));
}
