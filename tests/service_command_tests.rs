mod support;

use std::fs;
use std::net::TcpListener;
use std::path::Path;

use serde_json::Value;

use crate::support::{
    TestDir, ocm_env, path_string, run_ocm, stderr, stdout, write_executable_script, write_text,
};

fn install_fake_launchctl(root: &TestDir, env: &mut std::collections::BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log_path = root.child("launchctl.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\ncase \"$1\" in\n  print)\n    exit 1\n    ;;\n  *)\n    exit 0\n    ;;\nesac\n",
        path_string(&log_path)
    );
    write_executable_script(&bin_dir.join("launchctl"), &script);

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(&bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(&bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
}

fn allocate_free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn service_list_reports_launcher_and_runtime_bindings_in_json() {
    let root = TestDir::new("service-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
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

    let demo = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(demo.status.success(), "{}", stderr(&demo));

    let prod = run_ocm(&cwd, &env, &["env", "create", "prod", "--runtime", "managed"]);
    assert!(prod.status.success(), "{}", stderr(&prod));

    let output = run_ocm(&cwd, &env, &["service", "list", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["globalLabel"], "ai.openclaw.gateway");
    assert_eq!(summary["globalInstalled"], false);
    assert_eq!(summary["globalLoaded"], false);
    assert_eq!(summary["globalRunning"], false);

    let services = summary["services"].as_array().unwrap();
    assert_eq!(services.len(), 2);

    let demo = services
        .iter()
        .find(|service| service["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingKind"], "launcher");
    assert_eq!(demo["bindingName"], "stable");
    assert_eq!(demo["gatewayPort"], 18789);
    assert_eq!(demo["managedLabel"], "ai.openclaw.gateway.ocm.demo");
    assert_eq!(demo["managedPlistPath"], format!(
        "{}/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist",
        root.child("home").display()
    ));
    assert_eq!(demo["runDir"], path_string(&root.child("ocm-home/envs/demo")));
    assert!(
        demo["command"]
            .as_str()
            .unwrap()
            .contains("'gateway' 'run' '--port' '18789'")
    );
    assert_eq!(demo["installed"], false);
    assert_eq!(demo["globalMatchesEnv"], false);
    assert_eq!(demo["backupAvailable"], false);
    assert_eq!(demo["canAdoptGlobal"], false);
    assert_eq!(demo["canRestoreGlobal"], false);
    assert_eq!(demo["latestBackupPlistPath"], Value::Null);

    let prod = services
        .iter()
        .find(|service| service["envName"] == "prod")
        .unwrap();
    assert_eq!(prod["bindingKind"], "runtime");
    assert_eq!(prod["bindingName"], "managed");
    assert_eq!(prod["binaryPath"], path_string(&runtime_path));
    assert_eq!(prod["gatewayPort"], 18790);
    assert_eq!(prod["runDir"], path_string(&root.child("ocm-home/envs/prod")));
    assert_eq!(
        prod["args"],
        serde_json::json!(["gateway", "run", "--port", "18790"])
    );
    assert_eq!(prod["issue"], Value::Null);
}

#[test]
fn service_status_reports_missing_binding_issue() {
    let root = TestDir::new("service-status-unbound");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "bare"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "bare", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["envName"], "bare");
    assert_eq!(summary["gatewayPort"], 18789);
    assert_eq!(summary["bindingKind"], Value::Null);
    assert_eq!(summary["bindingName"], Value::Null);
    assert_eq!(summary["backupAvailable"], false);
    assert_eq!(summary["canAdoptGlobal"], false);
    assert_eq!(summary["canRestoreGlobal"], false);
    assert_eq!(summary["latestBackupPlistPath"], Value::Null);
    assert_eq!(summary["issue"], "environment \"bare\" has no default runtime or launcher; use env set-runtime, env set-launcher, or pass --runtime/--launcher");
}

#[test]
fn service_status_reports_adoption_and_restore_readiness() {
    let root = TestDir::new("service-status-readiness");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let assigned_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{assigned_port}}}}}\n"),
    );
    write_text(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>{}</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>{}</string>
    </dict>
  </dict>
</plist>
"#,
            path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            assigned_port
        ),
    );

    let before = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(before.status.success(), "{}", stderr(&before));
    let before_summary: Value = serde_json::from_str(&stdout(&before)).unwrap();
    assert_eq!(before_summary["canAdoptGlobal"], true);
    assert_eq!(before_summary["canRestoreGlobal"], false);
    assert_eq!(before_summary["backupAvailable"], false);
    assert_eq!(before_summary["latestBackupPlistPath"], Value::Null);

    let adopted = run_ocm(&cwd, &env, &["service", "adopt-global", "demo", "--json"]);
    assert!(adopted.status.success(), "{}", stderr(&adopted));
    let adopted_summary: Value = serde_json::from_str(&stdout(&adopted)).unwrap();
    let backup_plist_path = adopted_summary["backupPlistPath"].clone();

    let after = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(after.status.success(), "{}", stderr(&after));
    let after_summary: Value = serde_json::from_str(&stdout(&after)).unwrap();
    assert_eq!(after_summary["canAdoptGlobal"], false);
    assert_eq!(after_summary["canRestoreGlobal"], true);
    assert_eq!(after_summary["backupAvailable"], true);
    assert_eq!(after_summary["latestBackupPlistPath"], backup_plist_path);
}

#[test]
fn service_status_requires_target_or_all() {
    let root = TestDir::new("service-status-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service status requires <env> or --all"));
}

#[test]
fn service_install_persists_a_gateway_port_and_writes_a_launch_agent() {
    let root = TestDir::new("service-install");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let assigned_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{assigned_port}}}}}\n"),
    );

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["gatewayPort"], assigned_port);
    assert_eq!(summary["persistedGatewayPort"], true);
    assert_eq!(summary["previousGatewayPort"], Value::Null);
    assert_eq!(
        summary["warnings"],
        serde_json::json!([
            format!(
                "assigned gateway port {assigned_port} to env \"demo\" and saved it to env metadata for service stability"
            )
        ])
    );

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"], assigned_port);

    let plist_path = root.child("home/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist");
    let plist = fs::read_to_string(&plist_path).unwrap();
    assert!(plist.contains("ai.openclaw.gateway.ocm.demo"));
    assert!(plist.contains("<string>/bin/sh</string>"));
    assert!(plist.contains(&format!(
        "openclaw &apos;gateway&apos; &apos;run&apos; &apos;--port&apos; &apos;{assigned_port}&apos;"
    )));
    assert!(plist.contains("<key>OPENCLAW_GATEWAY_PORT</key>"));
    assert!(plist.contains(&format!("<string>{assigned_port}</string>")));

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains("bootstrap gui/"));
}

#[test]
fn service_install_auto_provisions_the_next_free_port_when_needed() {
    let root = TestDir::new("service-install-port-reassign");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let occupied = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let occupied_port = occupied.local_addr().unwrap().port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{occupied_port}}}}}\n"),
    );

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let assigned_port = summary["gatewayPort"].as_u64().unwrap() as u16;
    assert!(assigned_port > occupied_port);
    assert_eq!(summary["persistedGatewayPort"], true);
    assert_eq!(summary["previousGatewayPort"], occupied_port);
    assert_eq!(
        summary["warnings"],
        serde_json::json!([
            format!(
                "gateway port {occupied_port} was unavailable; assigned {assigned_port} to env \"demo\" and saved it to env metadata"
            )
        ])
    );

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"], assigned_port);
}

#[test]
fn service_lifecycle_commands_use_the_env_scoped_launch_agent_label() {
    let root = TestDir::new("service-lifecycle");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo"]);
    assert!(stop.status.success(), "{}", stderr(&stop));
    let start = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(start.status.success(), "{}", stderr(&start));
    let restart = run_ocm(&cwd, &env, &["service", "restart", "demo"]);
    assert!(restart.status.success(), "{}", stderr(&restart));
    let uninstall = run_ocm(&cwd, &env, &["service", "uninstall", "demo"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootstrap gui/"));
    assert!(launchctl_log.contains("kickstart -k gui/"));
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains("ai.openclaw.gateway.ocm.demo"));
    assert!(!root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist")
        .exists());
}

#[test]
fn service_logs_reads_stdout_and_stderr_logs() {
    let root = TestDir::new("service-logs");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/logs/gateway.log"),
        "one\ntwo\nthree\n",
    );
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/logs/gateway.err.log"),
        "stderr-a\nstderr-b\n",
    );

    let stdout_logs = run_ocm(&cwd, &env, &["service", "logs", "demo"]);
    assert!(stdout_logs.status.success(), "{}", stderr(&stdout_logs));
    assert_eq!(stdout(&stdout_logs), "one\ntwo\nthree\n");

    let stderr_logs = run_ocm(&cwd, &env, &["service", "logs", "demo", "--stderr"]);
    assert!(stderr_logs.status.success(), "{}", stderr(&stderr_logs));
    assert_eq!(stdout(&stderr_logs), "stderr-a\nstderr-b\n");
}

#[test]
fn service_logs_supports_tail_and_json_output() {
    let root = TestDir::new("service-logs-tail");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/logs/gateway.log"),
        "line-1\nline-2\nline-3\n",
    );

    let tailed = run_ocm(&cwd, &env, &["service", "logs", "demo", "--tail", "2"]);
    assert!(tailed.status.success(), "{}", stderr(&tailed));
    assert_eq!(stdout(&tailed), "line-2\nline-3\n");

    let json_logs = run_ocm(
        &cwd,
        &env,
        &["service", "logs", "demo", "--tail", "1", "--json"],
    );
    assert!(json_logs.status.success(), "{}", stderr(&json_logs));
    let summary: Value = serde_json::from_str(&stdout(&json_logs)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["stream"], "stdout");
    assert_eq!(
        summary["path"],
        path_string(&root.child("ocm-home/envs/demo/.openclaw/logs/gateway.log"))
    );
    assert_eq!(summary["tailLines"], 1);
    assert_eq!(summary["content"], "line-3\n");
}

#[test]
fn service_logs_validate_arguments_and_missing_files() {
    let root = TestDir::new("service-logs-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let missing_name = run_ocm(&cwd, &env, &["service", "logs"]);
    assert_eq!(missing_name.status.code(), Some(1));
    assert!(stderr(&missing_name).contains("service logs requires <env>"));

    let conflicting = run_ocm(&cwd, &env, &["service", "logs", "demo", "--stdout", "--stderr"]);
    assert_eq!(conflicting.status.code(), Some(1));
    assert!(stderr(&conflicting).contains("service logs accepts only one of --stdout or --stderr"));

    let bad_tail = run_ocm(&cwd, &env, &["service", "logs", "demo", "--tail", "0"]);
    assert_eq!(bad_tail.status.code(), Some(1));
    assert!(stderr(&bad_tail).contains("--tail must be a positive integer"));

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let missing_log = run_ocm(&cwd, &env, &["service", "logs", "demo"]);
    assert_eq!(missing_log.status.code(), Some(1));
    assert!(stderr(&missing_log).contains("stdout log does not exist for env \"demo\""));
}

#[test]
fn service_adopt_global_migrates_a_matching_global_launch_agent() {
    let root = TestDir::new("service-adopt-global");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let assigned_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{assigned_port}}}}}\n"),
    );
    write_text(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>{}</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>{}</string>
    </dict>
  </dict>
</plist>
"#,
            path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            assigned_port
        ),
    );

    let output = run_ocm(&cwd, &env, &["service", "adopt-global", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["globalLabel"], "ai.openclaw.gateway");
    assert_eq!(summary["managedLabel"], "ai.openclaw.gateway.ocm.demo");
    assert_eq!(summary["gatewayPort"], assigned_port);
    assert_eq!(summary["dryRun"], false);
    assert_eq!(summary["adopted"], true);
    let backup_plist_path = summary["backupPlistPath"].as_str().unwrap();
    assert!(backup_plist_path.contains("/ocm-home/services/backups/ai.openclaw.gateway."));
    assert!(backup_plist_path.ends_with(".plist"));

    assert!(!root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.plist")
        .exists());
    assert!(root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist")
        .exists());
    assert!(Path::new(backup_plist_path).exists());

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains("ai.openclaw.gateway"));
    assert!(launchctl_log.contains("bootstrap gui/"));
    assert!(launchctl_log.contains("ai.openclaw.gateway.ocm.demo.plist"));
}

#[test]
fn service_adopt_global_rejects_mismatched_global_plists() {
    let root = TestDir::new("service-adopt-global-mismatch");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let demo = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(demo.status.success(), "{}", stderr(&demo));
    let other = run_ocm(&cwd, &env, &["env", "create", "other", "--launcher", "stable"]);
    assert!(other.status.success(), "{}", stderr(&other));

    write_text(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>{}</string>
    </dict>
  </dict>
</plist>
"#,
            path_string(&root.child("ocm-home/envs/other/.openclaw/openclaw.json"))
        ),
    );

    let output = run_ocm(&cwd, &env, &["service", "adopt-global", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("global OpenClaw service points at a different env"));
}

#[test]
fn service_adopt_global_dry_run_reports_the_plan_without_mutating_state() {
    let root = TestDir::new("service-adopt-global-dry-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let assigned_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{assigned_port}}}}}\n"),
    );
    write_text(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>{}</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>{}</string>
    </dict>
  </dict>
</plist>
"#,
            path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            assigned_port
        ),
    );

    let output = run_ocm(
        &cwd,
        &env,
        &["service", "adopt-global", "demo", "--dry-run", "--json"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["dryRun"], true);
    assert_eq!(summary["adopted"], false);
    let backup_plist_path = summary["backupPlistPath"].as_str().unwrap();
    assert!(backup_plist_path.contains("/ocm-home/services/backups/ai.openclaw.gateway."));

    assert!(root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.plist")
        .exists());
    assert!(!root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist")
        .exists());
    assert!(!Path::new(backup_plist_path).exists());

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(!launchctl_log.contains("bootstrap gui/"));
    assert!(!launchctl_log.contains("ai.openclaw.gateway.ocm.demo.plist"));
    assert!(!launchctl_log.contains("bootout gui/"));

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"], Value::Null);
}

#[test]
fn service_restore_global_restores_the_latest_matching_backup() {
    let root = TestDir::new("service-restore-global");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let assigned_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{assigned_port}}}}}\n"),
    );
    write_text(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>{}</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>{}</string>
    </dict>
  </dict>
</plist>
"#,
            path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            assigned_port
        ),
    );

    let adopted = run_ocm(&cwd, &env, &["service", "adopt-global", "demo", "--json"]);
    assert!(adopted.status.success(), "{}", stderr(&adopted));
    let adopted_summary: Value = serde_json::from_str(&stdout(&adopted)).unwrap();
    let backup_plist_path = adopted_summary["backupPlistPath"].as_str().unwrap().to_string();
    assert!(Path::new(&backup_plist_path).exists());

    let restored = run_ocm(&cwd, &env, &["service", "restore-global", "demo", "--json"]);
    assert!(restored.status.success(), "{}", stderr(&restored));
    let summary: Value = serde_json::from_str(&stdout(&restored)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["globalLabel"], "ai.openclaw.gateway");
    assert_eq!(summary["gatewayPort"], assigned_port);
    assert_eq!(summary["dryRun"], false);
    assert_eq!(summary["restored"], true);
    assert_eq!(summary["backupPlistPath"], backup_plist_path);

    let global_plist = root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist");
    assert!(global_plist.exists());
    assert!(!root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist")
        .exists());
    assert_eq!(
        fs::read_to_string(&global_plist).unwrap(),
        fs::read_to_string(Path::new(&backup_plist_path)).unwrap()
    );

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains("ai.openclaw.gateway.ocm.demo"));
    assert!(launchctl_log.contains("ai.openclaw.gateway.plist"));
}

#[test]
fn service_restore_global_dry_run_reports_the_latest_matching_backup_without_mutation() {
    let root = TestDir::new("service-restore-global-dry-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(&cwd, &env, &["launcher", "add", "stable", "--command", "openclaw"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let assigned_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{assigned_port}}}}}\n"),
    );
    write_text(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>EnvironmentVariables</key>
    <dict>
      <key>OPENCLAW_CONFIG_PATH</key>
      <string>{}</string>
      <key>OPENCLAW_GATEWAY_PORT</key>
      <string>{}</string>
    </dict>
  </dict>
</plist>
"#,
            path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            assigned_port
        ),
    );

    let adopted = run_ocm(&cwd, &env, &["service", "adopt-global", "demo", "--json"]);
    assert!(adopted.status.success(), "{}", stderr(&adopted));
    let adopted_summary: Value = serde_json::from_str(&stdout(&adopted)).unwrap();
    let backup_plist_path = adopted_summary["backupPlistPath"].as_str().unwrap().to_string();
    let launchctl_log_before = fs::read_to_string(root.child("launchctl.log")).unwrap();

    let restored = run_ocm(
        &cwd,
        &env,
        &["service", "restore-global", "demo", "--dry-run", "--json"],
    );
    assert!(restored.status.success(), "{}", stderr(&restored));
    let summary: Value = serde_json::from_str(&stdout(&restored)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["dryRun"], true);
    assert_eq!(summary["restored"], false);
    assert_eq!(summary["backupPlistPath"], backup_plist_path);

    assert!(!root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.plist")
        .exists());
    assert!(root
        .child("home/Library/LaunchAgents/ai.openclaw.gateway.ocm.demo.plist")
        .exists());
    assert_eq!(
        fs::read_to_string(root.child("launchctl.log")).unwrap(),
        launchctl_log_before
    );
}

#[test]
fn service_restore_global_rejects_missing_backups() {
    let root = TestDir::new("service-restore-global-missing-backup");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "restore-global", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("no global service backup exists for env \"demo\""));
}

#[test]
fn service_restore_global_requires_a_target_env() {
    let root = TestDir::new("service-restore-global-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "restore-global"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service restore-global requires <env>"));
}

#[test]
fn service_adopt_global_requires_a_target_env() {
    let root = TestDir::new("service-adopt-global-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "adopt-global"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service adopt-global requires <env>"));
}

#[test]
fn service_install_requires_a_target_env() {
    let root = TestDir::new("service-install-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "install"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service install requires <env>"));
}

#[test]
fn service_lifecycle_commands_require_a_target_env() {
    let root = TestDir::new("service-lifecycle-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    for action in ["start", "stop", "restart", "uninstall"] {
        let output = run_ocm(&cwd, &env, &["service", action]);
        assert_eq!(output.status.code(), Some(1), "action={action}");
        assert!(
            stderr(&output).contains(&format!("service {action} requires <env>")),
            "action={action}\n{}",
            stderr(&output)
        );
    }
}

#[test]
fn unknown_service_commands_use_service_specific_errors() {
    let root = TestDir::new("service-unknown-command");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "reload"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown service command: reload"));
}
