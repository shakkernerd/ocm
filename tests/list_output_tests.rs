mod support;

use std::fs;
use std::net::TcpListener;

use serde_json::Value;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

fn allocate_free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn env_list_accepts_raw_output_mode() {
    let root = TestDir::new("env-list-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--raw"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("demo"));
    assert!(output.contains("port="));
    assert!(!output.contains("┌"));
}

#[test]
fn env_list_json_reports_effective_ports_for_fresh_envs() {
    let root = TestDir::new("env-list-effective-port-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let value: Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1);
    assert_eq!(value[0]["name"], "demo");
    assert!(value[0]["gatewayPort"].as_u64().unwrap() >= 18_789);
}

#[test]
fn launcher_runtime_and_service_lists_accept_raw_output_mode() {
    let root = TestDir::new("list-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let binary_path = cwd.join("bin/openclaw");
    write_executable_script(&binary_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/openclaw"],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let launcher_list = run_ocm(&cwd, &env, &["launcher", "list", "--raw"]);
    assert!(launcher_list.status.success(), "{}", stderr(&launcher_list));
    assert!(stdout(&launcher_list).contains("stable  openclaw"));
    assert!(!stdout(&launcher_list).contains("┌"));

    let runtime_list = run_ocm(&cwd, &env, &["runtime", "list", "--raw"]);
    assert!(runtime_list.status.success(), "{}", stderr(&runtime_list));
    assert!(stdout(&runtime_list).contains("stable"));
    assert!(!stdout(&runtime_list).contains("┌"));

    let service_status = run_ocm(&cwd, &env, &["service", "status", "--raw"]);
    assert!(
        service_status.status.success(),
        "{}",
        stderr(&service_status)
    );
    assert!(stdout(&service_status).contains("demo"));
    assert!(!stdout(&service_status).contains("┌"));
}

#[test]
fn color_always_forces_pretty_colored_output_for_human_views() {
    let root = TestDir::new("color-always-pretty");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--color", "always"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("┌"));
    assert!(output.contains("\u{1b}["));
}

#[test]
fn raw_output_ignores_explicit_color_requests() {
    let root = TestDir::new("color-always-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--raw", "--color", "always"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(!output.contains("┌"));
    assert!(!output.contains("\u{1b}["));
}

#[test]
fn color_always_overrides_no_color_for_pretty_output() {
    let root = TestDir::new("color-always-no-color");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    env.insert("NO_COLOR".to_string(), "1".to_string());

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--color=always"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("\u{1b}["));
}

#[test]
fn invalid_color_mode_uses_clear_error() {
    let root = TestDir::new("color-invalid");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["--color", "rainbow", "help"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("--color must be one of auto, always, or never"));
}

#[test]
fn env_and_service_detail_commands_accept_raw_output_mode() {
    let root = TestDir::new("detail-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let port = allocate_free_port().to_string();

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "create",
            "demo",
            "--launcher",
            "stable",
            "--port",
            &port,
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--raw"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    assert!(stdout(&env_show).contains("name: demo"));
    assert!(!stdout(&env_show).contains("┌"));

    let env_status = run_ocm(&cwd, &env, &["env", "status", "demo", "--raw"]);
    assert!(env_status.status.success(), "{}", stderr(&env_status));
    assert!(stdout(&env_status).contains(&format!("gatewayPort: {port}")));
    assert!(!stdout(&env_status).contains("┌"));

    let env_resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo", "--raw"]);
    assert!(env_resolve.status.success(), "{}", stderr(&env_resolve));
    assert!(stdout(&env_resolve).contains("bindingKind: launcher"));
    assert!(!stdout(&env_resolve).contains("┌"));

    let env_doctor = run_ocm(&cwd, &env, &["env", "doctor", "demo", "--raw"]);
    assert!(env_doctor.status.success(), "{}", stderr(&env_doctor));
    assert!(stdout(&env_doctor).contains("healthy: true"));
    assert!(!stdout(&env_doctor).contains("┌"));

    let service_status = run_ocm(&cwd, &env, &["service", "status", "demo", "--raw"]);
    assert!(
        service_status.status.success(),
        "{}",
        stderr(&service_status)
    );
    assert!(stdout(&service_status).contains("installed: false"));
    assert!(stdout(&service_status).contains("desiredRunning: false"));
    assert!(stdout(&service_status).contains("running: false"));
    assert!(!stdout(&service_status).contains("┌"));

    let gateway_log = root.child("ocm-home/envs/demo/.openclaw/logs/gateway.log");
    fs::create_dir_all(gateway_log.parent().unwrap()).unwrap();
    fs::write(&gateway_log, "hello from logs\n").unwrap();

    let logs = run_ocm(&cwd, &env, &["logs", "demo", "--raw"]);
    assert!(logs.status.success(), "{}", stderr(&logs));
    assert_eq!(stdout(&logs), "hello from logs\n");
    assert!(!stdout(&logs).contains("┌"));
}

#[test]
fn release_and_runtime_show_accept_raw_output_mode() {
    let root = TestDir::new("release-runtime-show-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let binary_path = cwd.join("bin/openclaw");
    write_executable_script(&binary_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/openclaw"],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let runtime_show = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--raw"]);
    assert!(runtime_show.status.success(), "{}", stderr(&runtime_show));
    assert!(stdout(&runtime_show).contains("name: stable"));
    assert!(!stdout(&runtime_show).contains("┌"));

    let release_show = run_ocm(
        &cwd,
        &env,
        &["release", "show", "--channel", "stable", "--raw"],
    );
    assert!(release_show.status.success(), "{}", stderr(&release_show));
    assert!(stdout(&release_show).contains("channel: stable"));
    assert!(!stdout(&release_show).contains("┌"));
}

#[test]
fn release_and_runtime_show_reject_json_and_raw_together() {
    let root = TestDir::new("release-runtime-show-json-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let runtime_show = run_ocm(
        &cwd,
        &env,
        &["runtime", "show", "stable", "--json", "--raw"],
    );
    assert_eq!(runtime_show.status.code(), Some(1));
    assert!(stderr(&runtime_show).contains("runtime show accepts only one of --json or --raw"));

    let release_show = run_ocm(
        &cwd,
        &env,
        &["release", "show", "--channel", "stable", "--json", "--raw"],
    );
    assert_eq!(release_show.status.code(), Some(1));
    assert!(stderr(&release_show).contains("release show accepts only one of --json or --raw"));
}

#[test]
fn runtime_verify_accepts_raw_output_mode() {
    let root = TestDir::new("runtime-verify-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let binary_path = cwd.join("bin/openclaw");
    write_executable_script(&binary_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/openclaw"],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let verify = run_ocm(&cwd, &env, &["runtime", "verify", "stable", "--raw"]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    assert!(stdout(&verify).contains("name: stable"));
    assert!(!stdout(&verify).contains("┌"));

    let verify_all = run_ocm(&cwd, &env, &["runtime", "verify", "--all", "--raw"]);
    assert!(verify_all.status.success(), "{}", stderr(&verify_all));
    assert!(stdout(&verify_all).contains("healthy=true"));
    assert!(!stdout(&verify_all).contains("┌"));
}

#[test]
fn env_snapshot_commands_accept_raw_output_mode() {
    let root = TestDir::new("snapshot-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "create",
            "demo",
            "--port",
            "18799",
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "demo",
            "--label",
            "before-upgrade",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let snapshot_2 = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "demo",
            "--label",
            "after-upgrade",
        ],
    );
    assert!(snapshot_2.status.success(), "{}", stderr(&snapshot_2));

    let list_json = run_ocm(&cwd, &env, &["env", "snapshot", "list", "demo", "--json"]);
    assert!(list_json.status.success(), "{}", stderr(&list_json));
    let value: Value = serde_json::from_str(&stdout(&list_json)).unwrap();
    let snapshot_id = value[0]["id"].as_str().unwrap().to_string();

    let show = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "show", "demo", &snapshot_id, "--raw"],
    );
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("snapshotId:"));
    assert!(!stdout(&show).contains("┌"));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "demo", "--raw"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("label=before-upgrade"));
    assert!(!stdout(&list).contains("┌"));

    let prune = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "prune", "demo", "--keep", "1", "--raw"],
    );
    assert!(prune.status.success(), "{}", stderr(&prune));
    assert!(stdout(&prune).contains("Snapshot prune preview"));
    assert!(!stdout(&prune).contains("┌"));
}

#[test]
fn list_output_flags_reject_mixed_json_and_raw() {
    let root = TestDir::new("list-output-flags-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(&cwd, &env, &["launcher", "list", "--json", "--raw"]);
    assert_eq!(launcher.status.code(), Some(1));
    assert!(stderr(&launcher).contains("launcher list accepts only one of --json or --raw"));

    let runtime = run_ocm(&cwd, &env, &["runtime", "list", "--json", "--raw"]);
    assert_eq!(runtime.status.code(), Some(1));
    assert!(stderr(&runtime).contains("runtime list accepts only one of --json or --raw"));

    let service = run_ocm(&cwd, &env, &["service", "status", "--json", "--raw"]);
    assert_eq!(service.status.code(), Some(1));
    assert!(stderr(&service).contains("service status accepts only one of --json or --raw"));

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json", "--raw"]);
    assert_eq!(env_show.status.code(), Some(1));
    assert!(stderr(&env_show).contains("env show accepts only one of --json or --raw"));

    let env_status = run_ocm(&cwd, &env, &["env", "status", "demo", "--json", "--raw"]);
    assert_eq!(env_status.status.code(), Some(1));
    assert!(stderr(&env_status).contains("env status accepts only one of --json or --raw"));

    let env_resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo", "--json", "--raw"]);
    assert_eq!(env_resolve.status.code(), Some(1));
    assert!(stderr(&env_resolve).contains("env resolve accepts only one of --json or --raw"));

    let env_doctor = run_ocm(&cwd, &env, &["env", "doctor", "demo", "--json", "--raw"]);
    assert_eq!(env_doctor.status.code(), Some(1));
    assert!(stderr(&env_doctor).contains("env doctor accepts only one of --json or --raw"));

    let service_status = run_ocm(
        &cwd,
        &env,
        &["service", "status", "demo", "--json", "--raw"],
    );
    assert_eq!(service_status.status.code(), Some(1));
    assert!(stderr(&service_status).contains("service status accepts only one of --json or --raw"));

    let snapshot_show = run_ocm(
        &cwd,
        &env,
        &[
            "env", "snapshot", "show", "demo", "snap-1", "--json", "--raw",
        ],
    );
    assert_eq!(snapshot_show.status.code(), Some(1));
    assert!(
        stderr(&snapshot_show).contains("env snapshot show accepts only one of --json or --raw")
    );

    let snapshot_list = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "list", "demo", "--json", "--raw"],
    );
    assert_eq!(snapshot_list.status.code(), Some(1));
    assert!(
        stderr(&snapshot_list).contains("env snapshot list accepts only one of --json or --raw")
    );

    let snapshot_prune = run_ocm(
        &cwd,
        &env,
        &[
            "env", "snapshot", "prune", "demo", "--keep", "1", "--json", "--raw",
        ],
    );
    assert_eq!(snapshot_prune.status.code(), Some(1));
    assert!(
        stderr(&snapshot_prune).contains("env snapshot prune accepts only one of --json or --raw")
    );
}
