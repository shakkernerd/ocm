mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn env_set_launcher_updates_and_clears_the_default_launcher() {
    let root = TestDir::new("launcher-binding-set");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "sh"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let bind = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "stable"]);
    assert!(bind.status.success(), "{}", stderr(&bind));
    assert!(stdout(&bind).contains("Updated env demo: defaultLauncher=stable"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultLauncher: stable"));

    let clear = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "none"]);
    assert!(clear.status.success(), "{}", stderr(&clear));
    assert!(stdout(&clear).contains("Updated env demo: defaultLauncher=none"));

    let show_cleared = run_ocm(&cwd, &env, &["env", "show", "demo"]);
    assert!(show_cleared.status.success(), "{}", stderr(&show_cleared));
    assert!(!stdout(&show_cleared).contains("defaultLauncher:"));
}

#[test]
fn env_run_without_a_launcher_uses_launcher_specific_guidance() {
    let root = TestDir::new("launcher-binding-run-error");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let run = run_ocm(&cwd, &env, &["env", "run", "demo", "--", "onboard"]);
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains(
        "environment \"demo\" has no default runtime or launcher; use env set-runtime, env set-launcher, or pass --runtime/--launcher",
    ));
}
