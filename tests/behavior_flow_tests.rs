mod support;

use std::fs;

use ocm::paths::clean_path;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn env_use_prints_activation_exports_for_the_selected_environment() {
    let root = TestDir::new("behavior-env-use");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo", "--port", "19789"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let use_output = run_ocm(&cwd, &env, &["env", "use", "demo"]);
    assert!(use_output.status.success(), "{}", stderr(&use_output));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    let script = stdout(&use_output);
    assert!(script.contains("unset OPENCLAW_PROFILE"));
    assert!(script.contains(&format!("export OPENCLAW_HOME='{}'", env_root.display())));
    assert!(script.contains(&format!("export OPENCLAW_GATEWAY_PORT='{}'", 19789)));
}

#[test]
fn env_exec_injects_openclaw_environment_variables() {
    let root = TestDir::new("behavior-env-exec");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let exec_output = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "exec",
            "demo",
            "--",
            "sh",
            "-lc",
            "printf '%s|%s' \"$OPENCLAW_HOME\" \"${OPENCLAW_PROFILE:-unset}\"",
        ],
    );
    assert!(exec_output.status.success(), "{}", stderr(&exec_output));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    assert_eq!(
        stdout(&exec_output),
        format!("{}|unset", env_root.display())
    );
}

#[test]
fn env_run_uses_the_registered_launcher_and_its_cwd() {
    let root = TestDir::new("behavior-env-run");
    let cwd = root.child("workspace");
    let launcher_dir = cwd.join("launchers/stable");
    fs::create_dir_all(&launcher_dir).unwrap();
    let env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            "sh",
            "--cwd",
            "./launchers/stable",
        ],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let run_output = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "run",
            "demo",
            "--",
            "-lc",
            "printf '%s|%s' \"$PWD\" \"$OPENCLAW_HOME\"",
        ],
    );
    assert!(run_output.status.success(), "{}", stderr(&run_output));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    let expected_run_dir = fs::canonicalize(&launcher_dir).unwrap();
    assert_eq!(
        stdout(&run_output),
        format!("{}|{}", expected_run_dir.display(), env_root.display())
    );
}
