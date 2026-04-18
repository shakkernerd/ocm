mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use ocm::env::EnvironmentService;
use serde_json::Value;

use crate::support::{
    TestDir, install_fake_service_manager, managed_service_definition_path, ocm_env, path_string,
    run_ocm, stderr, write_executable_script,
};

fn spawn_supervisor_process(cwd: &Path, env: &BTreeMap<String, String>) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ocm"));
    command.current_dir(cwd);
    command.args(["supervisor", "run"]);
    command.env_clear();
    command.envs(env);
    command.stdout(Stdio::null());
    command.stderr(Stdio::null());
    command.spawn().unwrap()
}

fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        sleep(Duration::from_millis(50));
    }
    false
}

fn wait_for_runtime_children(
    path: &Path,
    expected_children: usize,
    env_name: Option<&str>,
    timeout: Duration,
) -> Option<Value> {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Ok(raw) = fs::read(path)
            && let Ok(body) = serde_json::from_slice::<Value>(&raw)
            && body["children"].as_array().map(|children| children.len()) == Some(expected_children)
        {
            let matches_env = env_name.is_none_or(|name| {
                body["children"]
                    .as_array()
                    .unwrap_or(&Vec::new())
                    .iter()
                    .any(|child| child["envName"] == name)
            });
            if matches_env {
                return Some(body);
            }
        }
        sleep(Duration::from_millis(50));
    }
    None
}

fn stop_process(child: &mut Child) {
    let _ = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status();
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn set_service_enabled(cwd: &Path, env: &BTreeMap<String, String>, name: &str, enabled: bool) {
    let service = EnvironmentService::new(env, cwd);
    service.set_service_enabled(name, enabled).unwrap();
    service.set_service_running(name, enabled).unwrap();
}

fn setup_supervisor_fixture(
    root: &TestDir,
) -> (
    std::path::PathBuf,
    std::collections::BTreeMap<String, String>,
) {
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let runtime_path = root.child("bin/openclaw");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "managed",
            "--path",
            &path_string(&runtime_path),
        ],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let demo = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "dev"]);
    assert!(demo.status.success(), "{}", stderr(&demo));
    set_service_enabled(&cwd, &env, "demo", true);

    let prod = run_ocm(
        &cwd,
        &env,
        &["env", "create", "prod", "--runtime", "managed"],
    );
    assert!(prod.status.success(), "{}", stderr(&prod));
    set_service_enabled(&cwd, &env, "prod", true);

    let bare = run_ocm(&cwd, &env, &["env", "create", "bare"]);
    assert!(bare.status.success(), "{}", stderr(&bare));

    (cwd, env)
}

fn setup_supervisor_run_fixture(
    root: &TestDir,
) -> (
    std::path::PathBuf,
    std::collections::BTreeMap<String, String>,
    std::path::PathBuf,
    std::path::PathBuf,
) {
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(root);

    let launcher_marker = root.child("launcher-ran.txt");
    let runtime_marker = root.child("runtime-ran.txt");

    let launcher_script = root.child("bin/launcher-openclaw");
    write_executable_script(
        &launcher_script,
        &format!(
            "#!/bin/sh\nprintf 'launcher\\n' > '{}'\nprintf 'launcher stdout\\n'\nprintf 'launcher stderr\\n' >&2\n",
            path_string(&launcher_marker)
        ),
    );
    let runtime_script = root.child("bin/runtime-openclaw");
    write_executable_script(
        &runtime_script,
        &format!(
            "#!/bin/sh\nprintf 'runtime\\n' > '{}'\nprintf 'runtime stdout\\n'\nprintf 'runtime stderr\\n' >&2\n",
            path_string(&runtime_marker)
        ),
    );

    let launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            &path_string(&launcher_script),
        ],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let runtime = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "managed",
            "--path",
            &path_string(&runtime_script),
        ],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let demo = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "dev"]);
    assert!(demo.status.success(), "{}", stderr(&demo));
    set_service_enabled(&cwd, &env, "demo", true);

    let prod = run_ocm(
        &cwd,
        &env,
        &["env", "create", "prod", "--runtime", "managed"],
    );
    assert!(prod.status.success(), "{}", stderr(&prod));
    set_service_enabled(&cwd, &env, "prod", true);

    (cwd, env, launcher_marker, runtime_marker)
}

#[test]
fn supervisor_plan_reports_runnable_children_and_skipped_envs() {
    let root = TestDir::new("supervisor-plan");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let output = run_ocm(&cwd, &env, &["supervisor", "plan", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["persisted"], false);
    assert!(
        body["statePath"]
            .as_str()
            .unwrap()
            .ends_with("/supervisor/state.json")
    );

    let children = body["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);

    let demo = children
        .iter()
        .find(|child| child["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingKind"], "launcher");
    assert_eq!(demo["startMode"], "on-demand");
    assert_eq!(demo["childPort"], 18789);
    assert!(
        demo["processEnv"]["OPENCLAW_HOME"]
            .as_str()
            .unwrap()
            .contains("/envs/demo")
    );

    let prod = children
        .iter()
        .find(|child| child["envName"] == "prod")
        .unwrap();
    assert_eq!(prod["bindingKind"], "runtime");
    assert_eq!(prod["bindingName"], "managed");
    assert!(
        prod["binaryPath"]
            .as_str()
            .unwrap()
            .ends_with("/bin/openclaw")
    );

    let skipped = body["skippedEnvs"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0]["envName"], "bare");
    assert_eq!(skipped[0]["reason"], "service is disabled");
}

#[test]
fn supervisor_plan_skips_enabled_envs_that_are_stopped() {
    let root = TestDir::new("supervisor-plan-stopped");
    let (cwd, env) = setup_supervisor_fixture(&root);
    EnvironmentService::new(&env, &cwd)
        .set_service_running("demo", false)
        .unwrap();

    let output = run_ocm(&cwd, &env, &["supervisor", "plan", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "demo")
    );
    assert!(
        body["skippedEnvs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["envName"] == "demo" && entry["reason"] == "service is stopped")
    );
}

#[test]
fn supervisor_run_persists_live_runtime_children() {
    let root = TestDir::new("supervisor-run-runtime-state");
    let (cwd, env, _, _) = setup_supervisor_run_fixture(&root);
    let runtime_path = root.child("ocm-home/supervisor/runtime.json");

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let mut supervisor = spawn_supervisor_process(&cwd, &env);
    let runtime = wait_for_runtime_children(&runtime_path, 2, Some("demo"), Duration::from_secs(5))
        .expect("supervisor runtime state did not report running children");
    assert_eq!(runtime["kind"], "ocm-supervisor-runtime");
    let demo = runtime["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["envName"] == "demo")
        .unwrap();
    assert!(demo["pid"].as_u64().unwrap() > 0);
    assert_eq!(demo["restartCount"], 0);

    let runtime_command = run_ocm(&cwd, &env, &["supervisor", "runtime", "--json"]);
    assert!(
        runtime_command.status.success(),
        "{}",
        stderr(&runtime_command)
    );
    let runtime_body: Value = serde_json::from_slice(&runtime_command.stdout).unwrap();
    assert_eq!(runtime_body["present"], true);
    assert_eq!(runtime_body["runtimePath"], path_string(&runtime_path));
    assert_eq!(runtime_body["children"].as_array().unwrap().len(), 2);

    stop_process(&mut supervisor);
    let cleared = wait_for_runtime_children(&runtime_path, 0, None, Duration::from_secs(5))
        .expect("supervisor runtime state did not clear after shutdown");
    assert!(cleared["updatedAt"].as_str().is_some());
}

#[test]
fn supervisor_runtime_reports_missing_state_before_the_daemon_runs() {
    let root = TestDir::new("supervisor-runtime-missing");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let output = run_ocm(&cwd, &env, &["supervisor", "runtime", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["present"], false);
    assert_eq!(body["children"], serde_json::json!([]));
    assert!(
        body["runtimePath"]
            .as_str()
            .unwrap()
            .ends_with("/supervisor/runtime.json")
    );
}

#[test]
fn supervisor_sync_persists_and_show_reads_the_state() {
    let root = TestDir::new("supervisor-sync");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync", "--json"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_body: Value = serde_json::from_slice(&sync.stdout).unwrap();
    assert_eq!(sync_body["persisted"], true);

    let state_path = sync_body["statePath"].as_str().unwrap();
    assert!(
        fs::metadata(state_path).is_ok(),
        "missing state file at {state_path}"
    );

    let show = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_body: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(show_body["persisted"], true);
    assert_eq!(show_body["statePath"], sync_body["statePath"]);
    assert_eq!(show_body["children"], sync_body["children"]);
    assert_eq!(show_body["skippedEnvs"], sync_body["skippedEnvs"]);
}

#[test]
fn supervisor_run_once_executes_planned_children() {
    let root = TestDir::new("supervisor-run-once");
    let (cwd, env, launcher_marker, runtime_marker) = setup_supervisor_run_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let run = run_ocm(&cwd, &env, &["supervisor", "run", "--once", "--json"]);
    assert!(run.status.success(), "{}", stderr(&run));
    let body: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(body["once"], true);
    assert_eq!(body["childCount"], 2);
    assert_eq!(body["childResults"].as_array().unwrap().len(), 2);
    assert!(
        body["childResults"]
            .as_array()
            .unwrap()
            .iter()
            .all(|result| result["success"] == true)
    );

    assert_eq!(fs::read_to_string(launcher_marker).unwrap(), "launcher\n");
    assert_eq!(fs::read_to_string(runtime_marker).unwrap(), "runtime\n");
}

#[test]
fn supervisor_logs_read_stdout_and_stderr_from_persisted_child_paths() {
    let root = TestDir::new("supervisor-logs");
    let (cwd, env, _, _) = setup_supervisor_run_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let run = run_ocm(&cwd, &env, &["supervisor", "run", "--once"]);
    assert!(run.status.success(), "{}", stderr(&run));

    let stdout_log = run_ocm(&cwd, &env, &["supervisor", "logs", "demo", "--json"]);
    assert!(stdout_log.status.success(), "{}", stderr(&stdout_log));
    let stdout_body: Value = serde_json::from_slice(&stdout_log.stdout).unwrap();
    assert_eq!(stdout_body["stream"], "stdout");
    assert!(
        stdout_body["content"]
            .as_str()
            .unwrap()
            .contains("launcher stdout")
    );

    let stderr_log = run_ocm(
        &cwd,
        &env,
        &[
            "supervisor",
            "logs",
            "prod",
            "--stderr",
            "--tail",
            "1",
            "--json",
        ],
    );
    assert!(stderr_log.status.success(), "{}", stderr(&stderr_log));
    let stderr_body: Value = serde_json::from_slice(&stderr_log.stdout).unwrap();
    assert_eq!(stderr_body["stream"], "stderr");
    assert_eq!(stderr_body["content"], "runtime stderr\n");
}

#[test]
fn supervisor_drift_reports_missing_and_changed_state() {
    let root = TestDir::new("supervisor-status");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let before = run_ocm(&cwd, &env, &["supervisor", "drift", "--json"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_body: Value = serde_json::from_slice(&before.stdout).unwrap();
    assert_eq!(before_body["statePresent"], false);
    assert_eq!(before_body["inSync"], false);
    assert_eq!(before_body["missingChildren"].as_array().unwrap().len(), 2);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync", "--json"]);
    assert!(sync.status.success(), "{}", stderr(&sync));
    let sync_body: Value = serde_json::from_slice(&sync.stdout).unwrap();
    let state_path = sync_body["statePath"].as_str().unwrap();

    let after_sync = run_ocm(&cwd, &env, &["supervisor", "drift", "--json"]);
    assert!(after_sync.status.success(), "{}", stderr(&after_sync));
    let after_sync_body: Value = serde_json::from_slice(&after_sync.stdout).unwrap();
    assert_eq!(after_sync_body["statePresent"], true);
    assert_eq!(after_sync_body["inSync"], true);
    assert!(
        after_sync_body["changedChildren"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let mut persisted: Value = serde_json::from_slice(&fs::read(state_path).unwrap()).unwrap();
    let children = persisted["children"].as_array_mut().unwrap();
    children.retain(|child| child["envName"] != "prod");
    fs::write(state_path, serde_json::to_vec_pretty(&persisted).unwrap()).unwrap();

    let after_change = run_ocm(&cwd, &env, &["supervisor", "drift", "--json"]);
    assert!(after_change.status.success(), "{}", stderr(&after_change));
    let after_change_body: Value = serde_json::from_slice(&after_change.stdout).unwrap();
    assert_eq!(after_change_body["inSync"], false);
    assert_eq!(
        after_change_body["missingChildren"],
        serde_json::json!(["prod"])
    );
}

#[test]
fn env_binding_changes_refresh_persisted_supervisor_state() {
    let root = TestDir::new("supervisor-auto-refresh-env-binding");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev-b", "--command", "openclaw --beta"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));
    let rebind = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev-b"]);
    assert!(rebind.status.success(), "{}", stderr(&rebind));

    let show = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_body: Value = serde_json::from_slice(&show.stdout).unwrap();
    let demo = show_body["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingName"], "dev-b");

    let status = run_ocm(&cwd, &env, &["supervisor", "drift", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_body["inSync"], true);
}

#[test]
fn env_create_and_remove_refresh_persisted_supervisor_state() {
    let root = TestDir::new("supervisor-auto-refresh-env-shape");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let created = run_ocm(&cwd, &env, &["env", "create", "extra", "--launcher", "dev"]);
    assert!(created.status.success(), "{}", stderr(&created));
    set_service_enabled(&cwd, &env, "extra", true);

    let show_after_create = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(
        show_after_create.status.success(),
        "{}",
        stderr(&show_after_create)
    );
    let created_body: Value = serde_json::from_slice(&show_after_create.stdout).unwrap();
    assert!(
        created_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "extra")
    );

    let removed = run_ocm(&cwd, &env, &["env", "remove", "extra", "--force"]);
    assert!(removed.status.success(), "{}", stderr(&removed));

    let show_after_remove = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(
        show_after_remove.status.success(),
        "{}",
        stderr(&show_after_remove)
    );
    let removed_body: Value = serde_json::from_slice(&show_after_remove.stdout).unwrap();
    assert!(
        !removed_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "extra")
    );
}

#[test]
fn launcher_removal_refreshes_persisted_supervisor_state() {
    let root = TestDir::new("supervisor-auto-refresh-launcher");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let removed = run_ocm(&cwd, &env, &["launcher", "remove", "dev"]);
    assert!(removed.status.success(), "{}", stderr(&removed));

    let show = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_body: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert!(
        !show_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "demo")
    );
    assert!(
        show_body["skippedEnvs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["envName"] == "demo")
    );

    let status = run_ocm(&cwd, &env, &["supervisor", "drift", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_body["inSync"], true);
}

#[test]
fn runtime_removal_refreshes_persisted_supervisor_state() {
    let root = TestDir::new("supervisor-auto-refresh-runtime");
    let (cwd, env) = setup_supervisor_fixture(&root);

    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let removed = run_ocm(&cwd, &env, &["runtime", "remove", "managed"]);
    assert!(removed.status.success(), "{}", stderr(&removed));

    let show = run_ocm(&cwd, &env, &["supervisor", "show", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_body: Value = serde_json::from_slice(&show.stdout).unwrap();
    assert!(
        !show_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "prod")
    );
    assert!(
        show_body["skippedEnvs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["envName"] == "prod")
    );

    let status = run_ocm(&cwd, &env, &["supervisor", "drift", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_body["inSync"], true);
}

#[test]
fn supervisor_install_writes_and_starts_the_managed_daemon() {
    let root = TestDir::new("supervisor-daemon-install");
    let (cwd, mut env) = setup_supervisor_fixture(&root);
    install_fake_service_manager(&root, &mut env);

    let install = run_ocm(&cwd, &env, &["supervisor", "install", "--json"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let body: Value = serde_json::from_slice(&install.stdout).unwrap();
    let definition_path = managed_service_definition_path(&env, &cwd, "supervisor");
    let managed_label = body["managedLabel"].as_str().unwrap();

    assert_eq!(body["action"], "install");
    assert_eq!(body["definitionPath"], path_string(&definition_path));
    assert_eq!(body["installed"], true);
    assert_eq!(body["loaded"], true);
    assert_eq!(body["running"], true);
    assert!(managed_label.ends_with(".supervisor"));
    assert!(fs::metadata(&definition_path).is_ok());

    let definition = fs::read_to_string(&definition_path).unwrap();
    assert!(definition.contains("supervisor"));
    assert!(definition.contains("run"));
    assert!(definition.contains(env!("CARGO_BIN_EXE_ocm")));

    let status = run_ocm(&cwd, &env, &["supervisor", "status", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_body["action"], "status");
    assert_eq!(status_body["managedLabel"], body["managedLabel"]);
    assert_eq!(status_body["installed"], true);

    if cfg!(target_os = "macos") {
        let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
        assert!(launchctl_log.contains("bootstrap gui/"));
        assert!(launchctl_log.contains(managed_label));
    } else {
        let systemctl_log = fs::read_to_string(root.child("systemctl.log")).unwrap();
        assert!(systemctl_log.contains(&format!("--user enable {managed_label}")));
        assert!(systemctl_log.contains(&format!("--user restart {managed_label}")));
    }
}

#[test]
fn supervisor_stop_and_uninstall_manage_the_daemon_definition() {
    let root = TestDir::new("supervisor-daemon-uninstall");
    let (cwd, mut env) = setup_supervisor_fixture(&root);
    install_fake_service_manager(&root, &mut env);

    let install = run_ocm(&cwd, &env, &["supervisor", "install", "--json"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let install_body: Value = serde_json::from_slice(&install.stdout).unwrap();
    let definition_path = managed_service_definition_path(&env, &cwd, "supervisor");
    let managed_label = install_body["managedLabel"].as_str().unwrap();

    let stop = run_ocm(&cwd, &env, &["supervisor", "stop", "--json"]);
    assert!(stop.status.success(), "{}", stderr(&stop));
    let stop_body: Value = serde_json::from_slice(&stop.stdout).unwrap();
    assert_eq!(stop_body["action"], "stop");
    assert_eq!(stop_body["managedLabel"], install_body["managedLabel"]);

    let uninstall = run_ocm(&cwd, &env, &["supervisor", "uninstall", "--json"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));
    let uninstall_body: Value = serde_json::from_slice(&uninstall.stdout).unwrap();
    assert_eq!(uninstall_body["action"], "uninstall");
    assert_eq!(uninstall_body["installed"], false);
    assert!(!definition_path.exists());

    let status = run_ocm(&cwd, &env, &["supervisor", "status", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let status_body: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_body["installed"], false);

    if cfg!(target_os = "macos") {
        let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
        assert!(launchctl_log.contains("bootout gui/"));
        assert!(launchctl_log.contains(managed_label));
    } else {
        let systemctl_log = fs::read_to_string(root.child("systemctl.log")).unwrap();
        assert!(systemctl_log.contains(&format!("--user stop {managed_label}")));
        assert!(systemctl_log.contains(&format!("--user disable --now {managed_label}")));
    }
}

#[test]
fn supervisor_run_reconciles_state_changes_without_a_restart() {
    let root = TestDir::new("supervisor-run-reconcile");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let old_started = root.child("old-started.txt");
    let old_stopped = root.child("old-stopped.txt");
    let new_started = root.child("new-started.txt");

    let old_script = root.child("bin/launcher-old");
    write_executable_script(
        &old_script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'printf \"stopped\\n\" >> \"{}\"; exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&old_started),
            path_string(&old_stopped),
        ),
    );
    let new_script = root.child("bin/launcher-new");
    write_executable_script(
        &new_script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&new_started),
        ),
    );

    let add_old = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "old",
            "--command",
            &path_string(&old_script),
        ],
    );
    assert!(add_old.status.success(), "{}", stderr(&add_old));
    let add_new = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "new",
            "--command",
            &path_string(&new_script),
        ],
    );
    assert!(add_new.status.success(), "{}", stderr(&add_new));

    let create = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "old"]);
    assert!(create.status.success(), "{}", stderr(&create));
    set_service_enabled(&cwd, &env, "demo", true);
    let sync = run_ocm(&cwd, &env, &["supervisor", "sync"]);
    assert!(sync.status.success(), "{}", stderr(&sync));

    let mut supervisor = spawn_supervisor_process(&cwd, &env);
    assert!(wait_for_file(&old_started, Duration::from_secs(5)));

    let switch = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "new"]);
    assert!(switch.status.success(), "{}", stderr(&switch));

    assert!(wait_for_file(&old_stopped, Duration::from_secs(5)));
    assert!(wait_for_file(&new_started, Duration::from_secs(5)));

    stop_process(&mut supervisor);
}
