mod support;

use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, path_string, run_ocm, stderr, stdout};

fn setup_launcher_env(cwd: &std::path::Path, env: &std::collections::BTreeMap<String, String>) {
    let launcher = run_ocm(
        cwd,
        env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(cwd, env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
}

fn gateway_log_paths(root: &TestDir) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        root.child("ocm-home/envs/demo/.openclaw/logs/gateway.log"),
        root.child("ocm-home/envs/demo/.openclaw/logs/gateway.err.log"),
    )
}

#[test]
fn logs_reads_gateway_logs_from_the_env_root() {
    let root = TestDir::new("logs-gateway-root");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    setup_launcher_env(&cwd, &env);

    let (stdout_path, stderr_path) = gateway_log_paths(&root);
    fs::create_dir_all(stdout_path.parent().unwrap()).unwrap();
    fs::write(&stdout_path, "hello from gateway stdout\n").unwrap();
    fs::write(&stderr_path, "hello from gateway stderr\n").unwrap();

    let stdout_log = run_ocm(&cwd, &env, &["logs", "demo"]);
    assert!(stdout_log.status.success(), "{}", stderr(&stdout_log));
    assert!(stdout(&stdout_log).contains("hello from gateway stdout"));

    let stderr_log = run_ocm(&cwd, &env, &["logs", "demo", "--stderr", "--json"]);
    assert!(stderr_log.status.success(), "{}", stderr(&stderr_log));
    let body: Value = serde_json::from_str(&stdout(&stderr_log)).unwrap();
    assert_eq!(body["sourceKind"], "gateway");
    assert_eq!(body["path"], path_string(&stderr_path));
    assert_eq!(body["content"], "hello from gateway stderr\n");
}

#[test]
fn logs_can_merge_stdout_and_stderr_in_one_snapshot() {
    let root = TestDir::new("logs-all-streams");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    setup_launcher_env(&cwd, &env);

    let (stdout_path, stderr_path) = gateway_log_paths(&root);
    fs::create_dir_all(stdout_path.parent().unwrap()).unwrap();
    fs::write(
        &stdout_path,
        concat!(
            "2026-04-20T00:13:45.497+01:00 [gateway] one\n",
            "2026-04-20T00:13:47.497+01:00 [gateway] three\n"
        ),
    )
    .unwrap();
    fs::write(
        &stderr_path,
        "2026-04-20T00:13:46.497+01:00 error gateway two\n",
    )
    .unwrap();

    let output = run_ocm(&cwd, &env, &["logs", "demo", "--all-streams", "--raw"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        concat!(
            "2026-04-20T00:13:45.497+01:00 [gateway] one\n",
            "2026-04-20T00:13:46.497+01:00 error gateway two\n",
            "2026-04-20T00:13:47.497+01:00 [gateway] three\n"
        )
    );
}

#[test]
fn logs_fall_back_to_supervisor_logs_when_gateway_logs_are_missing() {
    let root = TestDir::new("logs-service-fallback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    setup_launcher_env(&cwd, &env);

    let fallback = root.child("ocm-home/supervisor/logs/demo.stdout.log");
    fs::create_dir_all(fallback.parent().unwrap()).unwrap();
    fs::write(&fallback, "hello from supervisor stdout\n").unwrap();

    let output = run_ocm(&cwd, &env, &["logs", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(body["sourceKind"], "service");
    assert_eq!(body["path"], path_string(&fallback));
    assert_eq!(body["content"], "hello from supervisor stdout\n");
}

#[test]
fn logs_prefer_the_newer_service_log_when_gateway_log_is_stale() {
    let root = TestDir::new("logs-prefer-newer-service");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    setup_launcher_env(&cwd, &env);

    let (gateway_path, _) = gateway_log_paths(&root);
    fs::create_dir_all(gateway_path.parent().unwrap()).unwrap();
    fs::write(&gateway_path, "old gateway output\n").unwrap();
    thread::sleep(Duration::from_millis(1100));

    let service_path = root.child("ocm-home/supervisor/logs/demo.stdout.log");
    fs::create_dir_all(service_path.parent().unwrap()).unwrap();
    fs::write(&service_path, "new service output\n").unwrap();

    let output = run_ocm(&cwd, &env, &["logs", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let body: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(body["sourceKind"], "service");
    assert_eq!(body["path"], path_string(&service_path));
    assert_eq!(body["content"], "new service output\n");
}

#[test]
fn logs_follow_streams_new_lines() {
    let root = TestDir::new("logs-follow");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    setup_launcher_env(&cwd, &env);

    let (stdout_path, _) = gateway_log_paths(&root);
    fs::create_dir_all(stdout_path.parent().unwrap()).unwrap();
    fs::write(&stdout_path, "first line\n").unwrap();

    let mut child = Command::new(env!("CARGO_BIN_EXE_ocm"))
        .current_dir(&cwd)
        .args(["logs", "demo", "--follow", "--tail", "1", "--raw"])
        .env_clear()
        .envs(&env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            lines.push(line.clone());
            let _ = sender.send(line);
        }
        lines
    });

    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        "first line"
    );
    fs::write(&stdout_path, "first line\nsecond line\n").unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(3)).unwrap(),
        "second line"
    );

    let _ = child.kill();
    let _ = child.wait();
    let lines = reader.join().unwrap();
    assert!(lines.iter().any(|line| line == "first line"));
    assert!(lines.iter().any(|line| line == "second line"));
}
