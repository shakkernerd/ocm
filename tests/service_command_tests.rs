mod support;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread::{self, JoinHandle};

use serde_json::Value;

use crate::support::{
    TestDir, TestHttpServer, managed_service_definition_path, managed_service_label, ocm_env,
    path_string, run_ocm, stderr, stdout, write_executable_script, write_text,
};

struct FixedHealthzServer {
    addr: String,
    handle: Option<JoinHandle<()>>,
}

impl Drop for FixedHealthzServer {
    fn drop(&mut self) {
        let _ = std::net::TcpStream::connect(&self.addr);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn serve_fixed_healthz(port: u16, body: &'static [u8], request_limit: usize) -> FixedHealthzServer {
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let addr = format!("127.0.0.1:{port}");
    let handle = thread::spawn(move || {
        for _ in 0..request_limit.max(1) {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request);
            let request_text = String::from_utf8_lossy(&request);
            let (status_line, response_body) = if request_text.starts_with("GET /healthz ") {
                ("HTTP/1.1 200 OK", body)
            } else {
                ("HTTP/1.1 404 Not Found", b"not found".as_slice())
            };
            let response = format!(
                "{status_line}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.write_all(response_body);
            let _ = stream.flush();
        }
    });

    FixedHealthzServer {
        addr,
        handle: Some(handle),
    }
}

fn install_fake_launchctl(root: &TestDir, env: &mut std::collections::BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log_path = root.child("launchctl.log");
    let print_path = root.child("launchctl-print.txt");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\ncase \"$1\" in\n  print)\n    if [ -f \"{}\" ]; then\n      /bin/cat \"{}\"\n      exit 0\n    fi\n    exit 1\n    ;;\n  *)\n    exit 0\n    ;;\nesac\n",
        path_string(&log_path),
        path_string(&print_path),
        path_string(&print_path),
    );
    write_executable_script(&bin_dir.join("launchctl"), &script);

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(&bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(&bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
    env.insert(
        "OCM_INTERNAL_LAUNCHCTL_BIN".to_string(),
        path_string(&bin_dir.join("launchctl")),
    );
}

fn install_fake_systemd_tools(root: &TestDir, env: &mut BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log_path = root.child("systemctl.log");
    let journal_log_path = root.child("journalctl.log");
    let systemctl_script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nif [ \"$1\" = \"--user\" ] && [ \"$2\" = \"show\" ]; then\n  unit=\"$3\"\n  home=\"${{HOME:-$PWD}}\"\n  unit_path=\"$home/.config/systemd/user/$unit.service\"\n  show_path=\"$home/.config/systemd/user/$unit.show\"\n  if [ -f \"$show_path\" ]; then\n    /bin/cat \"$show_path\"\n    exit 0\n  fi\n  if [ -f \"$unit_path\" ]; then\n    printf 'LoadState=loaded\\nUnitFileState=enabled\\nActiveState=active\\nSubState=running\\nMainPID=4242\\nFragmentPath=%s\\n' \"$unit_path\"\n    exit 0\n  fi\n  printf 'Unit %s could not be found\\n' \"$unit\" >&2\n  exit 1\nfi\nexit 0\n",
        path_string(&log_path)
    );
    let journalctl_script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\nprintf 'gateway ok\\n'\n",
        path_string(&journal_log_path)
    );
    write_executable_script(&bin_dir.join("systemctl"), &systemctl_script);
    write_executable_script(&bin_dir.join("journalctl"), &journalctl_script);

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(&bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(&bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "systemd-user".to_string(),
    );
    env.insert(
        "OCM_INTERNAL_SYSTEMCTL_BIN".to_string(),
        path_string(&bin_dir.join("systemctl")),
    );
    env.insert(
        "OCM_INTERNAL_JOURNALCTL_BIN".to_string(),
        path_string(&bin_dir.join("journalctl")),
    );
}

fn ocm_launchd_env(root: &TestDir) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    env
}

fn write_systemd_unit(
    path: &Path,
    description: &str,
    exec_start: &str,
    working_directory: Option<&str>,
    env_vars: &[(&str, &str)],
) {
    let working_directory_section = working_directory
        .map(|value| format!("WorkingDirectory={value}\n"))
        .unwrap_or_default();
    let environment_section = env_vars
        .iter()
        .map(|(key, value)| format!("Environment=\"{key}={value}\"\n"))
        .collect::<String>();
    write_text(
        path,
        &format!(
            "[Unit]\nDescription={description}\n\n[Service]\nType=simple\n{working_directory_section}ExecStart={exec_start}\n{environment_section}Restart=always\n\n[Install]\nWantedBy=default.target\n"
        ),
    );
}

fn allocate_free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn port_from_http_url(url: &str) -> u16 {
    url.trim_start_matches("http://127.0.0.1:")
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

fn write_launch_agent_plist(
    path: &Path,
    label: &str,
    program_arguments: &[&str],
    working_directory: Option<&str>,
    env_vars: &[(&str, &str)],
) {
    let program_arguments_section = if program_arguments.is_empty() {
        String::new()
    } else {
        let values = program_arguments
            .iter()
            .map(|value| format!("      <string>{value}</string>\n"))
            .collect::<String>();
        format!("    <key>ProgramArguments</key>\n    <array>\n{values}    </array>\n")
    };
    let working_directory_section = working_directory
        .map(|value| format!("    <key>WorkingDirectory</key>\n    <string>{value}</string>\n"))
        .unwrap_or_default();
    let env_section = env_vars
        .iter()
        .map(|(key, value)| format!("      <key>{key}</key>\n      <string>{value}</string>\n"))
        .collect::<String>();
    write_text(
        path,
        &format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>{label}</string>
{program_arguments_section}{working_directory_section}
    <key>EnvironmentVariables</key>
    <dict>
{env_section}    </dict>
  </dict>
</plist>
"#
        ),
    );
}

#[test]
fn service_list_reports_launcher_and_runtime_bindings_in_json() {
    let root = TestDir::new("service-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
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

    let demo = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(demo.status.success(), "{}", stderr(&demo));

    let prod = run_ocm(
        &cwd,
        &env,
        &["env", "create", "prod", "--runtime", "managed"],
    );
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
    let demo_label = managed_service_label(&env, &cwd, "demo");
    let demo_path = managed_service_definition_path(&env, &cwd, "demo");

    let demo = services
        .iter()
        .find(|service| service["envName"] == "demo")
        .unwrap();
    assert_eq!(demo["bindingKind"], "launcher");
    assert_eq!(demo["bindingName"], "stable");
    assert_eq!(demo["gatewayPort"], 18789);
    assert_eq!(demo["managedLabel"], demo_label);
    assert_eq!(demo["managedPlistPath"], path_string(&demo_path));
    assert_eq!(
        demo["runDir"],
        path_string(&root.child("ocm-home/envs/demo"))
    );
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
    assert_eq!(
        prod["runDir"],
        path_string(&root.child("ocm-home/envs/prod"))
    );
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
    let env = ocm_launchd_env(&root);

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
    assert_eq!(
        summary["issue"],
        "environment \"bare\" has no default runtime or launcher; use env set-runtime, env set-launcher, or pass --runtime/--launcher"
    );
}

#[test]
fn service_status_reports_wrong_service_when_gateway_probe_is_not_openclaw() {
    let root = TestDir::new("service-status-wrong-service");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);
    let server = TestHttpServer::serve_bytes_times("/not-healthz", "text/plain", b"ok", 2);
    let port = port_from_http_url(&server.url());

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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["openclawState"], "wrong-service");
    assert!(
        summary["openclawDetail"]
            .as_str()
            .unwrap()
            .contains("/healthz returned HTTP 404")
    );
}

#[test]
fn service_status_reports_auth_required_when_gateway_health_probe_is_rejected() {
    let root = TestDir::new("service-status-auth-required");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);
    let server = TestHttpServer::serve_bytes_times(
        "/healthz",
        "application/json",
        br#"{"ok":true,"status":"live"}"#,
        2,
    );
    let port = port_from_http_url(&server.url());

    let fake_openclaw = root.child("bin/openclaw-health");
    write_executable_script(
        &fake_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  echo 'Health check failed: unauthorized: gateway token mismatch' >&2\n  exit 1\nfi\nexit 0\n",
    );

    let launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            &path_string(&fake_openclaw),
        ],
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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["openclawState"], "auth-required");
    assert!(
        summary["openclawDetail"]
            .as_str()
            .unwrap()
            .contains("gateway token mismatch")
    );
}

#[test]
fn service_status_reports_responding_but_invalid_when_gateway_health_probe_fails() {
    let root = TestDir::new("service-status-responding-invalid");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);
    let server = TestHttpServer::serve_bytes_times(
        "/healthz",
        "application/json",
        br#"{"ok":true,"status":"live"}"#,
        2,
    );
    let port = port_from_http_url(&server.url());

    let fake_openclaw = root.child("bin/openclaw-health");
    write_executable_script(
        &fake_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  echo 'Health check failed: invalid hello payload' >&2\n  exit 1\nfi\nexit 0\n",
    );

    let launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            &path_string(&fake_openclaw),
        ],
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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["openclawState"], "responding-but-invalid");
    assert_eq!(summary["openclawDetail"], "invalid hello payload");
}

#[test]
fn service_status_probes_launcher_health_from_the_service_run_dir() {
    let root = TestDir::new("service-status-launcher-probe-run-dir");
    let cwd = root.child("workspace");
    let project_dir = root.child("project");
    fs::create_dir_all(&cwd).unwrap();
    fs::create_dir_all(&project_dir).unwrap();
    let canonical_project_dir = fs::canonicalize(&project_dir).unwrap();
    let env = ocm_launchd_env(&root);
    let server = TestHttpServer::serve_bytes_times(
        "/healthz",
        "application/json",
        br#"{"ok":true,"status":"live"}"#,
        2,
    );
    let port = port_from_http_url(&server.url());

    let fake_openclaw = root.child("bin/openclaw-health");
    write_executable_script(
        &fake_openclaw,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  if [ \"$PWD\" != \"{}\" ]; then\n    echo \"wrong run dir: $PWD\" >&2\n    exit 1\n  fi\n  exit 0\nfi\nexit 0\n",
            path_string(&canonical_project_dir)
        ),
    );

    let launcher = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            &path_string(&fake_openclaw),
            "--cwd",
            &path_string(&project_dir),
        ],
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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["openclawState"], "healthy");
}

#[test]
fn service_status_reports_installed_definition_drift_after_binding_changes() {
    let root = TestDir::new("service-status-definition-drift");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let server = TestHttpServer::serve_bytes_times(
        "/healthz",
        "application/json",
        br#"{"ok":true,"status":"live"}"#,
        2,
    );
    let port = port_from_http_url(&server.url());

    let stable_openclaw = root.child("bin/stable-openclaw");
    write_executable_script(
        &stable_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  exit 0\nfi\nexit 0\n",
    );
    let dev_openclaw = root.child("bin/dev-openclaw");
    write_executable_script(
        &dev_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  echo 'dev launcher should not be used yet' >&2\n  exit 1\nfi\nexit 0\n",
    );

    let stable = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            &path_string(&stable_openclaw),
        ],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));

    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            &path_string(&dev_openclaw),
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));

    let created = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "create",
            "demo",
            "--port",
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let rebound = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(rebound.status.success(), "{}", stderr(&rebound));

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["bindingKind"], "launcher");
    assert_eq!(summary["bindingName"], "dev");
    assert_eq!(summary["definitionDrift"], true);
    assert_eq!(summary["gatewayPort"], port);
    assert_eq!(summary["desiredGatewayPort"], port);
    assert_eq!(summary["installedGatewayPort"], port);
    assert_ne!(summary["openclawState"], "responding-but-invalid");
    assert!(
        summary["command"]
            .as_str()
            .unwrap()
            .contains(&path_string(&stable_openclaw))
    );
    assert!(
        summary["issue"]
            .as_str()
            .unwrap()
            .contains("installed service definition does not match the current env binding")
    );
}

#[test]
fn service_status_reports_env_block_drift_and_actual_installed_port() {
    let root = TestDir::new("service-status-env-block-drift");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let desired_server = TestHttpServer::serve_bytes_times(
        "/healthz",
        "application/json",
        br#"{"ok":true,"status":"live"}"#,
        1,
    );
    let desired_port = port_from_http_url(&desired_server.url());
    let installed_server = TestHttpServer::serve_bytes_times(
        "/healthz",
        "application/json",
        br#"{"ok":true,"status":"live"}"#,
        2,
    );
    let installed_port = port_from_http_url(&installed_server.url());

    let stable_openclaw = root.child("bin/stable-openclaw");
    write_executable_script(
        &stable_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  exit 0\nfi\nexit 0\n",
    );

    let stable = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            &path_string(&stable_openclaw),
        ],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));

    let created = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "create",
            "demo",
            "--port",
            &desired_port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let plist_path = managed_service_definition_path(&env, &cwd, "demo");
    let desired_config_path =
        path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json"));
    let desired_state_dir = path_string(&root.child("ocm-home/envs/demo/.openclaw"));
    let desired_home = path_string(&root.child("ocm-home/envs/demo"));
    let installed_config_path = "/tmp/foreign/.openclaw/openclaw.json";
    let installed_state_dir = "/tmp/foreign/.openclaw";
    let installed_home = "/tmp/foreign";
    let plist = fs::read_to_string(&plist_path).unwrap();
    let rewritten = plist
        .replace(&desired_port.to_string(), &installed_port.to_string())
        .replace(&desired_config_path, installed_config_path)
        .replace(&desired_state_dir, installed_state_dir)
        .replace(&desired_home, installed_home);
    fs::write(&plist_path, rewritten).unwrap();

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["definitionDrift"], true);
    assert_eq!(summary["gatewayPort"], installed_port);
    assert_eq!(summary["desiredGatewayPort"], desired_port);
    assert_eq!(summary["installedGatewayPort"], installed_port);
    assert_ne!(summary["openclawState"], "responding-but-invalid");
    assert!(
        summary["issue"]
            .as_str()
            .unwrap()
            .contains("installed service definition does not match the current env binding")
    );
}

#[test]
fn service_status_reports_desired_and_installed_gateway_ports_when_they_drift() {
    let root = TestDir::new("service-status-port-drift");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let assigned_port = allocate_free_port();

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
            &assigned_port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let meta_path = root.child("ocm-home/envs/demo.json");
    let mut meta: Value = serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
    let desired_port = assigned_port + 1;
    meta["gatewayPort"] = Value::from(desired_port);
    fs::write(
        &meta_path,
        format!("{}\n", serde_json::to_string_pretty(&meta).unwrap()),
    )
    .unwrap();

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["gatewayPort"], assigned_port);
    assert_eq!(summary["desiredGatewayPort"], desired_port);
    assert_eq!(summary["installedGatewayPort"], assigned_port);
    assert_eq!(summary["definitionDrift"], true);
    assert!(
        summary["issue"]
            .as_str()
            .unwrap()
            .contains("installed service definition does not match the current env binding")
    );
}

#[test]
fn service_status_skips_stale_launcher_health_probes_when_definition_drift_exists() {
    let root = TestDir::new("service-status-stale-definition-health");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let port = allocate_free_port();

    let stable_openclaw = root.child("bin/openclaw-stable");
    write_executable_script(
        &stable_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  echo 'Health check failed: unauthorized: gateway token mismatch' >&2\n  exit 1\nfi\nexit 0\n",
    );
    let dev_openclaw = root.child("bin/openclaw-dev");
    write_executable_script(
        &dev_openclaw,
        "#!/bin/sh\nif [ \"$1\" = \"health\" ]; then\n  exit 0\nfi\nexit 0\n",
    );

    let stable = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            &path_string(&stable_openclaw),
        ],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));
    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            &path_string(&dev_openclaw),
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));

    let created = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "create",
            "demo",
            "--port",
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let rebound = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(rebound.status.success(), "{}", stderr(&rebound));

    let _server = serve_fixed_healthz(port, br#"{"ok":true,"status":"live"}"#, 2);

    let output = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["definitionDrift"], true);
    assert_eq!(summary["openclawState"], "healthy");
    assert!(
        summary["issue"]
            .as_str()
            .unwrap()
            .contains("service start demo")
    );
}

#[test]
fn service_status_reports_adoption_and_restore_readiness() {
    let root = TestDir::new("service-status-readiness");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
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
    let env = ocm_launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service status requires <env> or --all"));
}

#[test]
fn service_discover_lists_ocm_global_and_foreign_services_in_json() {
    let root = TestDir::new("service-discover");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let demo_config_path = path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json"));
    let demo_state_dir = path_string(&root.child("ocm-home/envs/demo/.openclaw"));
    let demo_openclaw_home = demo_state_dir.clone();
    let demo_label = managed_service_label(&env, &cwd, "demo");
    let demo_path = managed_service_definition_path(&env, &cwd, "demo");
    write_launch_agent_plist(
        &demo_path,
        &demo_label,
        &["/bin/sh", "-lc", "openclaw gateway run --port 18789"],
        Some(&demo_state_dir),
        &[
            ("OPENCLAW_CONFIG_PATH", demo_config_path.as_str()),
            ("OPENCLAW_STATE_DIR", demo_state_dir.as_str()),
            ("OPENCLAW_HOME", demo_openclaw_home.as_str()),
            ("OPENCLAW_GATEWAY_PORT", "18789"),
        ],
    );
    write_launch_agent_plist(
        &root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist"),
        "ai.openclaw.gateway",
        &[
            "/opt/openclaw/bin/openclaw",
            "gateway",
            "run",
            "--port",
            "18789",
        ],
        Some("/srv/openclaw/global"),
        &[
            ("OPENCLAW_CONFIG_PATH", demo_config_path.as_str()),
            ("OPENCLAW_GATEWAY_PORT", "18789"),
        ],
    );
    write_launch_agent_plist(
        &root.child("home/Library/LaunchAgents/com.example.openclaw.staging.plist"),
        "com.example.openclaw.staging",
        &["/bin/sh", "-lc", "openclaw gateway run --port 19789"],
        Some("/srv/openclaw/staging"),
        &[
            (
                "OPENCLAW_CONFIG_PATH",
                "/srv/openclaw/staging/openclaw.json",
            ),
            ("OPENCLAW_STATE_DIR", "/srv/openclaw/staging"),
            ("OPENCLAW_HOME", "/srv/openclaw/staging"),
            ("OPENCLAW_GATEWAY_PORT", "19789"),
        ],
    );

    let output = run_ocm(&cwd, &env, &["service", "discover", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let services = summary["services"].as_array().unwrap();
    assert_eq!(services.len(), 3);

    let managed = services
        .iter()
        .find(|service| service["label"] == demo_label)
        .unwrap();
    assert_eq!(managed["sourceKind"], "ocm-managed");
    assert_eq!(managed["matchedEnvName"], "demo");
    assert_eq!(managed["gatewayPort"], 18789);
    assert_eq!(managed["stateDir"], demo_state_dir);
    assert_eq!(managed["openclawHome"], demo_openclaw_home);
    assert_eq!(managed["program"], "/bin/sh");
    assert_eq!(
        managed["programArguments"],
        serde_json::json!(["/bin/sh", "-lc", "openclaw gateway run --port 18789"])
    );
    assert_eq!(managed["workingDirectory"], demo_state_dir);
    assert_eq!(managed["adoptable"], false);
    assert_eq!(managed["adoptReason"], "already managed by ocm");

    let global = services
        .iter()
        .find(|service| service["label"] == "ai.openclaw.gateway")
        .unwrap();
    assert_eq!(global["sourceKind"], "openclaw-global");
    assert_eq!(global["matchedEnvName"], "demo");
    assert_eq!(global["program"], "/opt/openclaw/bin/openclaw");
    assert_eq!(
        global["programArguments"],
        serde_json::json!([
            "/opt/openclaw/bin/openclaw",
            "gateway",
            "run",
            "--port",
            "18789"
        ])
    );
    assert_eq!(global["workingDirectory"], "/srv/openclaw/global");
    assert_eq!(global["adoptable"], true);
    assert_eq!(
        global["adoptReason"],
        "ready to adopt into env \"demo\" with service adopt-global"
    );

    let foreign = services
        .iter()
        .find(|service| service["label"] == "com.example.openclaw.staging")
        .unwrap();
    assert_eq!(foreign["sourceKind"], "foreign");
    assert_eq!(foreign["matchedEnvName"], Value::Null);
    assert_eq!(foreign["gatewayPort"], 19789);
    assert_eq!(foreign["program"], "/bin/sh");
    assert_eq!(
        foreign["programArguments"],
        serde_json::json!(["/bin/sh", "-lc", "openclaw gateway run --port 19789"])
    );
    assert_eq!(foreign["workingDirectory"], "/srv/openclaw/staging");
    assert_eq!(foreign["adoptable"], false);
    assert_eq!(
        foreign["adoptReason"],
        "foreign OpenClaw services are discoverable but not adoptable yet"
    );
}

#[test]
fn service_discover_ignores_unrelated_launch_agents() {
    let root = TestDir::new("service-discover-unrelated");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    write_launch_agent_plist(
        &root.child("home/Library/LaunchAgents/com.example.other.plist"),
        "com.example.other",
        &["/usr/bin/echo", "hello"],
        Some("/tmp"),
        &[("SOME_KEY", "some-value")],
    );

    let output = run_ocm(&cwd, &env, &["service", "discover", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(summary["services"], serde_json::json!([]));
}

#[test]
fn service_discover_finds_openclaw_programs_without_openclaw_env_vars() {
    let root = TestDir::new("service-discover-program-only");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    write_launch_agent_plist(
        &root.child("home/Library/LaunchAgents/com.example.gateway.plist"),
        "com.example.gateway",
        &[
            "/usr/local/bin/openclaw",
            "gateway",
            "run",
            "--port",
            "19790",
        ],
        Some("/srv/openclaw/program-only"),
        &[("PATH", "/usr/local/bin:/usr/bin:/bin")],
    );

    let output = run_ocm(&cwd, &env, &["service", "discover", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let services = summary["services"].as_array().unwrap();
    assert_eq!(services.len(), 1);
    assert_eq!(services[0]["label"], "com.example.gateway");
    assert_eq!(services[0]["sourceKind"], "foreign");
    assert_eq!(services[0]["program"], "/usr/local/bin/openclaw");
    assert_eq!(
        services[0]["programArguments"],
        serde_json::json!([
            "/usr/local/bin/openclaw",
            "gateway",
            "run",
            "--port",
            "19790"
        ])
    );
    assert_eq!(
        services[0]["workingDirectory"],
        "/srv/openclaw/program-only"
    );
}

#[test]
fn service_discover_requires_no_extra_args() {
    let root = TestDir::new("service-discover-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "discover", "demo"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unexpected arguments: demo"));
}

#[test]
fn service_install_persists_a_gateway_port_and_writes_a_launch_agent() {
    let root = TestDir::new("service-install");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let preferred_port = allocate_free_port();
    write_text(
        &root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        &format!("{{\"gateway\":{{\"port\":{preferred_port}}}}}\n"),
    );

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let assigned_port = summary["gatewayPort"].as_u64().unwrap() as u16;
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["gatewayPort"], assigned_port);
    assert_eq!(summary["persistedGatewayPort"], true);
    if assigned_port == preferred_port {
        assert_eq!(summary["previousGatewayPort"], Value::Null);
        assert_eq!(
            summary["warnings"],
            serde_json::json!([format!(
                "assigned gateway port {assigned_port} to env \"demo\" and saved it to env metadata for service stability"
            )])
        );
    } else {
        assert_eq!(summary["previousGatewayPort"], preferred_port);
        assert_eq!(
            summary["warnings"],
            serde_json::json!([format!(
                "gateway port {preferred_port} was unavailable; assigned {assigned_port} to env \"demo\" and saved it to env metadata"
            )])
        );
    }

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"], assigned_port);
    let config =
        fs::read_to_string(root.child("ocm-home/envs/demo/.openclaw/openclaw.json")).unwrap();
    let config: Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["gateway"]["port"], assigned_port);

    let managed_label = managed_service_label(&env, &cwd, "demo");
    let plist_path = managed_service_definition_path(&env, &cwd, "demo");
    let plist = fs::read_to_string(&plist_path).unwrap();
    assert!(plist.contains(&managed_label));
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
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
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
        serde_json::json!([format!(
            "gateway port {occupied_port} was unavailable; assigned {assigned_port} to env \"demo\" and saved it to env metadata"
        )])
    );

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"], assigned_port);
    let config =
        fs::read_to_string(root.child("ocm-home/envs/demo/.openclaw/openclaw.json")).unwrap();
    let config: Value = serde_json::from_str(&config).unwrap();
    assert_eq!(config["gateway"]["port"], assigned_port);
}

#[test]
fn service_lifecycle_commands_use_the_env_scoped_launch_agent_label() {
    let root = TestDir::new("service-lifecycle");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));
    assert!(stdout(&install).contains("start: ocm service start demo"));
    assert!(stdout(&install).contains("status: ocm service status demo"));
    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo"]);
    assert!(stop.status.success(), "{}", stderr(&stop));
    assert!(stdout(&stop).contains("status: ocm service status demo"));
    let start = run_ocm(&cwd, &env, &["service", "start", "demo"]);
    assert!(start.status.success(), "{}", stderr(&start));
    assert!(stdout(&start).contains("status: ocm service status demo"));
    let restart = run_ocm(&cwd, &env, &["service", "restart", "demo"]);
    assert!(restart.status.success(), "{}", stderr(&restart));
    assert!(stdout(&restart).contains("status: ocm service status demo"));
    let uninstall = run_ocm(&cwd, &env, &["service", "uninstall", "demo"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));
    assert!(stdout(&uninstall).contains("install: ocm service install demo"));
    let managed_label = managed_service_label(&env, &cwd, "demo");
    let managed_path = managed_service_definition_path(&env, &cwd, "demo");

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootstrap gui/"));
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains(&managed_label));
    assert!(!managed_path.exists());
}

#[test]
fn service_restart_refreshes_the_installed_service_binding() {
    let root = TestDir::new("service-restart-refreshes-binding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let stable = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));
    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            "pnpm openclaw",
            "--cwd",
            "/tmp/openclaw-dev",
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let plist_path = managed_service_definition_path(&env, &cwd, "demo");
    let installed_plist = fs::read_to_string(&plist_path).unwrap();
    assert!(installed_plist.contains("openclaw &apos;gateway&apos; &apos;run&apos;"));
    assert!(!installed_plist.contains("pnpm openclaw"));

    let rebound = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(rebound.status.success(), "{}", stderr(&rebound));

    let restart = run_ocm(&cwd, &env, &["service", "restart", "demo", "--json"]);
    assert!(restart.status.success(), "{}", stderr(&restart));
    let summary: Value = serde_json::from_str(&stdout(&restart)).unwrap();
    assert_eq!(summary["envName"], "demo");

    let refreshed_plist = fs::read_to_string(&plist_path).unwrap();
    assert!(refreshed_plist.contains("pnpm openclaw &apos;gateway&apos; &apos;run&apos;"));
    assert!(refreshed_plist.contains("<string>/tmp/openclaw-dev</string>"));

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    let bootstrap_count = launchctl_log.matches("bootstrap gui/").count();
    assert!(bootstrap_count >= 2, "{launchctl_log}");
}

#[test]
fn service_start_refreshes_the_installed_service_binding() {
    let root = TestDir::new("service-start-refreshes-binding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let stable = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));
    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            "pnpm openclaw",
            "--cwd",
            "/tmp/openclaw-dev",
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo"]);
    assert!(stop.status.success(), "{}", stderr(&stop));

    let rebound = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(rebound.status.success(), "{}", stderr(&rebound));

    let start = run_ocm(&cwd, &env, &["service", "start", "demo", "--json"]);
    assert!(start.status.success(), "{}", stderr(&start));
    let summary: Value = serde_json::from_str(&stdout(&start)).unwrap();
    assert_eq!(summary["envName"], "demo");

    let plist_path = managed_service_definition_path(&env, &cwd, "demo");
    let refreshed_plist = fs::read_to_string(&plist_path).unwrap();
    assert!(refreshed_plist.contains("pnpm openclaw &apos;gateway&apos; &apos;run&apos;"));
    assert!(refreshed_plist.contains("<string>/tmp/openclaw-dev</string>"));

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    let bootstrap_count = launchctl_log.matches("bootstrap gui/").count();
    assert!(bootstrap_count >= 2, "{launchctl_log}");
}

#[test]
fn service_restart_keeps_the_existing_gateway_port_when_it_is_busy() {
    let root = TestDir::new("service-restart-keeps-existing-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let port = allocate_free_port();

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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let occupied = TcpListener::bind(("127.0.0.1", port)).unwrap();

    let restart = run_ocm(&cwd, &env, &["service", "restart", "demo", "--json"]);
    assert!(restart.status.success(), "{}", stderr(&restart));
    let summary: Value = serde_json::from_str(&stdout(&restart)).unwrap();
    assert_eq!(summary["gatewayPort"], port);

    let env_meta: Value =
        serde_json::from_str(&fs::read_to_string(root.child("ocm-home/envs/demo.json")).unwrap())
            .unwrap();
    assert_eq!(env_meta["gatewayPort"], port);
    drop(occupied);
}

#[test]
fn service_start_keeps_the_existing_gateway_port_when_it_is_busy() {
    let root = TestDir::new("service-start-keeps-existing-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let port = allocate_free_port();

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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo"]);
    assert!(stop.status.success(), "{}", stderr(&stop));

    let occupied = TcpListener::bind(("127.0.0.1", port)).unwrap();

    let start = run_ocm(&cwd, &env, &["service", "start", "demo", "--json"]);
    assert!(start.status.success(), "{}", stderr(&start));
    let summary: Value = serde_json::from_str(&stdout(&start)).unwrap();
    assert_eq!(summary["gatewayPort"], port);

    let env_meta: Value =
        serde_json::from_str(&fs::read_to_string(root.child("ocm-home/envs/demo.json")).unwrap())
            .unwrap();
    assert_eq!(env_meta["gatewayPort"], port);
    drop(occupied);
}

#[test]
fn service_uninstall_does_not_require_a_still_valid_binding() {
    let root = TestDir::new("service-uninstall-stale-binding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let remove_launcher = run_ocm(&cwd, &env, &["launcher", "remove", "stable"]);
    assert!(
        remove_launcher.status.success(),
        "{}",
        stderr(&remove_launcher)
    );

    let uninstall = run_ocm(&cwd, &env, &["service", "uninstall", "demo"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));
    assert!(stdout(&uninstall).contains("Uninstalled service demo"));
    assert!(!managed_service_definition_path(&env, &cwd, "demo").exists());
}

#[test]
fn service_status_reports_loaded_launchd_services_even_when_the_plist_is_gone() {
    let root = TestDir::new("service-status-launchd-orphan");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    let port = allocate_free_port();
    let _server = serve_fixed_healthz(port, br#"{"ok":true,"status":"live"}"#, 2);

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
            &port.to_string(),
            "--launcher",
            "stable",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let plist_path = managed_service_definition_path(&env, &cwd, "demo");
    fs::remove_file(&plist_path).unwrap();
    write_text(
        &root.child("launchctl-print.txt"),
        &format!(
            "state = running\npid = 4242\nenvironment = {{\n  OPENCLAW_GATEWAY_PORT => {port}\n  OPENCLAW_CONFIG_PATH => {}\n}}\n",
            path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json"))
        ),
    );

    let status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let summary: Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(summary["installed"], false);
    assert_eq!(summary["loaded"], true);
    assert_eq!(summary["running"], true);
    assert_eq!(summary["state"], "running");
    assert_eq!(summary["definitionDrift"], true);
    assert!(summary["command"].is_null());
    assert!(
        summary["issue"]
            .as_str()
            .unwrap()
            .contains("service restart demo")
    );
}

#[test]
fn systemd_service_uninstall_disables_orphans_even_when_the_unit_file_is_missing() {
    let root = TestDir::new("service-uninstall-systemd-orphan");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let managed_label = managed_service_label(&env, &cwd, "demo");
    let unit_path = managed_service_definition_path(&env, &cwd, "demo");
    fs::remove_file(&unit_path).unwrap();

    let uninstall = run_ocm(&cwd, &env, &["service", "uninstall", "demo"]);
    assert!(uninstall.status.success(), "{}", stderr(&uninstall));

    let systemctl_log = fs::read_to_string(root.child("systemctl.log")).unwrap();
    assert!(systemctl_log.contains(&format!("--user disable --now {managed_label}")));
    assert!(systemctl_log.contains("--user daemon-reload"));
}

#[test]
fn service_logs_reads_stdout_and_stderr_logs() {
    let root = TestDir::new("service-logs");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

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
    let env = ocm_launchd_env(&root);

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
    let env = ocm_launchd_env(&root);

    let missing_name = run_ocm(&cwd, &env, &["service", "logs"]);
    assert_eq!(missing_name.status.code(), Some(1));
    assert!(stderr(&missing_name).contains("service logs requires <env>"));

    let conflicting = run_ocm(
        &cwd,
        &env,
        &["service", "logs", "demo", "--stdout", "--stderr"],
    );
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
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
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
    let managed_label = managed_service_label(&env, &cwd, "demo");
    let managed_path = managed_service_definition_path(&env, &cwd, "demo");
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["globalLabel"], "ai.openclaw.gateway");
    assert_eq!(summary["managedLabel"], managed_label);
    assert_eq!(summary["gatewayPort"], assigned_port);
    assert_eq!(summary["dryRun"], false);
    assert_eq!(summary["adopted"], true);
    let backup_plist_path = summary["backupPlistPath"].as_str().unwrap();
    assert!(backup_plist_path.contains("/ocm-home/services/backups/ai.openclaw.gateway."));
    assert!(backup_plist_path.ends_with(".plist"));

    assert!(
        !root
            .child("home/Library/LaunchAgents/ai.openclaw.gateway.plist")
            .exists()
    );
    assert!(managed_path.exists());
    assert!(Path::new(backup_plist_path).exists());

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains("ai.openclaw.gateway"));
    assert!(launchctl_log.contains("bootstrap gui/"));
    assert!(launchctl_log.contains(&format!("{managed_label}.plist")));

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"].as_u64(), Some(assigned_port as u64));

    let config: Value = serde_json::from_str(
        &fs::read_to_string(root.child("ocm-home/envs/demo/.openclaw/openclaw.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        config["gateway"]["port"].as_u64(),
        Some(assigned_port as u64)
    );
}

#[test]
fn service_adopt_global_rejects_mismatched_global_plists() {
    let root = TestDir::new("service-adopt-global-mismatch");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let demo = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(demo.status.success(), "{}", stderr(&demo));
    let other = run_ocm(
        &cwd,
        &env,
        &["env", "create", "other", "--launcher", "stable"],
    );
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
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
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

    assert!(
        root.child("home/Library/LaunchAgents/ai.openclaw.gateway.plist")
            .exists()
    );
    assert!(!managed_service_definition_path(&env, &cwd, "demo").exists());
    assert!(!Path::new(backup_plist_path).exists());

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(!launchctl_log.contains("bootstrap gui/"));
    assert!(!launchctl_log.contains(&format!(
        "{}.plist",
        managed_service_label(&env, &cwd, "demo")
    )));
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
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
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
    let backup_plist_path = adopted_summary["backupPlistPath"]
        .as_str()
        .unwrap()
        .to_string();
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
    assert!(!managed_service_definition_path(&env, &cwd, "demo").exists());
    assert_eq!(
        fs::read_to_string(&global_plist).unwrap(),
        fs::read_to_string(Path::new(&backup_plist_path)).unwrap()
    );

    let launchctl_log = fs::read_to_string(root.child("launchctl.log")).unwrap();
    assert!(launchctl_log.contains("bootout gui/"));
    assert!(launchctl_log.contains(&managed_service_label(&env, &cwd, "demo")));
    assert!(launchctl_log.contains("ai.openclaw.gateway.plist"));
}

#[test]
fn service_restore_global_persists_the_restored_gateway_port_into_env_state() {
    let root = TestDir::new("service-restore-global-port-sync");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let restored_port = allocate_free_port();
    let config_path = root.child("ocm-home/envs/demo/.openclaw/openclaw.json");
    write_text(&config_path, "{\"gateway\":{\"port\":19999}}\n");
    write_text(
        &root.child("ocm-home/services/backups/ai.openclaw.gateway.123.plist"),
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
            path_string(&config_path),
            restored_port
        ),
    );

    let restored = run_ocm(&cwd, &env, &["service", "restore-global", "demo", "--json"]);
    assert!(restored.status.success(), "{}", stderr(&restored));
    let summary: Value = serde_json::from_str(&stdout(&restored)).unwrap();
    assert_eq!(summary["gatewayPort"], restored_port);

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_meta: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    assert_eq!(env_meta["gatewayPort"].as_u64(), Some(restored_port as u64));

    let config: Value = serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(
        config["gateway"]["port"].as_u64(),
        Some(restored_port as u64)
    );
}

#[test]
fn service_restore_global_dry_run_reports_the_latest_matching_backup_without_mutation() {
    let root = TestDir::new("service-restore-global-dry-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
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
    let backup_plist_path = adopted_summary["backupPlistPath"]
        .as_str()
        .unwrap()
        .to_string();
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

    assert!(
        !root
            .child("home/Library/LaunchAgents/ai.openclaw.gateway.plist")
            .exists()
    );
    assert!(managed_service_definition_path(&env, &cwd, "demo").exists());
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
    let env = ocm_launchd_env(&root);

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
    let env = ocm_launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "restore-global"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service restore-global requires <env>"));
}

#[test]
fn service_adopt_global_requires_a_target_env() {
    let root = TestDir::new("service-adopt-global-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "adopt-global"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service adopt-global requires <env>"));
}

#[test]
fn service_install_requires_a_target_env() {
    let root = TestDir::new("service-install-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "install"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("service install requires <env>"));
}

#[test]
fn service_lifecycle_commands_require_a_target_env() {
    let root = TestDir::new("service-lifecycle-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

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
fn service_stop_does_not_persist_a_gateway_port_when_only_stopping() {
    let root = TestDir::new("service-stop-does-not-persist-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let meta_path = root.child("ocm-home/envs/demo.json");
    let before_meta: Value =
        serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert_eq!(before_meta["gatewayPort"], Value::Null);

    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo", "--json"]);
    assert!(stop.status.success(), "{}", stderr(&stop));

    let after_meta: Value = serde_json::from_str(&fs::read_to_string(&meta_path).unwrap()).unwrap();
    assert_eq!(after_meta["gatewayPort"], Value::Null);
}

#[test]
fn unknown_service_commands_use_service_specific_errors() {
    let root = TestDir::new("service-unknown-command");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let output = run_ocm(&cwd, &env, &["service", "reload"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown service command: reload"));
}

#[test]
fn service_install_rejects_unsupported_backends() {
    let root = TestDir::new("service-install-unsupported");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "unsupported".to_string(),
    );

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert_eq!(install.status.code(), Some(1));
    assert!(stderr(&install).contains("managed services are not supported on this platform yet"));
}

#[test]
fn systemd_service_install_writes_unit_and_restarts_it() {
    let root = TestDir::new("service-install-systemd");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "install", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let managed_label = managed_service_label(&env, &cwd, "demo");
    let unit_path = managed_service_definition_path(&env, &cwd, "demo");
    assert_eq!(summary["managedPlistPath"], path_string(&unit_path));
    assert!(unit_path.exists());
    let unit = fs::read_to_string(&unit_path).unwrap();
    let gateway_port = summary["gatewayPort"].as_u64().unwrap();
    assert!(unit.contains("ExecStart=/bin/sh -lc"));
    assert!(unit.contains("/bin/true"));
    assert!(unit.contains(&format!(
        "Environment=\"OPENCLAW_GATEWAY_PORT={gateway_port}\""
    )));

    let systemctl_log = fs::read_to_string(root.child("systemctl.log")).unwrap();
    assert!(systemctl_log.contains("--user daemon-reload"));
    assert!(systemctl_log.contains(&format!("--user enable {managed_label}")));
    assert!(systemctl_log.contains(&format!("--user restart {managed_label}")));
}

#[test]
fn systemd_service_restart_refreshes_the_installed_service_binding() {
    let root = TestDir::new("service-restart-systemd-refresh");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let stable = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));

    let dev_dir = root.child("openclaw-dev");
    fs::create_dir_all(&dev_dir).unwrap();
    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &path_string(&dev_dir),
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let set_launcher = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(set_launcher.status.success(), "{}", stderr(&set_launcher));

    let restart = run_ocm(&cwd, &env, &["service", "restart", "demo", "--json"]);
    assert!(restart.status.success(), "{}", stderr(&restart));

    let managed_label = managed_service_label(&env, &cwd, "demo");
    let unit_path = managed_service_definition_path(&env, &cwd, "demo");
    let unit = fs::read_to_string(&unit_path).unwrap();
    assert!(unit.contains("ExecStart=/bin/sh -lc"));
    assert!(unit.contains("pnpm openclaw"));
    assert!(unit.contains("'gateway' 'run' '--port'"));
    assert!(unit.contains(&format!("WorkingDirectory={}", path_string(&dev_dir))));

    let systemctl_log = fs::read_to_string(root.child("systemctl.log")).unwrap();
    assert!(systemctl_log.contains(&format!("--user enable {managed_label}")));
    assert!(systemctl_log.contains(&format!("--user restart {managed_label}")));
}

#[test]
fn systemd_service_start_refreshes_the_installed_service_binding() {
    let root = TestDir::new("service-start-systemd-refresh");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let stable = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));

    let dev_dir = root.child("openclaw-dev");
    fs::create_dir_all(&dev_dir).unwrap();
    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &path_string(&dev_dir),
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let stop = run_ocm(&cwd, &env, &["service", "stop", "demo"]);
    assert!(stop.status.success(), "{}", stderr(&stop));

    let set_launcher = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(set_launcher.status.success(), "{}", stderr(&set_launcher));

    let start = run_ocm(&cwd, &env, &["service", "start", "demo", "--json"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let managed_label = managed_service_label(&env, &cwd, "demo");
    let unit_path = managed_service_definition_path(&env, &cwd, "demo");
    let unit = fs::read_to_string(&unit_path).unwrap();
    assert!(unit.contains("ExecStart=/bin/sh -lc"));
    assert!(unit.contains("pnpm openclaw"));
    assert!(unit.contains("'gateway' 'run' '--port'"));
    assert!(unit.contains(&format!("WorkingDirectory={}", path_string(&dev_dir))));

    let systemctl_log = fs::read_to_string(root.child("systemctl.log")).unwrap();
    assert!(systemctl_log.contains(&format!("--user enable {managed_label}")));
    assert!(systemctl_log.contains(&format!("--user restart {managed_label}")));
}

#[test]
fn systemd_service_status_and_discover_use_units() {
    let root = TestDir::new("service-status-systemd");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let managed_label = managed_service_label(&env, &cwd, "demo");
    let managed_path = managed_service_definition_path(&env, &cwd, "demo");
    write_systemd_unit(
        &managed_path,
        "demo",
        "/bin/sh -lc \"/bin/true gateway run --port 18789\"",
        Some(&path_string(&root.child("ocm-home/envs/demo"))),
        &[
            (
                "OPENCLAW_CONFIG_PATH",
                &path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            ),
            ("OPENCLAW_GATEWAY_PORT", "18789"),
            (
                "OPENCLAW_HOME",
                &path_string(&root.child("ocm-home/envs/demo")),
            ),
        ],
    );
    write_systemd_unit(
        &root.child("home/.config/systemd/user/ai.openclaw.gateway.service"),
        "global",
        "/usr/bin/node /tmp/openclaw gateway --port 18790",
        Some("/tmp"),
        &[
            (
                "OPENCLAW_CONFIG_PATH",
                &path_string(&root.child("ocm-home/envs/demo/.openclaw/openclaw.json")),
            ),
            ("OPENCLAW_GATEWAY_PORT", "18790"),
        ],
    );

    let status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let summary: Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(summary["installed"], true);
    assert_eq!(summary["loaded"], true);
    assert_eq!(summary["running"], true);
    assert_eq!(summary["managedPlistPath"], path_string(&managed_path));

    let discover = run_ocm(&cwd, &env, &["service", "discover", "--json"]);
    assert!(discover.status.success(), "{}", stderr(&discover));
    let discovered: Value = serde_json::from_str(&stdout(&discover)).unwrap();
    let services = discovered["services"].as_array().unwrap();
    assert!(services.iter().any(|service| {
        service["label"] == managed_label && service["sourceKind"] == "ocm-managed"
    }));
    assert!(services.iter().any(|service| {
        service["label"] == "ai.openclaw.gateway" && service["sourceKind"] == "openclaw-global"
    }));
}

#[test]
fn systemd_status_and_discover_prefer_live_show_details_when_the_loaded_unit_is_stale() {
    let root = TestDir::new("service-status-systemd-live-drift");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let stable = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(stable.status.success(), "{}", stderr(&stable));

    let dev_dir = root.child("openclaw-dev");
    fs::create_dir_all(&dev_dir).unwrap();
    let dev = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "dev",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &path_string(&dev_dir),
        ],
    );
    assert!(dev.status.success(), "{}", stderr(&dev));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let rebound = run_ocm(&cwd, &env, &["env", "set-launcher", "demo", "dev"]);
    assert!(rebound.status.success(), "{}", stderr(&rebound));

    let restart = run_ocm(&cwd, &env, &["service", "restart", "demo"]);
    assert!(restart.status.success(), "{}", stderr(&restart));

    let managed_label = managed_service_label(&env, &cwd, "demo");
    let managed_path = managed_service_definition_path(&env, &cwd, "demo");
    let live_port = 18795_u16;
    let live_root = path_string(&root.child("ocm-home/envs/demo"));
    let live_state_dir = format!("{live_root}/.openclaw");
    let live_config_path = format!("{live_state_dir}/openclaw.json");
    write_text(
        &managed_path.with_extension("show"),
        &format!(
            "LoadState=loaded\nUnitFileState=enabled\nActiveState=active\nSubState=running\nMainPID=4242\nFragmentPath={}\nExecStart=/bin/sh -lc \"/bin/true gateway run --port {}\"\nWorkingDirectory={}\nEnvironment=\"OPENCLAW_CONFIG_PATH={}\" \"OPENCLAW_STATE_DIR={}\" \"OPENCLAW_HOME={}\" \"OPENCLAW_GATEWAY_PORT={}\"\n",
            path_string(&managed_path),
            live_port,
            live_root,
            live_config_path,
            live_state_dir,
            live_root,
            live_port,
        ),
    );

    let status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let summary: Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(summary["definitionDrift"], true);
    assert_eq!(summary["gatewayPort"], live_port);
    assert!(
        summary["command"]
            .as_str()
            .unwrap()
            .contains("/bin/true gateway run --port 18795")
    );
    assert!(
        summary["issue"]
            .as_str()
            .unwrap()
            .contains("service restart demo")
    );

    let discover = run_ocm(&cwd, &env, &["service", "discover", "--json"]);
    assert!(discover.status.success(), "{}", stderr(&discover));
    let discovered: Value = serde_json::from_str(&stdout(&discover)).unwrap();
    let service = discovered["services"]
        .as_array()
        .unwrap()
        .iter()
        .find(|service| service["label"] == managed_label)
        .unwrap();
    assert_eq!(service["gatewayPort"], live_port);
    assert_eq!(service["workingDirectory"], live_root);
    assert!(
        service["programArguments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "/bin/true gateway run --port 18795")
    );
}

#[test]
fn systemd_service_logs_use_journalctl() {
    let root = TestDir::new("service-logs-systemd");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_systemd_tools(&root, &mut env);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "/bin/true"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let output = run_ocm(&cwd, &env, &["service", "logs", "demo", "--json"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let summary: Value = serde_json::from_str(&stdout(&output)).unwrap();
    let managed_label = managed_service_label(&env, &cwd, "demo");
    assert_eq!(
        summary["path"],
        format!("journalctl --user --unit {managed_label}")
    );
    assert_eq!(summary["content"], "gateway ok\n");

    let journalctl_log = fs::read_to_string(root.child("journalctl.log")).unwrap();
    assert!(journalctl_log.contains(&format!("--user --unit {managed_label}")));
}
