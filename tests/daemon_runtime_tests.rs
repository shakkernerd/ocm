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
