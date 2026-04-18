mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::support::{
    TestDir, install_fake_launchctl, install_fake_systemd_tools, managed_service_definition_path,
    ocm_env, path_string, run_ocm, stderr, stdout, write_executable_script,
};

fn launchd_env(root: &TestDir) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    install_fake_launchctl(root, &mut env);
    env
}

fn systemd_env(root: &TestDir) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    install_fake_systemd_tools(root, &mut env);
    env
}

fn setup_launcher_env(cwd: &Path, env: &BTreeMap<String, String>) {
    let launcher = run_ocm(
        cwd,
        env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(cwd, env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
}

fn json_output(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn service_install_requires_a_target_env() {
    let root = TestDir::new("service-install-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "install"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("service install requires <env>"));
}

#[test]
fn service_lifecycle_commands_require_a_target_env() {
    let root = TestDir::new("service-lifecycle-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    for action in ["start", "stop", "restart", "uninstall"] {
        let output = run_ocm(&cwd, &env, &["service", action]);
        assert!(!output.status.success(), "{action} unexpectedly succeeded");
        assert!(stderr(&output).contains(&format!("service {action} requires <env>")));
    }
}

#[test]
fn service_status_requires_target_or_all() {
    let root = TestDir::new("service-status-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "status"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("service status requires <env> or --all"));
}

#[test]
fn unknown_service_commands_use_service_specific_errors() {
    let root = TestDir::new("service-unknown-command");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "wat"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("unknown service command: wat"));
}

#[test]
fn service_install_enables_the_env_and_installs_the_supervisor_daemon() {
    let root = TestDir::new("service-install");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], true);
    assert_eq!(body["desiredRunning"], false);

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_body = json_output(&env_show);
    assert_eq!(env_body["serviceEnabled"], true);
    assert_eq!(env_body["serviceRunning"], false);

    let supervisor_path = managed_service_definition_path(&env, &cwd, "supervisor");
    assert!(
        supervisor_path.exists(),
        "missing {}",
        path_string(&supervisor_path)
    );
}

#[test]
fn service_start_marks_the_env_running() {
    let root = TestDir::new("service-start");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let output = run_ocm(&cwd, &env, &["service", "start", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], true);
    assert_eq!(body["desiredRunning"], true);

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    let env_body = json_output(&env_show);
    assert_eq!(env_body["serviceEnabled"], true);
    assert_eq!(env_body["serviceRunning"], true);
}

#[test]
fn service_stop_keeps_the_env_installed_but_stopped() {
    let root = TestDir::new("service-stop");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let started = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(started.status.success(), "{}", stderr(&started));

    let output = run_ocm(&cwd, &env, &["service", "stop", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], true);
    assert_eq!(body["desiredRunning"], false);

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    let env_body = json_output(&env_show);
    assert_eq!(env_body["serviceEnabled"], true);
    assert_eq!(env_body["serviceRunning"], false);
}

#[test]
fn service_restart_restores_running_policy() {
    let root = TestDir::new("service-restart");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let installed = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(installed.status.success(), "{}", stderr(&installed));

    let output = run_ocm(&cwd, &env, &["service", "restart", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], true);
    assert_eq!(body["desiredRunning"], true);
}

#[test]
fn service_uninstall_disables_the_env_service() {
    let root = TestDir::new("service-uninstall");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let started = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(started.status.success(), "{}", stderr(&started));

    let output = run_ocm(&cwd, &env, &["service", "uninstall", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], false);
    assert_eq!(body["desiredRunning"], false);

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    let env_body = json_output(&env_show);
    assert_eq!(env_body["serviceEnabled"], false);
    assert_eq!(env_body["serviceRunning"], false);
}

#[test]
fn service_list_reports_env_and_supervisor_state_in_json() {
    let root = TestDir::new("service-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let started = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(started.status.success(), "{}", stderr(&started));

    let output = run_ocm(&cwd, &env, &["service", "list", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["daemonInstalled"], true);
    assert_eq!(body["services"][0]["envName"], "demo");
    assert_eq!(body["services"][0]["installed"], true);
    assert_eq!(body["services"][0]["desiredRunning"], true);
}

#[test]
fn service_logs_read_from_the_planned_child_log_paths() {
    let root = TestDir::new("service-logs");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    let installed = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(installed.status.success(), "{}", stderr(&installed));

    let status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let body = json_output(&status);
    let stdout_path = body["stdoutPath"].as_str().unwrap();
    let stderr_path = body["stderrPath"].as_str().unwrap();
    fs::write(stdout_path, "hello from stdout\n").unwrap();
    fs::write(stderr_path, "hello from stderr\n").unwrap();

    let stdout_log = run_ocm(&cwd, &env, &["service", "logs", "demo"]);
    assert!(stdout_log.status.success(), "{}", stderr(&stdout_log));
    assert_eq!(stdout(&stdout_log), "hello from stdout\n");

    let stderr_log = run_ocm(
        &cwd,
        &env,
        &["service", "logs", "demo", "--stderr", "--json"],
    );
    assert!(stderr_log.status.success(), "{}", stderr(&stderr_log));
    let stderr_body = json_output(&stderr_log);
    assert_eq!(stderr_body["content"], "hello from stderr\n");
}

#[test]
fn systemd_service_install_writes_the_supervisor_unit() {
    let root = TestDir::new("service-systemd-install");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = systemd_env(&root);
    setup_launcher_env(&cwd, &env);

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], true);

    let supervisor_path = managed_service_definition_path(&env, &cwd, "supervisor");
    assert!(
        supervisor_path.exists(),
        "missing {}",
        path_string(&supervisor_path)
    );
    let unit = fs::read_to_string(supervisor_path).unwrap();
    assert!(unit.contains("__daemon run"));
}

#[test]
fn service_start_requires_a_valid_binding() {
    let root = TestDir::new("service-start-binding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("has no default runtime or launcher"));
}

#[test]
fn service_status_reports_missing_binding_issue() {
    let root = TestDir::new("service-status-missing-binding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let started = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(!started.status.success());

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(!install.status.success());

    let status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let body = json_output(&status);
    assert!(
        body["issue"]
            .as_str()
            .unwrap()
            .contains("has no default runtime or launcher")
    );
}

#[test]
fn service_status_uses_runtime_bindings_too() {
    let root = TestDir::new("service-runtime-binding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let runtime_path = root.child("bin/openclaw");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "stable",
            "--path",
            &path_string(&runtime_path),
        ],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["bindingKind"], "runtime");
    assert_eq!(body["bindingName"], "stable");
    assert_eq!(body["binaryPath"], path_string(&runtime_path));
}
