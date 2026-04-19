mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use ocm::env::EnvironmentService;
use ocm::supervisor::SupervisorService;
use serde_json::{Value, to_value};

use crate::support::{TestDir, ocm_env, path_string, run_ocm, stderr, write_executable_script};

fn spawn_daemon_process(cwd: &Path, env: &BTreeMap<String, String>) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ocm"));
    command.current_dir(cwd);
    command.args(["__daemon", "run"]);
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

fn read_persisted_service_state(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
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
    EnvironmentService::new(env, cwd)
        .set_service_policy(name, Some(enabled), Some(enabled))
        .unwrap();
}

fn setup_service_fixture(
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

fn setup_daemon_run_fixture(
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
fn service_state_plans_runnable_children_and_skips_disabled_envs() {
    let root = TestDir::new("service-state-plan");
    let (cwd, env) = setup_service_fixture(&root);
    let service = SupervisorService::new(&env, &cwd);

    let body = to_value(service.plan().unwrap()).unwrap();
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
    let demo_port = demo["childPort"].as_u64().unwrap();
    assert!(demo_port >= 18_789);
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
    let prod_port = prod["childPort"].as_u64().unwrap();
    assert!(prod_port.abs_diff(demo_port) > 110);
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

    EnvironmentService::new(&env, &cwd)
        .set_service_running("demo", false)
        .unwrap();
    let body = to_value(service.plan().unwrap()).unwrap();
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
fn daemon_run_persists_live_runtime_children() {
    let root = TestDir::new("daemon-runtime-state");
    let (cwd, env, _, _) = setup_daemon_run_fixture(&root);
    let runtime_path = root.child("ocm-home/supervisor/runtime.json");
    let service = SupervisorService::new(&env, &cwd);

    service.sync().unwrap();

    let mut daemon = spawn_daemon_process(&cwd, &env);
    let runtime = wait_for_runtime_children(&runtime_path, 2, Some("demo"), Duration::from_secs(5))
        .expect("daemon runtime state did not report running children");
    assert_eq!(runtime["kind"], "ocm-supervisor-runtime");

    let runtime_body = to_value(service.runtime().unwrap()).unwrap();
    assert_eq!(runtime_body["present"], true);
    assert_eq!(runtime_body["runtimePath"], path_string(&runtime_path));
    assert_eq!(runtime_body["children"].as_array().unwrap().len(), 2);

    stop_process(&mut daemon);
    let cleared = wait_for_runtime_children(&runtime_path, 0, None, Duration::from_secs(5))
        .expect("daemon runtime state did not clear after shutdown");
    assert!(cleared["updatedAt"].as_str().is_some());
}

#[test]
fn daemon_run_once_executes_planned_children() {
    let root = TestDir::new("daemon-run-once");
    let (cwd, env, launcher_marker, runtime_marker) = setup_daemon_run_fixture(&root);
    let service = SupervisorService::new(&env, &cwd);

    service.sync().unwrap();

    let run = run_ocm(&cwd, &env, &["__daemon", "run", "--once", "--json"]);
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
fn env_changes_refresh_persisted_service_state_without_extra_commands() {
    let root = TestDir::new("service-state-refresh");
    let (cwd, env) = setup_service_fixture(&root);
    let service = SupervisorService::new(&env, &cwd);
    let state_path = root.child("ocm-home/supervisor/state.json");

    service.sync().unwrap();

    let add_launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev-b", "--command", "openclaw --beta"],
    );
    assert!(add_launcher.status.success(), "{}", stderr(&add_launcher));
    let rebind = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev-b"]);
    assert!(rebind.status.success(), "{}", stderr(&rebind));

    let show_body = read_persisted_service_state(&state_path);
    let demo = show_body["children"]
        .as_array()
        .unwrap()
        .iter()
        .find(|child| child["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingName"], "dev-b");

    let created = run_ocm(&cwd, &env, &["env", "create", "extra", "--launcher", "dev"]);
    assert!(created.status.success(), "{}", stderr(&created));
    set_service_enabled(&cwd, &env, "extra", true);

    let show_body = read_persisted_service_state(&state_path);
    assert!(
        show_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "extra")
    );

    let removed = run_ocm(&cwd, &env, &["env", "remove", "extra", "--force"]);
    assert!(removed.status.success(), "{}", stderr(&removed));
    let show_body = read_persisted_service_state(&state_path);
    assert!(
        !show_body["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|child| child["envName"] == "extra")
    );
}

#[test]
fn daemon_run_reloads_children_after_binding_changes() {
    let root = TestDir::new("daemon-run-reconcile");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let service = SupervisorService::new(&env, &cwd);

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
    service.sync().unwrap();

    let mut daemon = spawn_daemon_process(&cwd, &env);
    assert!(wait_for_file(&old_started, Duration::from_secs(5)));

    let switch = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "new"]);
    assert!(switch.status.success(), "{}", stderr(&switch));

    assert!(wait_for_file(&old_stopped, Duration::from_secs(5)));
    assert!(wait_for_file(&new_started, Duration::from_secs(5)));

    stop_process(&mut daemon);
}

#[test]
fn daemon_keeps_running_when_one_env_fails_to_spawn() {
    let root = TestDir::new("daemon-run-spawn-failure");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let service = SupervisorService::new(&env, &cwd);
    let runtime_path = root.child("ocm-home/supervisor/runtime.json");

    let good_started = root.child("good-started.txt");
    let bad_started = root.child("bad-started.txt");
    let missing_cwd = root.child("missing-cwd");

    let good_script = root.child("bin/good-openclaw");
    write_executable_script(
        &good_script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&good_started),
        ),
    );
    let bad_script = root.child("bin/bad-openclaw");
    write_executable_script(
        &bad_script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&bad_started),
        ),
    );

    let good_launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "good",
            "--command",
            &path_string(&good_script),
        ],
    );
    assert!(good_launcher.status.success(), "{}", stderr(&good_launcher));
    let bad_launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "bad",
            "--command",
            &path_string(&bad_script),
            "--cwd",
            &path_string(&missing_cwd),
        ],
    );
    assert!(bad_launcher.status.success(), "{}", stderr(&bad_launcher));

    let good_env = run_ocm(&cwd, &env, &["env", "create", "good", "--launcher", "good"]);
    assert!(good_env.status.success(), "{}", stderr(&good_env));
    set_service_enabled(&cwd, &env, "good", true);

    let bad_env = run_ocm(&cwd, &env, &["env", "create", "bad", "--launcher", "bad"]);
    assert!(bad_env.status.success(), "{}", stderr(&bad_env));
    set_service_enabled(&cwd, &env, "bad", true);

    service.sync().unwrap();

    let mut daemon = spawn_daemon_process(&cwd, &env);
    assert!(wait_for_file(&good_started, Duration::from_secs(5)));
    let runtime = wait_for_runtime_children(&runtime_path, 1, Some("good"), Duration::from_secs(5))
        .expect("daemon runtime state did not keep the healthy child running");
    assert_eq!(runtime["children"][0]["envName"], "good");
    assert!(!bad_started.exists());
    assert!(daemon.try_wait().unwrap().is_none());

    fs::create_dir_all(&missing_cwd).unwrap();

    let runtime = wait_for_runtime_children(&runtime_path, 2, Some("bad"), Duration::from_secs(5))
        .expect("daemon runtime state did not recover the previously failing child");
    assert_eq!(runtime["children"].as_array().unwrap().len(), 2);
    assert!(wait_for_file(&bad_started, Duration::from_secs(5)));

    stop_process(&mut daemon);
}

#[test]
fn daemon_stops_a_running_child_after_service_stop() {
    let root = TestDir::new("daemon-run-service-stop");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let service = SupervisorService::new(&env, &cwd);
    let runtime_path = root.child("ocm-home/supervisor/runtime.json");

    let started = root.child("started.txt");
    let stopped = root.child("stopped.txt");
    let script = root.child("bin/openclaw");
    write_executable_script(
        &script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'printf \"stopped\\n\" >> \"{}\"; exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&started),
            path_string(&stopped),
        ),
    );

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "dev", "--command", &path_string(&script)],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "dev"]);
    assert!(created.status.success(), "{}", stderr(&created));
    set_service_enabled(&cwd, &env, "demo", true);
    service.sync().unwrap();

    let mut daemon = spawn_daemon_process(&cwd, &env);
    assert!(wait_for_file(&started, Duration::from_secs(5)));
    wait_for_runtime_children(&runtime_path, 1, Some("demo"), Duration::from_secs(5))
        .expect("daemon runtime state did not report the running child");

    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo"]);
    assert!(stop.status.success(), "{}", stderr(&stop));

    wait_for_runtime_children(&runtime_path, 0, None, Duration::from_secs(5))
        .expect("daemon runtime state did not clear after service stop");
    assert!(wait_for_file(&stopped, Duration::from_secs(5)));

    stop_process(&mut daemon);
}

#[test]
fn live_runtime_changes_recreate_missing_supervisor_state() {
    let root = TestDir::new("daemon-runtime-recovers-missing-state");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let service = SupervisorService::new(&env, &cwd);
    let runtime_path = root.child("ocm-home/supervisor/runtime.json");
    let state_path = root.child("ocm-home/supervisor/state.json");

    let first_started = root.child("first-started.txt");
    let second_started = root.child("second-started.txt");
    let first_script = root.child("bin/first-openclaw");
    write_executable_script(
        &first_script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&first_started),
        ),
    );
    let second_script = root.child("bin/second-openclaw");
    write_executable_script(
        &second_script,
        &format!(
            "#!/bin/sh\nprintf 'started\\n' >> '{}'\ntrap 'exit 0' TERM INT\nwhile :; do sleep 1; done\n",
            path_string(&second_started),
        ),
    );

    let first_launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "first",
            "--command",
            &path_string(&first_script),
        ],
    );
    assert!(
        first_launcher.status.success(),
        "{}",
        stderr(&first_launcher)
    );
    let second_launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "second",
            "--command",
            &path_string(&second_script),
        ],
    );
    assert!(
        second_launcher.status.success(),
        "{}",
        stderr(&second_launcher)
    );

    let demo = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "first"],
    );
    assert!(demo.status.success(), "{}", stderr(&demo));
    set_service_enabled(&cwd, &env, "demo", true);
    service.sync().unwrap();

    let mut daemon = spawn_daemon_process(&cwd, &env);
    assert!(wait_for_file(&first_started, Duration::from_secs(5)));
    wait_for_runtime_children(&runtime_path, 1, Some("demo"), Duration::from_secs(5))
        .expect("daemon runtime state did not report the first child");

    fs::remove_file(&state_path).unwrap();
    assert!(!state_path.exists());

    let extra = run_ocm(
        &cwd,
        &env,
        &["env", "create", "extra", "--launcher", "second"],
    );
    assert!(extra.status.success(), "{}", stderr(&extra));
    set_service_enabled(&cwd, &env, "extra", true);

    assert!(wait_for_file(&state_path, Duration::from_secs(5)));
    wait_for_runtime_children(&runtime_path, 2, Some("extra"), Duration::from_secs(5))
        .expect("daemon did not recover after the supervisor state file was recreated");
    assert!(wait_for_file(&second_started, Duration::from_secs(5)));

    stop_process(&mut daemon);
}
