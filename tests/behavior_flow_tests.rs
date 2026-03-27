mod support;

use std::fs;

use ocm::store::clean_path;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_text};

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

#[test]
fn env_run_overrides_parent_openclaw_environment_state() {
    let root = TestDir::new("behavior-env-run-parent-env");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "inspect", "--command", "sh"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create_demo = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "inspect"],
    );
    assert!(create_demo.status.success(), "{}", stderr(&create_demo));

    let create_test = run_ocm(
        &cwd,
        &env,
        &["env", "create", "test", "--launcher", "inspect"],
    );
    assert!(create_test.status.success(), "{}", stderr(&create_test));

    let demo_root = clean_path(&root.child("ocm-home/envs/demo"));
    env.insert("OPENCLAW_HOME".to_string(), demo_root.display().to_string());
    env.insert(
        "OPENCLAW_STATE_DIR".to_string(),
        demo_root.join(".openclaw").display().to_string(),
    );
    env.insert(
        "OPENCLAW_CONFIG_PATH".to_string(),
        demo_root
            .join(".openclaw/openclaw.json")
            .display()
            .to_string(),
    );
    env.insert("OCM_ACTIVE_ENV".to_string(), "demo".to_string());
    env.insert(
        "OCM_ACTIVE_ENV_ROOT".to_string(),
        demo_root.display().to_string(),
    );
    env.insert("OPENCLAW_PROFILE".to_string(), "legacy".to_string());

    let run_output = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "run",
            "test",
            "--",
            "-lc",
            "printf '%s|%s|%s|%s|%s' \"$OPENCLAW_HOME\" \"$OPENCLAW_STATE_DIR\" \"$OPENCLAW_CONFIG_PATH\" \"$OCM_ACTIVE_ENV\" \"${OPENCLAW_PROFILE:-unset}\"",
        ],
    );
    assert!(run_output.status.success(), "{}", stderr(&run_output));

    let test_root = clean_path(&root.child("ocm-home/envs/test"));
    assert_eq!(
        stdout(&run_output),
        format!(
            "{}|{}|{}|{}|{}",
            test_root.display(),
            test_root.join(".openclaw").display(),
            test_root.join(".openclaw/openclaw.json").display(),
            "test",
            "unset"
        )
    );
}

#[test]
fn root_double_dash_runs_openclaw_in_the_active_environment() {
    let root = TestDir::new("behavior-root-double-dash");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "inspect", "--command", "sh"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "inspect"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    env.insert("OCM_ACTIVE_ENV".to_string(), "demo".to_string());
    env.insert(
        "OPENCLAW_HOME".to_string(),
        "/tmp/legacy-openclaw-home".to_string(),
    );
    env.insert("OPENCLAW_PROFILE".to_string(), "legacy".to_string());

    let run_output = run_ocm(
        &cwd,
        &env,
        &[
            "--",
            "-lc",
            "printf '%s|%s|%s' \"$OCM_ACTIVE_ENV\" \"$OPENCLAW_HOME\" \"${OPENCLAW_PROFILE:-unset}\"",
        ],
    );
    assert!(run_output.status.success(), "{}", stderr(&run_output));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    assert_eq!(
        stdout(&run_output),
        format!("demo|{}|unset", env_root.display())
    );
}

#[test]
fn env_use_auto_assigns_distinct_gateway_ports_for_fresh_envs() {
    let root = TestDir::new("behavior-env-use-auto-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create_demo = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create_demo.status.success(), "{}", stderr(&create_demo));

    let create_test = run_ocm(&cwd, &env, &["env", "create", "test"]);
    assert!(create_test.status.success(), "{}", stderr(&create_test));

    let demo_use = run_ocm(&cwd, &env, &["env", "use", "demo"]);
    assert!(demo_use.status.success(), "{}", stderr(&demo_use));
    assert!(stdout(&demo_use).contains("export OPENCLAW_GATEWAY_PORT='18789'"));

    let test_use = run_ocm(&cwd, &env, &["env", "use", "test"]);
    assert!(test_use.status.success(), "{}", stderr(&test_use));
    assert!(stdout(&test_use).contains("export OPENCLAW_GATEWAY_PORT='18790'"));
}

#[test]
fn env_exec_skips_gateway_port_claimed_by_an_initialized_environment() {
    let root = TestDir::new("behavior-env-exec-config-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create_demo = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create_demo.status.success(), "{}", stderr(&create_demo));

    let create_test = run_ocm(&cwd, &env, &["env", "create", "test"]);
    assert!(create_test.status.success(), "{}", stderr(&create_test));

    let demo_root = root.child("ocm-home/envs/demo");
    write_text(
        &demo_root.join(".openclaw/openclaw.json"),
        "{\n  \"gateway\": {\n    \"port\": 18789\n  }\n}\n",
    );

    let exec_output = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "exec",
            "test",
            "--",
            "sh",
            "-lc",
            "printf '%s' \"$OPENCLAW_GATEWAY_PORT\"",
        ],
    );
    assert!(exec_output.status.success(), "{}", stderr(&exec_output));
    assert_eq!(stdout(&exec_output), "18790");
}
