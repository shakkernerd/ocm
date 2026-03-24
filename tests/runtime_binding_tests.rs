mod support;

use std::fs;

use ocm::paths::clean_path;

use crate::support::{
    TestDir, TestHttpServer, ocm_env, run_ocm, stderr, stdout, write_executable_script,
};

#[test]
fn env_set_runtime_updates_and_clears_the_default_runtime() {
    let root = TestDir::new("runtime-binding-set");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(&bin_dir.join("stable"), "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let bind = run_ocm(&cwd, &env, &["env", "set-runtime", "demo", "stable"]);
    assert!(bind.status.success(), "{}", stderr(&bind));
    assert!(stdout(&bind).contains("Updated env demo: defaultRuntime=stable"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("defaultRuntime: stable"));

    let clear = run_ocm(&cwd, &env, &["env", "set-runtime", "demo", "none"]);
    assert!(clear.status.success(), "{}", stderr(&clear));
    assert!(stdout(&clear).contains("Updated env demo: defaultRuntime=none"));

    let show_cleared = run_ocm(&cwd, &env, &["env", "show", "demo"]);
    assert!(show_cleared.status.success(), "{}", stderr(&show_cleared));
    assert!(!stdout(&show_cleared).contains("defaultRuntime:"));
}

#[test]
fn env_run_uses_the_registered_runtime_binary() {
    let root = TestDir::new("runtime-binding-run");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(
        &bin_dir.join("stable"),
        "#!/bin/sh\nprintf 'runtime|%s|%s' \"$OPENCLAW_HOME\" \"$PWD\"\n",
    );
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let run = run_ocm(&cwd, &env, &["env", "run", "demo", "--"]);
    assert!(run.status.success(), "{}", stderr(&run));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    let expected_cwd = fs::canonicalize(&cwd).unwrap();
    assert_eq!(
        stdout(&run),
        format!("runtime|{}|{}", env_root.display(), expected_cwd.display())
    );
}

#[test]
fn env_run_prefers_the_bound_runtime_over_the_bound_launcher() {
    let root = TestDir::new("runtime-binding-precedence");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    write_executable_script(&bin_dir.join("stable"), "#!/bin/sh\nprintf 'runtime'\n");
    let env = ocm_env(&root);

    let add_runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add_runtime.status.success(), "{}", stderr(&add_runtime));

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "fallback",
            "--command",
            "printf launcher",
        ],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "create",
            "demo",
            "--runtime",
            "stable",
            "--launcher",
            "fallback",
        ],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let run = run_ocm(&cwd, &env, &["env", "run", "demo", "--"]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run), "runtime");
}

#[test]
fn env_run_uses_a_runtime_installed_from_url() {
    let root = TestDir::new("runtime-binding-installed-url");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let server = TestHttpServer::serve_bytes(
        "/releases/openclaw-nightly",
        "application/octet-stream",
        b"#!/bin/sh\nprintf 'runtime-url|%s|%s' \"$OPENCLAW_HOME\" \"$PWD\"\n",
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "nightly", "--url", &server.url()],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "nightly"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let run = run_ocm(&cwd, &env, &["env", "run", "demo", "--"]);
    assert!(run.status.success(), "{}", stderr(&run));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    let expected_cwd = fs::canonicalize(&cwd).unwrap();
    assert_eq!(
        stdout(&run),
        format!(
            "runtime-url|{}|{}",
            env_root.display(),
            expected_cwd.display()
        )
    );
}
