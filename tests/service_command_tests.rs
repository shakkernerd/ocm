mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use ocm::env::EnvironmentService;
use ocm::store::{now_utc, supervisor_runtime_path};
use ocm::supervisor::{SupervisorRuntimeChild, SupervisorRuntimeState};
use serde_json::Value;

use crate::support::{
    TestDir, install_fake_launchctl, install_fake_systemd_tools, managed_service_definition_path,
    ocm_env, path_string, run_ocm, stderr, write_executable_script,
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

fn unsupported_env(root: &TestDir) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "unsupported".to_string(),
    );
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
fn service_install_enables_the_env_and_installs_the_ocm_service() {
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

    let service_path = managed_service_definition_path(&env, &cwd, "ocm");
    assert!(
        service_path.exists(),
        "missing {}",
        path_string(&service_path)
    );
}

#[test]
fn service_stop_and_uninstall_do_not_require_a_managed_service_backend() {
    let root = TestDir::new("service-stop-uninstall-unsupported");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = unsupported_env(&root);
    setup_launcher_env(&cwd, &env);

    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo", "--json"]);
    assert!(stop.status.success(), "{}", stderr(&stop));
    let stop_body = json_output(&stop);
    assert_eq!(stop_body["installed"], true);
    assert_eq!(stop_body["desiredRunning"], false);

    let uninstall = run_ocm(&cwd, &env, &["service", "uninstall", "demo", "--json"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));
    let uninstall_body = json_output(&uninstall);
    assert_eq!(uninstall_body["installed"], false);
    assert_eq!(uninstall_body["desiredRunning"], false);
}

#[test]
fn service_start_still_requires_a_managed_service_backend() {
    let root = TestDir::new("service-start-unsupported");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = unsupported_env(&root);
    setup_launcher_env(&cwd, &env);

    let output = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("managed services are not supported on this platform yet"));
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
fn service_list_reports_env_and_ocm_service_state_in_json() {
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
    assert_eq!(body["ocmServiceInstalled"], true);
    assert_eq!(body["services"][0]["envName"], "demo");
    assert_eq!(body["services"][0]["installed"], true);
    assert_eq!(body["services"][0]["desiredRunning"], true);
}

#[test]
fn systemd_service_install_writes_the_ocm_unit() {
    let root = TestDir::new("service-systemd-install");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = systemd_env(&root);
    setup_launcher_env(&cwd, &env);

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["installed"], true);

    let service_path = managed_service_definition_path(&env, &cwd, "ocm");
    assert!(
        service_path.exists(),
        "missing {}",
        path_string(&service_path)
    );
    let unit = fs::read_to_string(service_path).unwrap();
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
    assert!(stderr(&output).contains("has no default runtime, launcher, or dev binding"));
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
            .contains("has no default runtime, launcher, or dev binding")
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

#[test]
fn service_status_keeps_simple_package_manager_launchers_as_direct_exec() {
    let root = TestDir::new("service-status-package-manager-launcher");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "pnpm openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "dev"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let body = json_output(&status);
    assert_eq!(body["bindingKind"], "launcher");
    assert_eq!(body["binaryPath"], "pnpm");
    let gateway_port = body["gatewayPort"].as_u64().unwrap().to_string();
    assert_eq!(
        body["args"],
        Value::Array(vec![
            Value::String("openclaw".to_string()),
            Value::String("gateway".to_string()),
            Value::String("run".to_string()),
            Value::String("--port".to_string()),
            Value::String(gateway_port),
        ])
    );
}

#[test]
fn service_status_ignores_stale_runtime_children_when_the_daemon_is_down() {
    let root = TestDir::new("service-status-stale-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = launchd_env(&root);
    setup_launcher_env(&cwd, &env);

    EnvironmentService::new(&env, &cwd)
        .set_service_policy("demo", Some(true), Some(true))
        .unwrap();

    let runtime_path = supervisor_runtime_path(&env, &cwd).unwrap();
    fs::create_dir_all(runtime_path.parent().unwrap()).unwrap();
    let stale_runtime = SupervisorRuntimeState {
        kind: "ocm-supervisor-runtime".to_string(),
        ocm_home: path_string(&root.child("ocm-home")),
        updated_at: now_utc(),
        services: Vec::new(),
        children: vec![SupervisorRuntimeChild {
            env_name: "demo".to_string(),
            binding_kind: "launcher".to_string(),
            binding_name: "stable".to_string(),
            pid: 4242,
            restart_count: 3,
            child_port: 18789,
            stdout_path: path_string(&root.child("stale.stdout.log")),
            stderr_path: path_string(&root.child("stale.stderr.log")),
        }],
    };
    fs::write(&runtime_path, serde_json::to_vec(&stale_runtime).unwrap()).unwrap();

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body = json_output(&output);
    assert_eq!(body["running"], false);
    assert_eq!(body["childPid"], Value::Null);
    assert_eq!(body["childRestartCount"], Value::Null);
    assert!(
        body["issue"]
            .as_str()
            .unwrap()
            .contains("OCM background service is not installed")
    );
}
