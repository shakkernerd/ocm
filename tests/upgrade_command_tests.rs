mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::{self, sleep};
use std::time::Duration;

use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use ocm::store::{now_utc, supervisor_runtime_path, supervisor_state_path};
use ocm::supervisor::{SupervisorRuntimeChild, SupervisorRuntimeService, SupervisorRuntimeState};
use serde_json::Value;
use sha2::{Digest, Sha512};
use tar::{Builder, Header};

use crate::support::{
    TestDir, TestHttpServer, install_fake_launchctl, install_fake_node_and_npm, ocm_env,
    path_string, run_ocm, stderr, stdout, write_executable_script,
};

fn append_tar_file(
    builder: &mut Builder<&mut GzEncoder<Vec<u8>>>,
    path: &str,
    body: &[u8],
    mode: u32,
) {
    let mut header = Header::new_gnu();
    header.set_size(body.len() as u64);
    header.set_mode(mode);
    header.set_cksum();
    builder.append_data(&mut header, path, body).unwrap();
}

fn openclaw_package_tarball(script_body: &str, version: &str) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);
        append_tar_file(
            &mut builder,
            "package/openclaw.mjs",
            script_body.as_bytes(),
            0o755,
        );
        append_tar_file(
            &mut builder,
            "package/package.json",
            format!(
                "{{\"name\":\"openclaw\",\"version\":\"{version}\",\"bin\":{{\"openclaw\":\"openclaw.mjs\"}}}}"
            )
            .as_bytes(),
            0o644,
        );
        builder.finish().unwrap();
    }
    encoder.finish().unwrap()
}

fn write_running_supervisor_runtime(
    runtime_path: &Path,
    ocm_home: &str,
    binding_name: &str,
    pid: u32,
    child_port: u32,
) {
    let log_root = runtime_path.parent().unwrap();
    let stdout_path = path_string(&log_root.join("demo.stdout.log"));
    let stderr_path = path_string(&log_root.join("demo.stderr.log"));
    let runtime = SupervisorRuntimeState {
        kind: "ocm-supervisor-runtime".to_string(),
        ocm_home: ocm_home.to_string(),
        updated_at: now_utc(),
        services: vec![SupervisorRuntimeService {
            env_name: "demo".to_string(),
            binding_kind: "runtime".to_string(),
            binding_name: binding_name.to_string(),
            gateway_state: "running".to_string(),
            restart_handoff: Some("none".to_string()),
            restart_count: 0,
            child_port,
            pid: Some(pid),
            stdout_path: stdout_path.clone(),
            stderr_path: stderr_path.clone(),
            last_exit_code: None,
            last_error: None,
            last_event_at: None,
            next_retry_at: None,
        }],
        children: vec![SupervisorRuntimeChild {
            env_name: "demo".to_string(),
            binding_kind: "runtime".to_string(),
            binding_name: binding_name.to_string(),
            pid,
            restart_count: 0,
            child_port,
            stdout_path,
            stderr_path,
        }],
    };
    fs::write(runtime_path, serde_json::to_vec(&runtime).unwrap()).unwrap();
}

fn recording_openclaw_script(version: &str) -> String {
    format!(
        r#"#!/bin/sh
home="${{OPENCLAW_HOME:-$PWD}}"
mkdir -p "$home"
command_log="${{OCM_TEST_COMMAND_LOG:-$home/sim-commands.log}}"
printf '%s\n' "$*" >> "$command_log"
has_arg() {{
  needle="$1"
  shift
  for arg in "$@"; do
    if [ "$arg" = "$needle" ]; then
      return 0
    fi
  done
  return 1
}}
case "$1" in
  --version)
    printf '{version}\n'
    exit 0
    ;;
  update)
    if [ "$2" = "finalize" ]; then
      if [ "${{OCM_TEST_FAIL_UPDATE_FINALIZE:-}}" = "1" ]; then
        echo "forced update finalize failure" >&2
        exit 23
      fi
      has_arg "--json" "$@" && has_arg "--yes" "$@" && has_arg "--no-restart" "$@" || {{
        echo "missing update finalize flags" >&2
        exit 1
      }}
      if [ "${{OPENCLAW_UPDATE_IN_PROGRESS:-}}" != "1" ]; then
        echo "missing OPENCLAW_UPDATE_IN_PROGRESS" >&2
        exit 1
      fi
      if [ "${{OPENCLAW_UPDATE_PARENT_SUPPORTS_DOCTOR_CONFIG_WRITE:-}}" != "1" ]; then
        echo "missing OPENCLAW_UPDATE_PARENT_SUPPORTS_DOCTOR_CONFIG_WRITE" >&2
        exit 1
      fi
      printf '{{"status":"ok","mode":"finalize","postUpdate":{{"doctor":{{"status":"ok"}},"plugins":{{"status":"ok"}}}}}}\n'
      exit 0
    fi
    printf '{{"dryRun":true}}\n'
    exit 0
    ;;
  config)
    [ "$2" = "validate" ] || {{
      echo "unexpected config args: $*" >&2
      exit 1
    }}
    if [ "${{OCM_TEST_INVALID_CONFIG_UNTIL_DOCTOR:-}}" = "1" ] &&
       [ ! -f "$home/.openclaw/config-repaired" ]; then
      echo "OpenClaw config is invalid" >&2
      exit 1
    fi
    echo "Config valid"
    exit 0
    ;;
  doctor)
    has_arg "--non-interactive" "$@" && has_arg "--fix" "$@" || {{
      echo "missing doctor update flags" >&2
      exit 1
    }}
    if [ "${{OPENCLAW_UPDATE_IN_PROGRESS:-}}" != "1" ]; then
      echo "missing OPENCLAW_UPDATE_IN_PROGRESS" >&2
      exit 1
    fi
    if [ "${{OCM_TEST_REQUIRE_DOCTOR_OWNERSHIP_FLAGS:-}}" = "1" ]; then
      if [ "${{OPENCLAW_UPDATE_PARENT_SUPPORTS_GATEWAY_RESTART:-}}" != "1" ] ||
         [ "${{OPENCLAW_UPDATE_PARENT_ALLOWS_GATEWAY_SERVICE_REPAIR:-}}" != "0" ] ||
         [ "${{OPENCLAW_UPDATE_PARENT_ALLOWS_GATEWAY_ACTIVATION:-}}" != "0" ] ||
         [ "${{OPENCLAW_SERVICE_REPAIR_POLICY:-}}" != "external" ]; then
        echo "missing doctor service ownership flags" >&2
        exit 1
      fi
    fi
    if [ "${{OCM_TEST_FAIL_DOCTOR:-}}" = "1" ]; then
      echo "forced doctor failure" >&2
      exit 29
    fi
    mkdir -p "$home/.openclaw"
    if [ "${{OCM_TEST_DOCTOR_LEAVES_CONFIG_INVALID:-}}" != "1" ]; then
      touch "$home/.openclaw/config-repaired"
    fi
    echo "doctor ok"
    exit 0
    ;;
  plugins)
    [ "$2" = "update" ] && has_arg "--all" "$@" || {{
      echo "missing plugin update flags" >&2
      exit 1
    }}
    echo "No tracked plugins or hook packs to update."
    exit 0
    ;;
  gateway)
    [ "$2" = "status" ] && has_arg "--deep" "$@" && has_arg "--json" "$@" || {{
      echo "missing gateway status flags" >&2
      exit 1
    }}
    if [ "${{OCM_TEST_GATEWAY_AUTH_HANDSHAKE:-}}" = "1" ]; then
      printf '{{"rpc":{{"ok":false,"error":"device identity required"}}}}\n'
      exit "${{OCM_TEST_GATEWAY_STATUS_EXIT_CODE:-0}}"
    elif [ "${{OCM_TEST_GATEWAY_UNREADY:-}}" = "1" ]; then
      printf '{{"rpc":{{"ok":false,"error":"gateway RPC is not ready"}}}}\n'
    else
      printf '{{"rpc":{{"ok":true}}}}\n'
    fi
    exit 0
    ;;
esac
echo "unexpected args: $*" >&2
exit 1
"#
    )
}

fn destructive_finalize_openclaw_script(version: &str) -> String {
    format!(
        r#"#!/bin/sh
home="${{OPENCLAW_HOME:-$PWD}}"
case "$1" in
  --version)
    printf '{version}\n'
    exit 0
    ;;
  update)
    if [ "$2" = "finalize" ]; then
      rm -rf "$home/.openclaw/npm"
      printf '{{"status":"ok","mode":"finalize"}}\n'
      exit 0
    fi
    ;;
esac
echo "unexpected args: $*" >&2
exit 1
"#
    )
}

fn prebinding_guard_openclaw_script(version: &str, expected_runtime: &str) -> String {
    format!(
        r#"#!/bin/sh
home="${{OPENCLAW_HOME:-$PWD}}"
mkdir -p "$home"
printf '%s\n' "$*" >> "$home/sim-commands.log"
case "$1" in
  --version)
    printf '{version}\n'
    exit 0
    ;;
  update)
    if [ "$2" = "finalize" ]; then
      if ! grep -q '"defaultRuntime": "{expected_runtime}"' "$OCM_HOME/envs.json"; then
        echo "replacement runtime was published before update finalization" >&2
        exit 24
      fi
      printf '{{"status":"ok"}}\n'
      exit 0
    fi
    ;;
esac
echo "unexpected args: $*" >&2
exit 1
"#
    )
}

fn scenario_sensitive_openclaw_script(version: &str) -> String {
    format!(
        r#"#!/bin/sh
case "$1" in
  --version)
    printf '{version}\n'
    exit 0
    ;;
  update)
    printf '{{"dryRun":true}}\n'
    exit 0
    ;;
  doctor)
    if grep -q '"telegram"' "${{OPENCLAW_CONFIG_PATH:-/dev/null}}"; then
      echo "Error: Cannot find module 'grammy'" >&2
      exit 1
    fi
    echo "doctor ok"
    exit 0
    ;;
  plugins)
    echo "No tracked plugins or hook packs to update."
    exit 0
    ;;
  gateway)
    printf '{{"rpc":{{"ok":true}}}}\n'
    exit 0
    ;;
esac
echo "unexpected args: $*" >&2
exit 1
"#
    )
}

fn sha512_integrity(body: &[u8]) -> String {
    let digest = Sha512::digest(body);
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}

fn init_openclaw_repo(root: &TestDir) -> PathBuf {
    let repo = root.child("repo/openclaw");
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"name":"openclaw","version":"2026.4.20-local"}"#,
    )
    .unwrap();
    fs::write(repo.join("scripts/run-node.mjs"), "console.log('run');\n").unwrap();
    fs::write(
        repo.join("scripts/watch-node.mjs"),
        "console.log('watch');\n",
    )
    .unwrap();

    let init = Command::new("git").arg("init").arg(&repo).output().unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let email = Command::new("git")
        .args([
            "-C",
            &path_string(&repo),
            "config",
            "user.email",
            "tests@example.com",
        ])
        .output()
        .unwrap();
    assert!(
        email.status.success(),
        "{}",
        String::from_utf8_lossy(&email.stderr)
    );
    let name = Command::new("git")
        .args([
            "-C",
            &path_string(&repo),
            "config",
            "user.name",
            "OCM Tests",
        ])
        .output()
        .unwrap();
    assert!(
        name.status.success(),
        "{}",
        String::from_utf8_lossy(&name.stderr)
    );
    let add = Command::new("git")
        .args(["-C", &path_string(&repo), "add", "."])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let commit = Command::new("git")
        .args(["-C", &path_string(&repo), "commit", "-m", "init"])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );

    repo
}

fn prepend_fake_bin(env: &mut std::collections::BTreeMap<String, String>, bin_dir: &Path) {
    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
}

fn install_fake_simulation_pnpm(
    root: &TestDir,
    env: &mut std::collections::BTreeMap<String, String>,
) {
    let bin_dir = root.child("fake-sim-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let pnpm = r#"#!/bin/sh
case "$1" in
  install)
    mkdir -p node_modules/.pnpm node_modules/.bin
    touch node_modules/.bin/tsx
    exit 0
    ;;
  build|ui:build)
    echo "$1 ok"
    exit 0
    ;;
  openclaw)
    shift
    case "$1" in
      --version)
        echo "2026.4.20-local"
        exit 0
        ;;
      doctor)
        echo "Error: Cannot find module 'grammy'" >&2
        exit 1
        ;;
      plugins)
        echo "No tracked plugins or hook packs to update."
        exit 0
        ;;
      gateway)
        echo "{\"status\":\"ok\"}"
        exit 0
        ;;
      *)
        echo "fake local openclaw $*"
        exit 0
        ;;
    esac
    ;;
esac
echo "unexpected pnpm $*" >&2
exit 1
"#;
    write_executable_script(&bin_dir.join("pnpm"), pnpm);
    prepend_fake_bin(env, &bin_dir);
}

#[test]
fn upgrade_updates_a_tracked_runtime_and_refreshes_the_service() {
    let root = TestDir::new("upgrade-tracked-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );

    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    install_fake_launchctl(&root, &mut env);

    let start = run_ocm(&cwd, &env, &["start", "demo"]);
    assert!(start.status.success(), "{}", stderr(&start));

    env.insert(
        "OCM_TEST_GATEWAY_AUTH_HANDSHAKE".to_string(),
        "1".to_string(),
    );
    env.insert(
        "OCM_TEST_GATEWAY_STATUS_EXIT_CODE".to_string(),
        "1".to_string(),
    );
    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=updated"), "{output}");
    assert!(output.contains("service=started"), "{output}");
    assert!(output.contains("snapshot="), "{output}");
    assert!(output.contains("version=2026.3.25"), "{output}");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.25");

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let env_json: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
    let env_root = Path::new(env_json["root"].as_str().unwrap());
    let command_log = fs::read_to_string(env_root.join("sim-commands.log")).unwrap();
    assert!(
        command_log.contains("update finalize --json --yes --no-restart"),
        "{command_log}"
    );
    assert!(command_log.contains("--version"), "{command_log}");
    assert!(
        !command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        !command_log.contains("plugins update --all\n"),
        "{command_log}"
    );

    let snapshots = run_ocm(&cwd, &env, &["env", "snapshot", "list", "demo", "--json"]);
    assert!(snapshots.status.success(), "{}", stderr(&snapshots));
    let snapshot_json: Value = serde_json::from_str(&stdout(&snapshots)).unwrap();
    assert_eq!(snapshot_json.as_array().unwrap().len(), 1);
    assert_eq!(snapshot_json[0]["label"], "pre-upgrade");

    let history = run_ocm(&cwd, &env, &["upgrade", "history", "demo", "--json"]);
    assert!(history.status.success(), "{}", stderr(&history));
    let history_json: Value = serde_json::from_str(&stdout(&history)).unwrap();
    let record = &history_json[0];
    assert_eq!(record["kind"], "ocm-upgrade-transaction");
    assert_eq!(record["formatVersion"], 1);
    assert_eq!(record["source"]["name"], "stable");
    assert_eq!(record["source"]["openclawVersion"], "2026.3.24");
    assert_eq!(record["target"]["name"], "stable");
    assert_eq!(record["target"]["openclawVersion"], "2026.3.25");
    assert_eq!(record["snapshotId"], snapshot_json[0]["id"]);
    assert_eq!(record["outcome"], "updated");
    assert_eq!(record["migration"]["status"], "validated");
    assert_eq!(record["finalization"]["status"], "completed");
    assert_eq!(record["serviceBefore"]["running"], true);
    assert_eq!(record["serviceAfter"]["running"], true);
    assert!(record["note"].is_null());
    assert_eq!(record["runtimeRecovery"][0]["runtimeName"], "stable");
    assert_eq!(record["runtimeRecovery"][0]["releaseVersion"], "2026.3.24");
    assert_eq!(record["runtimeRecovery"][0]["backupId"], "stable");

    let ocm_home = Path::new(env.get("OCM_HOME").unwrap());
    let recovery_root = ocm_home
        .join("upgrade-history")
        .join("demo")
        .join(format!("{}.recovery", record["id"].as_str().unwrap()))
        .join("stable");
    let recovery_meta: Value =
        serde_json::from_slice(&fs::read(recovery_root.join("runtime.json")).unwrap()).unwrap();
    assert_eq!(recovery_meta["name"], "stable");
    assert_eq!(recovery_meta["releaseVersion"], "2026.3.24");
    assert!(recovery_root.join("files").is_dir());
    assert_eq!(
        fs::read_to_string(recovery_root.parent().unwrap().join("snapshot-id")).unwrap(),
        snapshot_json[0]["id"].as_str().unwrap()
    );

    let remove_snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "remove",
            "demo",
            snapshot_json[0]["id"].as_str().unwrap(),
        ],
    );
    assert!(
        remove_snapshot.status.success(),
        "{}",
        stderr(&remove_snapshot)
    );
    assert!(!recovery_root.exists());
}

#[test]
fn upgrade_rolls_back_when_gateway_rpc_is_not_ready() {
    let root = TestDir::new("upgrade-gateway-readiness-rollback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let health_server =
        TestHttpServer::serve_bytes_times("/health", "application/json", br#"{"ok":true}"#, 8);
    let health_url = health_server.url();
    let health_port = health_url
        .split(':')
        .nth(2)
        .and_then(|value| value.split('/').next())
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    install_fake_launchctl(&root, &mut env);

    let port = health_port.to_string();
    let start = run_ocm(&cwd, &env, &["start", "demo", "--port", port.as_str()]);
    assert!(start.status.success(), "{}", stderr(&start));

    let runtime_path = supervisor_runtime_path(&env, &cwd).unwrap();
    fs::create_dir_all(runtime_path.parent().unwrap()).unwrap();
    let ocm_home = env.get("OCM_HOME").unwrap().clone();
    write_running_supervisor_runtime(&runtime_path, &ocm_home, "stable", 4242, health_port);

    let state_path = supervisor_state_path(&env, &cwd).unwrap();
    let replacement_runtime_path = runtime_path.clone();
    let replacement_ocm_home = ocm_home.clone();
    let restart_observer = thread::spawn(move || {
        for _ in 0..200 {
            let state = fs::read_to_string(&state_path).unwrap_or_default();
            if state.contains("restartRequests") && state.contains("\"envName\": \"demo\"") {
                write_running_supervisor_runtime(
                    &replacement_runtime_path,
                    &replacement_ocm_home,
                    "stable",
                    4243,
                    health_port,
                );
                return;
            }
            sleep(Duration::from_millis(25));
        }
        panic!("upgrade did not request a supervised gateway restart");
    });

    env.insert("OCM_TEST_GATEWAY_UNREADY".to_string(), "1".to_string());
    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo"]);
    restart_observer.join().unwrap();

    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=rolled-back"), "{output}");
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(
        output.contains("post-upgrade gateway readiness failed: gateway RPC is not ready"),
        "{output}"
    );
    assert!(!health_server.requests().is_empty());

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");

    let history = run_ocm(&cwd, &env, &["upgrade", "history", "demo", "--json"]);
    assert!(history.status.success(), "{}", stderr(&history));
    let history_json: Value = serde_json::from_str(&stdout(&history)).unwrap();
    let record = &history_json[0];
    assert_eq!(record["source"]["openclawVersion"], "2026.3.24");
    assert_eq!(record["target"]["openclawVersion"], "2026.3.25");
    assert_eq!(record["outcome"], "rolled-back");
    assert_eq!(record["rollback"], "restored");
    assert_eq!(record["migration"]["status"], "validated");
    assert_eq!(record["finalization"]["status"], "completed");
    assert_eq!(record["serviceAfter"]["running"], true);
    assert!(record["note"].is_null());
    let recovery_root = Path::new(env.get("OCM_HOME").unwrap())
        .join("upgrade-history")
        .join("demo")
        .join(format!("{}.recovery", record["id"].as_str().unwrap()));
    assert!(!recovery_root.exists());
}

#[test]
fn upgrade_rejects_mutating_a_runtime_shared_with_another_env() {
    let root = TestDir::new("upgrade-shared-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );
    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    for name in ["primary", "sibling"] {
        let start = run_ocm(&cwd, &env, &["start", name, "--no-service"]);
        assert!(start.status.success(), "{}", stderr(&start));
    }

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "primary"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let error = stderr(&upgrade);
    assert!(error.contains("runtime \"stable\" is shared"), "{error}");
    assert!(error.contains("\"sibling\""), "{error}");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");

    for name in ["primary", "sibling"] {
        let version = run_ocm(&cwd, &env, &["env", "run", name, "--", "--version"]);
        assert!(version.status.success(), "{}", stderr(&version));
        assert_eq!(stdout(&version).trim(), "2026.3.24");

        let snapshots = run_ocm(&cwd, &env, &["env", "snapshot", "list", name, "--json"]);
        assert!(snapshots.status.success(), "{}", stderr(&snapshots));
        let snapshot_json: Value = serde_json::from_str(&stdout(&snapshots)).unwrap();
        assert!(snapshot_json.as_array().unwrap().is_empty());
    }
}

#[test]
fn upgrade_rejects_mutating_a_target_runtime_bound_to_another_env() {
    let root = TestDir::new("upgrade-shared-target-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );
    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let source_runtime = root.child("source-openclaw");
    write_executable_script(&source_runtime, &recording_openclaw_script("source-local"));
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let add_source = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "source-local",
            "--path",
            &source_runtime.display().to_string(),
        ],
    );
    assert!(add_source.status.success(), "{}", stderr(&add_source));
    let create_source = run_ocm(
        &cwd,
        &env,
        &["env", "create", "primary", "--runtime", "source-local"],
    );
    assert!(create_source.status.success(), "{}", stderr(&create_source));
    let start_sibling = run_ocm(&cwd, &env, &["start", "sibling", "--no-service"]);
    assert!(start_sibling.status.success(), "{}", stderr(&start_sibling));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "primary", "--channel", "stable"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let error = stderr(&upgrade);
    assert!(error.contains("runtime \"stable\" is shared"), "{error}");
    assert!(error.contains("\"sibling\""), "{error}");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");

    let source = run_ocm(&cwd, &env, &["env", "show", "primary", "--json"]);
    assert!(source.status.success(), "{}", stderr(&source));
    let source_json: Value = serde_json::from_str(&stdout(&source)).unwrap();
    assert_eq!(source_json["defaultRuntime"], "source-local");

    let snapshots = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "list", "primary", "--json"],
    );
    assert!(snapshots.status.success(), "{}", stderr(&snapshots));
    let snapshot_json: Value = serde_json::from_str(&stdout(&snapshots)).unwrap();
    assert!(snapshot_json.as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn upgrade_switches_across_versions_from_runtime_with_broken_package_bin_symlink() {
    let root = TestDir::new("upgrade-broken-runtime-symlink-versions");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let pairs = (0..10)
        .map(|index| {
            (
                format!("2026.3.{}", 20 + index),
                format!("2026.4.{}", 20 + index),
            )
        })
        .collect::<Vec<_>>();
    let versions = pairs
        .iter()
        .flat_map(|(source, target)| [source.clone(), target.clone()])
        .collect::<Vec<_>>();
    let mut tarball_servers = Vec::new();
    let mut version_entries = Vec::new();
    let mut time_entries = Vec::new();
    for version in versions {
        let tarball = openclaw_package_tarball(&recording_openclaw_script(&version), &version);
        let integrity = sha512_integrity(&tarball);
        let path = format!("/openclaw-{version}.tgz");
        let server =
            TestHttpServer::serve_bytes_times(&path, "application/octet-stream", &tarball, 4);
        version_entries.push(format!(
            "\"{version}\":{{\"version\":\"{version}\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{integrity}\"}}}}",
            server.url()
        ));
        time_entries.push(format!("\"{version}\":\"2026-03-25T16:35:52.000Z\""));
        tarball_servers.push(server);
    }
    let latest = pairs.last().unwrap().1.as_str();
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"{latest}\"}},\"versions\":{{{}}},\"time\":{{{}}}}}",
        version_entries.join(","),
        time_entries.join(",")
    );
    let packument_server = TestHttpServer::serve_bytes_times(
        "/openclaw",
        "application/json",
        packument.as_bytes(),
        80,
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    for (index, (source_version, target_version)) in pairs.iter().enumerate() {
        let env_name = format!("demo-{index}");
        let start = run_ocm(
            &cwd,
            &env,
            &[
                "start",
                &env_name,
                "--version",
                source_version,
                "--no-service",
            ],
        );
        assert!(start.status.success(), "{}", stderr(&start));

        let runtime = run_ocm(&cwd, &env, &["runtime", "show", source_version, "--json"]);
        assert!(runtime.status.success(), "{}", stderr(&runtime));
        let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
        let install_root = Path::new(runtime_json["installRoot"].as_str().unwrap());
        let bin_dir = install_root.join(format!(
            "files/node_modules/openclaw/dist/extensions/demo-{index}/node_modules/.bin"
        ));
        fs::create_dir_all(&bin_dir).unwrap();
        let broken_link = bin_dir.join("missing-tool");
        std::os::unix::fs::symlink("../missing-package/bin/missing-tool", &broken_link).unwrap();
        assert!(
            fs::symlink_metadata(&broken_link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(!broken_link.exists());

        let verify = run_ocm(&cwd, &env, &["runtime", "verify", source_version, "--json"]);
        assert!(verify.status.success(), "{}", stderr(&verify));

        let upgrade = run_ocm(
            &cwd,
            &env,
            &["upgrade", &env_name, "--version", target_version],
        );
        assert!(
            upgrade.status.success(),
            "{source_version} -> {target_version}\nstdout:\n{}\nstderr:\n{}",
            stdout(&upgrade),
            stderr(&upgrade)
        );
        let output = stdout(&upgrade);
        assert!(
            output.contains(&format!("from=runtime:{source_version}")),
            "{output}"
        );
        assert!(
            output.contains(&format!("to=runtime:{target_version}")),
            "{output}"
        );
        assert!(output.contains("outcome=switched"), "{output}");
        assert!(output.contains("snapshot="), "{output}");

        let env_show = run_ocm(&cwd, &env, &["env", "show", &env_name, "--json"]);
        assert!(env_show.status.success(), "{}", stderr(&env_show));
        let env_json: Value = serde_json::from_str(&stdout(&env_show)).unwrap();
        assert_eq!(env_json["defaultRuntime"], *target_version);
    }

    assert_eq!(tarball_servers.len(), 20);
}

#[test]
fn upgrade_rejects_downgrade_before_snapshot_or_target_download() {
    let root = TestDir::new("upgrade-reject-downgrade");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let older_version = "2026.7.1-2";
    let newer_version = "2026.7.2-beta.3";
    let older_tarball =
        openclaw_package_tarball(&recording_openclaw_script(older_version), older_version);
    let older_integrity = sha512_integrity(&older_tarball);
    let older_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.7.1-2.tgz",
        "application/octet-stream",
        &older_tarball,
        4,
    );
    let newer_tarball =
        openclaw_package_tarball(&recording_openclaw_script(newer_version), newer_version);
    let newer_integrity = sha512_integrity(&newer_tarball);
    let newer_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.7.2-beta.3.tgz",
        "application/octet-stream",
        &newer_tarball,
        4,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"{newer_version}\",\"beta\":\"{newer_version}\"}},\"versions\":{{\"{older_version}\":{{\"version\":\"{older_version}\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{older_integrity}\"}}}},\"{newer_version}\":{{\"version\":\"{newer_version}\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{newer_integrity}\"}}}}}},\"time\":{{\"{older_version}\":\"2026-07-18T03:53:48.967Z\",\"{newer_version}\":\"2026-07-18T23:15:12.160Z\"}}}}",
        older_server.url(),
        newer_server.url()
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 8);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &["start", "demo", "--version", newer_version, "--no-service"],
    );
    assert!(start.status.success(), "{}", stderr(&start));
    assert!(older_server.requests().is_empty());

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--version", older_version]);
    assert_eq!(upgrade.status.code(), Some(1));
    let error = stderr(&upgrade);
    assert!(
        error.contains(&format!(
            "refusing to downgrade env \"demo\" from OpenClaw {newer_version} to {older_version}"
        )),
        "{error}"
    );
    assert!(error.contains("SQLite state"), "{error}");
    assert!(older_server.requests().is_empty());

    let environment = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(environment.status.success(), "{}", stderr(&environment));
    let environment: Value = serde_json::from_str(&stdout(&environment)).unwrap();
    assert_eq!(environment["defaultRuntime"], newer_version);

    let target = run_ocm(&cwd, &env, &["runtime", "show", older_version]);
    assert!(!target.status.success());

    let install_target = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "--version", older_version],
    );
    assert!(
        install_target.status.success(),
        "{}",
        stderr(&install_target)
    );
    let older_requests_before_named_target = older_server.requests().len();
    let named_upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", older_version]);
    assert_eq!(named_upgrade.status.code(), Some(1));
    assert!(
        stderr(&named_upgrade).contains(&format!(
            "refusing to downgrade env \"demo\" from OpenClaw {newer_version} to {older_version}"
        )),
        "{}",
        stderr(&named_upgrade)
    );
    assert_eq!(
        older_server.requests().len(),
        older_requests_before_named_target
    );

    let snapshots = run_ocm(&cwd, &env, &["env", "snapshot", "list", "demo", "--json"]);
    assert!(snapshots.status.success(), "{}", stderr(&snapshots));
    let snapshots: Value = serde_json::from_str(&stdout(&snapshots)).unwrap();
    assert!(snapshots.as_array().unwrap().is_empty());
}

#[test]
fn upgrade_accepts_correction_release_and_freezes_channel_selection() {
    let root = TestDir::new("upgrade-correction-release");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let stable_version = "2026.7.1";
    let correction_version = "2026.7.1-2";
    let stable_tarball =
        openclaw_package_tarball(&recording_openclaw_script(stable_version), stable_version);
    let stable_integrity = sha512_integrity(&stable_tarball);
    let stable_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.7.1.tgz",
        "application/octet-stream",
        &stable_tarball,
        4,
    );
    let correction_tarball = openclaw_package_tarball(
        &recording_openclaw_script(correction_version),
        correction_version,
    );
    let correction_integrity = sha512_integrity(&correction_tarball);
    let correction_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.7.1-2.tgz",
        "application/octet-stream",
        &correction_tarball,
        4,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"{correction_version}\"}},\"versions\":{{\"{stable_version}\":{{\"version\":\"{stable_version}\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{stable_integrity}\"}}}},\"{correction_version}\":{{\"version\":\"{correction_version}\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{correction_integrity}\"}}}}}},\"time\":{{\"{stable_version}\":\"2026-07-13T17:58:18.920Z\",\"{correction_version}\":\"2026-07-18T03:53:48.967Z\"}}}}",
        stable_server.url(),
        correction_server.url()
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 8);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &["start", "demo", "--version", stable_version, "--no-service"],
    );
    assert!(start.status.success(), "{}", stderr(&start));
    let requests_before_upgrade = packument_server.requests().len();

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--channel", "stable"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    assert_eq!(
        packument_server.requests().len() - requests_before_upgrade,
        1
    );
    assert_eq!(correction_server.requests().len(), 1);

    let environment = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(environment.status.success(), "{}", stderr(&environment));
    let environment: Value = serde_json::from_str(&stdout(&environment)).unwrap();
    assert_eq!(environment["defaultRuntime"], "stable");
    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime["releaseVersion"], correction_version);
}

#[test]
fn upgrade_dry_run_reports_without_changing_runtime_or_creating_snapshot() {
    let root = TestDir::new("upgrade-dry-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball = openclaw_package_tarball(
        "#!/usr/bin/env node\nconsole.log('2026.3.24');\n",
        "2026.3.24",
    );
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 2);

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let dry_run = run_ocm(&cwd, &env, &["upgrade", "demo", "--dry-run"]);
    assert!(dry_run.status.success(), "{}", stderr(&dry_run));
    let output = stdout(&dry_run);
    assert!(output.contains("outcome=would-update"), "{output}");
    assert!(output.contains("dry run"), "{output}");
    assert!(!output.contains("snapshot="), "{output}");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");

    let snapshots = run_ocm(&cwd, &env, &["env", "snapshot", "list", "demo", "--json"]);
    assert!(snapshots.status.success(), "{}", stderr(&snapshots));
    let snapshot_json: Value = serde_json::from_str(&stdout(&snapshots)).unwrap();
    assert_eq!(snapshot_json.as_array().unwrap().len(), 0);

    let history = run_ocm(&cwd, &env, &["upgrade", "history", "demo", "--json"]);
    assert!(history.status.success(), "{}", stderr(&history));
    let history_json: Value = serde_json::from_str(&stdout(&history)).unwrap();
    assert_eq!(history_json.as_array().unwrap().len(), 0);
}

#[test]
fn upgrade_simulate_tests_a_published_version_without_changing_the_source_env() {
    let root = TestDir::new("upgrade-simulate-version");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball = openclaw_package_tarball("#!/bin/sh\nprintf '2026.3.24\\n'\n", "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball = openclaw_package_tarball("#!/bin/sh\nprintf '2026.3.25\\n'\n", "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));
    let source_state = root.child("ocm-home/envs/demo/.openclaw");
    let source_config = source_state.join("openclaw.json");
    let included_config = source_state.join("config/base.json");
    fs::create_dir_all(included_config.parent().unwrap()).unwrap();
    fs::write(
        &included_config,
        fs::read_to_string(&source_config).unwrap(),
    )
    .unwrap();
    fs::write(
        &source_config,
        "{\n  \"$include\": \"./config/base.json\"\n}\n",
    )
    .unwrap();

    let simulate = run_ocm(
        &cwd,
        &env,
        &["upgrade", "simulate", "demo", "--to", "2026.3.25", "--json"],
    );
    assert!(
        simulate.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&simulate),
        stderr(&simulate)
    );
    let json: Value = serde_json::from_str(&stdout(&simulate)).unwrap();
    assert_eq!(json["sourceEnv"], "demo");
    assert_eq!(json["outcome"], "passed");
    assert_eq!(json["toBindingKind"], "runtime");
    let runtime_name = json["toBindingName"].as_str().unwrap();
    assert!(
        runtime_name.starts_with("ocm-sim-runtime-"),
        "{runtime_name}"
    );
    assert_eq!(json["cleanup"], "cleaned");
    let sim_name = json["simulationEnv"].as_str().unwrap();
    assert_ne!(sim_name, "demo");

    let source = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(source.status.success(), "{}", stderr(&source));
    let source_json: Value = serde_json::from_str(&stdout(&source)).unwrap();
    assert_eq!(source_json["defaultRuntime"], "stable");
    assert_eq!(
        fs::read_to_string(&source_config).unwrap(),
        "{\n  \"$include\": \"./config/base.json\"\n}\n"
    );
    assert!(included_config.exists());

    let simulation = run_ocm(&cwd, &env, &["env", "show", sim_name, "--json"]);
    assert!(!simulation.status.success(), "{}", stdout(&simulation));
    assert!(
        stderr(&simulation).contains("does not exist"),
        "{}",
        stderr(&simulation)
    );

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", runtime_name, "--json"]);
    assert!(!runtime.status.success(), "{}", stdout(&runtime));
    assert!(
        stderr(&runtime).contains("does not exist"),
        "{}",
        stderr(&runtime)
    );
}

#[test]
fn upgrade_simulate_errors_before_cloning_when_published_target_is_missing() {
    let root = TestDir::new("upgrade-simulate-missing-target");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let tarball = openclaw_package_tarball("#!/bin/sh\nprintf '2026.3.24\\n'\n", "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
        10,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25-beta.1\":{{\"version\":\"2026.3.25-beta.1\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25-beta.1\":\"2026-03-26T09:00:00.000Z\"}}}}",
        tarball_server.url(),
        integrity,
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 4);

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &["start", "demo", "--version", "2026.3.24", "--no-service"],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let simulate = run_ocm(
        &cwd,
        &env,
        &[
            "upgrade",
            "simulate",
            "demo",
            "--to",
            "2026.3.25",
            "--scenario",
            "all",
        ],
    );
    assert!(!simulate.status.success(), "{}", stdout(&simulate));
    assert!(stdout(&simulate).is_empty(), "{}", stdout(&simulate));
    let error = stderr(&simulate);
    assert!(
        error.contains("OpenClaw release version \"2026.3.25\" was not found"),
        "{error}"
    );
    assert!(
        error.contains("simulation did not create any scenario envs"),
        "{error}"
    );
    assert!(error.contains("2026.3.25-beta.1"), "{error}");

    let list = run_ocm(&cwd, &env, &["env", "list", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let envs: Value = serde_json::from_str(&stdout(&list)).unwrap();
    let envs = envs.as_array().unwrap();
    assert_eq!(envs.len(), 1, "{envs:#?}");
    assert_eq!(envs[0]["name"], "demo");
}

#[test]
fn upgrade_simulate_runs_openclaw_update_contract_checks_for_published_targets() {
    let root = TestDir::new("upgrade-simulate-contract");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let simulate = run_ocm(
        &cwd,
        &env,
        &[
            "upgrade",
            "simulate",
            "demo",
            "--to",
            "2026.3.25",
            "--keep-simulations",
            "--json",
        ],
    );
    assert!(
        simulate.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&simulate),
        stderr(&simulate)
    );
    let json: Value = serde_json::from_str(&stdout(&simulate)).unwrap();
    assert_eq!(json["outcome"], "passed");
    assert_eq!(json["cleanup"], "kept");
    let runtime_name = json["toBindingName"].as_str().unwrap();
    assert!(
        runtime_name.starts_with("ocm-sim-runtime-"),
        "{runtime_name}"
    );
    let sim_name = json["simulationEnv"].as_str().unwrap();
    let simulation = run_ocm(&cwd, &env, &["env", "show", sim_name, "--json"]);
    assert!(simulation.status.success(), "{}", stderr(&simulation));
    let simulation_json: Value = serde_json::from_str(&stdout(&simulation)).unwrap();
    let sim_root = Path::new(simulation_json["root"].as_str().unwrap());
    let command_log = fs::read_to_string(sim_root.join("sim-commands.log")).unwrap();

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", runtime_name, "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    assert!(
        command_log.contains("update --dry-run --json --no-restart --yes --tag 2026.3.25"),
        "{command_log}"
    );
    assert!(command_log.contains("--version"), "{command_log}");
    assert!(
        command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        command_log.contains("plugins update --all --dry-run"),
        "{command_log}"
    );
    assert!(
        command_log.contains("gateway status --deep --json"),
        "{command_log}"
    );
}

#[test]
fn upgrade_simulate_all_scenarios_reports_plugin_specific_failures() {
    let root = TestDir::new("upgrade-simulate-scenarios");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball = openclaw_package_tarball(
        &scenario_sensitive_openclaw_script("2026.4.15"),
        "2026.4.15",
    );
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.4.15.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball = openclaw_package_tarball(
        &scenario_sensitive_openclaw_script("2026.4.20"),
        "2026.4.20",
    );
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.4.20.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.4.15\"}},\"versions\":{{\"2026.4.15\":{{\"version\":\"2026.4.15\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.4.15\":\"2026-04-15T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.4.20\"}},\"versions\":{{\"2026.4.15\":{{\"version\":\"2026.4.15\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.4.20\":{{\"version\":\"2026.4.20\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.4.15\":\"2026-04-15T16:35:52.000Z\",\"2026.4.20\":\"2026-04-20T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "scenario-source",
            "--version",
            "2026.4.15",
            "--no-service",
        ],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let simulate = run_ocm(
        &cwd,
        &env,
        &[
            "upgrade",
            "simulate",
            "scenario-source",
            "--to",
            "2026.4.20",
            "--scenario",
            "all",
            "--json",
        ],
    );
    assert!(!simulate.status.success(), "{}", stdout(&simulate));
    let json: Value = serde_json::from_str(&stdout(&simulate)).unwrap();
    assert_eq!(json["count"], 3);
    assert_eq!(json["passed"], 2);
    assert_eq!(json["failed"], 1);

    let results = json["results"].as_array().unwrap();
    let current = results
        .iter()
        .find(|entry| entry["scenario"] == "current")
        .unwrap();
    let minimum = results
        .iter()
        .find(|entry| entry["scenario"] == "minimum")
        .unwrap();
    let telegram = results
        .iter()
        .find(|entry| entry["scenario"] == "telegram")
        .unwrap();
    assert_eq!(current["outcome"], "passed");
    assert_eq!(minimum["outcome"], "passed");
    assert_eq!(telegram["outcome"], "failed");
    assert_eq!(current["cleanup"], "cleaned");
    assert_eq!(minimum["cleanup"], "cleaned");
    assert_eq!(telegram["cleanup"], "cleaned");
    assert_eq!(current["toBindingName"], minimum["toBindingName"]);
    assert_eq!(minimum["toBindingName"], telegram["toBindingName"]);
    assert!(
        telegram["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "openclaw doctor"
                && check["status"] == "failed"
                && check["note"]
                    .as_str()
                    .unwrap()
                    .contains("Cannot find module 'grammy'")),
        "{telegram:#}"
    );

    let source = run_ocm(&cwd, &env, &["env", "show", "scenario-source", "--json"]);
    assert!(source.status.success(), "{}", stderr(&source));
    let source_json: Value = serde_json::from_str(&stdout(&source)).unwrap();
    assert_eq!(source_json["defaultRuntime"], "2026.4.15");
    assert_eq!(source_json["serviceEnabled"], false);

    let env_list = run_ocm(&cwd, &env, &["env", "list", "--json"]);
    assert!(env_list.status.success(), "{}", stderr(&env_list));
    let envs: Value = serde_json::from_str(&stdout(&env_list)).unwrap();
    let envs = envs.as_array().unwrap();
    assert_eq!(envs.len(), 1, "{envs:#?}");
    assert_eq!(envs[0]["name"], "scenario-source");
}

#[test]
fn upgrade_simulate_reports_local_repo_doctor_failures() {
    let root = TestDir::new("upgrade-simulate-local-repo");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let repo = init_openclaw_repo(&root);

    let tarball = openclaw_package_tarball("console.log('2026.3.24');\n", "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
        10,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 1);

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    install_fake_simulation_pnpm(&root, &mut env);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let simulate = run_ocm(
        &cwd,
        &env,
        &[
            "upgrade",
            "simulate",
            "demo",
            "--to",
            &path_string(&repo),
            "--raw",
        ],
    );
    assert!(!simulate.status.success(), "{}", stdout(&simulate));
    let output = stdout(&simulate);
    assert!(output.contains("outcome=failed"), "{output}");
    assert!(output.contains("check=openclaw doctor"), "{output}");
    assert!(output.contains("Cannot find module 'grammy'"), "{output}");

    let source = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(source.status.success(), "{}", stderr(&source));
    let source_json: Value = serde_json::from_str(&stdout(&source)).unwrap();
    assert_eq!(source_json["defaultRuntime"], "stable");
    assert!(source_json["devRepoRoot"].is_null());
}

#[test]
fn upgrade_rolls_back_runtime_when_service_restart_fails() {
    let root = TestDir::new("upgrade-service-rollback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );

    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    install_fake_launchctl(&root, &mut env);

    let start = run_ocm(&cwd, &env, &["start", "demo"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let launchctl_bin = env.get("OCM_INTERNAL_LAUNCHCTL_BIN").unwrap();
    write_executable_script(
        std::path::Path::new(launchctl_bin),
        "#!/bin/sh\ncase \"$1\" in\n  managername)\n    exit 0\n    ;;\n  print)\n    printf 'state = waiting\\n'\n    exit 0\n    ;;\n  bootout|unload)\n    exit 0\n    ;;\n  bootstrap)\n    echo 'forced bootstrap failure' >&2\n    exit 1\n    ;;\n  *)\n    exit 0\n    ;;\nesac\n",
    );

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--version", "2026.3.25"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(
        output.contains("outcome=rolled-back"),
        "stdout:\n{output}\nstderr:\n{}",
        stderr(&upgrade)
    );
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(output.contains("snapshot="), "{output}");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");

    let target_runtime = run_ocm(&cwd, &env, &["runtime", "show", "2026.3.25", "--json"]);
    assert!(
        !target_runtime.status.success(),
        "{}",
        stdout(&target_runtime)
    );
    assert!(
        stderr(&target_runtime).contains("runtime \"2026.3.25\" does not exist"),
        "{}",
        stderr(&target_runtime)
    );
}

#[test]
fn upgrade_restores_runtime_when_runtime_preparation_fails() {
    let root = TestDir::new("upgrade-runtime-prepare-rollback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball = openclaw_package_tarball("console.log('2026.3.24');\n", "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );

    let new_tarball = openclaw_package_tarball("console.log('2026.3.25');\n", "2026.3.25");
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"sha512-not-valid\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo"]);
    assert!(
        !upgrade.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        stdout(&upgrade),
        stderr(&upgrade)
    );
    let output = stdout(&upgrade);
    assert!(
        output.contains("outcome=rolled-back"),
        "stdout:\n{output}\nstderr:\n{}",
        stderr(&upgrade)
    );
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(
        output.contains("runtime artifact integrity is invalid"),
        "{output}"
    );

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");
}

#[test]
fn upgrade_rolls_back_when_post_upgrade_version_verification_fails() {
    let root = TestDir::new("upgrade-version-verification-rollback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
    );

    let wrong_version_tarball = openclaw_package_tarball(
        &destructive_finalize_openclaw_script("2026.3.24"),
        "2026.3.25",
    );
    let wrong_version_integrity = sha512_integrity(&wrong_version_tarball);
    let wrong_version_tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &wrong_version_tarball,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        wrong_version_tarball_server.url(),
        wrong_version_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let state_root = root.child("ocm-home/envs/demo/.openclaw");
    let plugin_payloads = [
        (
            "npm/projects/demo/package.json",
            "{\"name\":\"@example/demo\"}\n",
        ),
        (
            "npm/projects/demo/package-lock.json",
            "{\"lockfileVersion\":3}\n",
        ),
        (
            "npm/projects/demo/node_modules/demo/index.js",
            "module.exports = 'restored';\n",
        ),
    ];
    for (path, contents) in plugin_payloads {
        let path = state_root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=rolled-back"), "{output}");
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(
        output.contains("post-upgrade version verification failed"),
        "{output}"
    );

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["releaseVersion"], "2026.3.24");
    for (path, contents) in plugin_payloads {
        assert_eq!(
            fs::read_to_string(state_root.join(path)).unwrap(),
            contents,
            "{path}"
        );
    }
}

#[test]
fn upgrade_reports_pinned_envs_without_moving_them() {
    let root = TestDir::new("upgrade-pinned");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let tarball = openclaw_package_tarball("console.log('2026.3.24');\n", "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 2);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &["start", "pinned", "--version", "2026.3.24", "--no-service"],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "pinned"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=pinned"), "{output}");
    assert!(output.contains("exact release"), "{output}");
}

#[test]
fn upgrade_can_switch_a_local_launcher_env_to_a_published_runtime() {
    let root = TestDir::new("upgrade-launcher-to-runtime");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();

    let tarball = openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 2);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "hacking",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &project_dir.display().to_string(),
            "--no-service",
        ],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "hacking", "--channel", "stable"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("from=launcher:hacking.local"), "{output}");
    assert!(output.contains("to=runtime:stable"), "{output}");
    assert!(output.contains("outcome=switched"), "{output}");

    let show = run_ocm(&cwd, &env, &["env", "show", "hacking", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "stable");
    assert!(env_json["defaultLauncher"].is_null());
}

#[test]
fn upgrade_can_switch_env_to_an_installed_runtime() {
    let root = TestDir::new("upgrade-installed-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(
        &new_runtime,
        &prebinding_guard_openclaw_script("new-openclaw", "old-local"),
    );

    let env = ocm_env(&root);
    let add_old = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "old-local",
            "--path",
            &old_runtime.display().to_string(),
        ],
    );
    assert!(add_old.status.success(), "{}", stderr(&add_old));

    let add_new = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "new-local",
            "--path",
            &new_runtime.display().to_string(),
        ],
    );
    assert!(add_new.status.success(), "{}", stderr(&add_new));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "new-local"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("from=runtime:old-local"), "{output}");
    assert!(output.contains("to=runtime:new-local"), "{output}");
    assert!(output.contains("outcome=switched"), "{output}");

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "new-local");
    let env_root = Path::new(env_json["root"].as_str().unwrap());
    let command_log = fs::read_to_string(env_root.join("sim-commands.log")).unwrap();
    assert!(
        command_log.contains("update finalize --json --yes --no-restart"),
        "{command_log}"
    );
    assert!(
        !command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        !command_log.contains("plugins update --all\n"),
        "{command_log}"
    );
}

#[test]
fn upgrade_reuses_the_bound_named_runtime_without_retaining_recovery_bytes() {
    let root = TestDir::new("upgrade-reuse-bound-runtime");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let runtime_path = root.child("local-openclaw");
    write_executable_script(&runtime_path, &recording_openclaw_script("local-openclaw"));

    let env = ocm_env(&root);
    let add = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "local",
            "--path",
            &runtime_path.display().to_string(),
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(&cwd, &env, &["env", "create", "demo", "--runtime", "local"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "local"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=up-to-date"), "{output}");

    let history = run_ocm(&cwd, &env, &["upgrade", "history", "demo", "--json"]);
    assert!(history.status.success(), "{}", stderr(&history));
    let history_json: Value = serde_json::from_str(&stdout(&history)).unwrap();
    let record = &history_json[0];
    assert_eq!(record["source"]["name"], "local");
    assert_eq!(record["target"]["name"], "local");
    assert_eq!(record["runtimeRecovery"][0]["runtimeName"], "local");
    assert!(record["runtimeRecovery"][0]["backupId"].is_null());

    let recovery_root = Path::new(env.get("OCM_HOME").unwrap())
        .join("upgrade-history")
        .join("demo")
        .join(format!("{}.recovery", record["id"].as_str().unwrap()));
    assert!(!recovery_root.exists());
}

#[test]
fn upgrade_repairs_target_config_before_finalization() {
    let root = TestDir::new("upgrade-target-config-repair");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

    let mut env = ocm_env(&root);
    for (name, runtime) in [("old-local", &old_runtime), ("new-local", &new_runtime)] {
        let add = run_ocm(
            &cwd,
            &env,
            &[
                "runtime",
                "add",
                name,
                "--path",
                &runtime.display().to_string(),
            ],
        );
        assert!(add.status.success(), "{}", stderr(&add));
    }

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));
    let env_root = root.child("ocm-home/envs/demo");
    fs::write(
        env_root.join(".openclaw/openclaw.json"),
        "{\"meta\":{\"lastTouchedAt\":\"legacy\"}}\n",
    )
    .unwrap();

    env.insert(
        "OCM_TEST_INVALID_CONFIG_UNTIL_DOCTOR".to_string(),
        "1".to_string(),
    );
    env.insert(
        "OCM_TEST_REQUIRE_DOCTOR_OWNERSHIP_FLAGS".to_string(),
        "1".to_string(),
    );
    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "new-local"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=switched"), "{output}");
    assert!(output.contains("config repair"), "{output}");

    let command_log = fs::read_to_string(env_root.join("sim-commands.log")).unwrap();
    let validate_before = command_log.find("config validate").unwrap();
    let doctor = command_log.find("doctor --non-interactive --fix").unwrap();
    let validate_after = command_log.rfind("config validate").unwrap();
    let finalize = command_log
        .find("update finalize --json --yes --no-restart")
        .unwrap();
    assert!(validate_before < doctor, "{command_log}");
    assert!(doctor < validate_after, "{command_log}");
    assert!(validate_after < finalize, "{command_log}");
    assert!(env_root.join(".openclaw/config-repaired").exists());

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "new-local");
}

#[test]
fn upgrade_rolls_back_when_target_config_doctor_fails() {
    let root = TestDir::new("upgrade-target-config-doctor-failure");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

    let mut env = ocm_env(&root);
    for (name, runtime) in [("old-local", &old_runtime), ("new-local", &new_runtime)] {
        let add = run_ocm(
            &cwd,
            &env,
            &[
                "runtime",
                "add",
                name,
                "--path",
                &runtime.display().to_string(),
            ],
        );
        assert!(add.status.success(), "{}", stderr(&add));
    }

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));
    let env_root = root.child("ocm-home/envs/demo");
    let config_path = env_root.join(".openclaw/openclaw.json");
    let original_config = "{\"meta\":{\"lastTouchedAt\":\"legacy\"}}\n";
    fs::write(&config_path, original_config).unwrap();

    env.insert(
        "OCM_TEST_INVALID_CONFIG_UNTIL_DOCTOR".to_string(),
        "1".to_string(),
    );
    env.insert("OCM_TEST_FAIL_DOCTOR".to_string(), "1".to_string());
    let command_log_path = root.child("doctor-failure-commands.log");
    env.insert(
        "OCM_TEST_COMMAND_LOG".to_string(),
        path_string(&command_log_path),
    );
    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "new-local"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=rolled-back"), "{output}");
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(output.contains("openclaw doctor failed"), "{output}");

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "old-local");
    assert_eq!(fs::read_to_string(config_path).unwrap(), original_config);
    assert!(!env_root.join(".openclaw/config-repaired").exists());

    let command_log = fs::read_to_string(command_log_path).unwrap();
    assert!(command_log.contains("config validate"), "{command_log}");
    assert!(
        command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        !command_log.contains("update finalize --json --yes --no-restart"),
        "{command_log}"
    );
}

#[test]
fn upgrade_rolls_back_when_doctor_does_not_repair_target_config() {
    let root = TestDir::new("upgrade-target-config-revalidation-failure");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

    let mut env = ocm_env(&root);
    for (name, runtime) in [("old-local", &old_runtime), ("new-local", &new_runtime)] {
        let add = run_ocm(
            &cwd,
            &env,
            &[
                "runtime",
                "add",
                name,
                "--path",
                &runtime.display().to_string(),
            ],
        );
        assert!(add.status.success(), "{}", stderr(&add));
    }

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));
    let env_root = root.child("ocm-home/envs/demo");
    let config_path = env_root.join(".openclaw/openclaw.json");
    let original_config = "{\"meta\":{\"lastTouchedAt\":\"legacy\"}}\n";
    fs::write(&config_path, original_config).unwrap();

    env.insert(
        "OCM_TEST_INVALID_CONFIG_UNTIL_DOCTOR".to_string(),
        "1".to_string(),
    );
    env.insert(
        "OCM_TEST_DOCTOR_LEAVES_CONFIG_INVALID".to_string(),
        "1".to_string(),
    );
    let command_log_path = root.child("revalidation-failure-commands.log");
    env.insert(
        "OCM_TEST_COMMAND_LOG".to_string(),
        path_string(&command_log_path),
    );
    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "new-local"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=rolled-back"), "{output}");
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(
        output.contains("openclaw config validate after doctor failed"),
        "{output}"
    );

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "old-local");
    assert_eq!(fs::read_to_string(config_path).unwrap(), original_config);

    let command_log = fs::read_to_string(command_log_path).unwrap();
    assert_eq!(command_log.matches("config validate").count(), 2);
    assert!(
        command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        !command_log.contains("update finalize --json --yes --no-restart"),
        "{command_log}"
    );
}

#[test]
fn upgrade_skips_config_repair_when_openclaw_config_is_missing() {
    let root = TestDir::new("upgrade-missing-config");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

    let env = ocm_env(&root);
    for (name, runtime) in [("old-local", &old_runtime), ("new-local", &new_runtime)] {
        let add = run_ocm(
            &cwd,
            &env,
            &[
                "runtime",
                "add",
                name,
                "--path",
                &runtime.display().to_string(),
            ],
        );
        assert!(add.status.success(), "{}", stderr(&add));
    }

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));
    let env_root = root.child("ocm-home/envs/demo");
    let config_path = env_root.join(".openclaw/openclaw.json");
    assert!(!config_path.exists());

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "new-local"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=switched"), "{output}");
    assert!(!output.contains("config repair"), "{output}");

    let command_log = fs::read_to_string(env_root.join("sim-commands.log")).unwrap();
    assert!(!command_log.contains("config validate"), "{command_log}");
    assert!(
        !command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        command_log.contains("update finalize --json --yes --no-restart"),
        "{command_log}"
    );
    assert!(!config_path.exists());

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "new-local");
}

#[test]
fn env_set_runtime_uses_the_transactional_upgrade_path_for_existing_bindings() {
    let root = TestDir::new("set-runtime-transactional-upgrade");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

    let mut env = ocm_env(&root);
    for (name, runtime) in [("old-local", &old_runtime), ("new-local", &new_runtime)] {
        let add = run_ocm(
            &cwd,
            &env,
            &[
                "runtime",
                "add",
                name,
                "--path",
                &runtime.display().to_string(),
            ],
        );
        assert!(add.status.success(), "{}", stderr(&add));
    }

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    env.insert("OCM_TEST_FAIL_UPDATE_FINALIZE".to_string(), "1".to_string());
    let bind = run_ocm(&cwd, &env, &["env", "set-runtime", "demo", "new-local"]);
    assert!(!bind.status.success(), "{}", stdout(&bind));
    assert!(
        stderr(&bind).contains("restored the pre-upgrade snapshot"),
        "{}",
        stderr(&bind)
    );

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "old-local");
}

#[test]
fn upgrade_rolls_back_runtime_binding_when_update_finalization_fails() {
    let root = TestDir::new("upgrade-finalize-rollback");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let old_runtime = root.child("old-openclaw");
    let new_runtime = root.child("new-openclaw");
    write_executable_script(&old_runtime, &recording_openclaw_script("old-openclaw"));
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

    let mut env = ocm_env(&root);
    let add_old = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "old-local",
            "--path",
            &old_runtime.display().to_string(),
        ],
    );
    assert!(add_old.status.success(), "{}", stderr(&add_old));

    let add_new = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "new-local",
            "--path",
            &new_runtime.display().to_string(),
        ],
    );
    assert!(add_new.status.success(), "{}", stderr(&add_new));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "old-local"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    fs::write(
        root.child("ocm-home/envs/demo/.openclaw/openclaw.json"),
        r#"{"agents":{"list":[{"id":"main","default":true},{"id":"clawforce"}]}}"#,
    )
    .unwrap();
    let secondary_skill =
        root.child("ocm-home/envs/demo/.openclaw/workspace-clawforce/skills/social/SKILL.md");
    fs::create_dir_all(secondary_skill.parent().unwrap()).unwrap();
    fs::write(&secondary_skill, "skill before upgrade\n").unwrap();

    env.insert("OCM_TEST_FAIL_UPDATE_FINALIZE".to_string(), "1".to_string());
    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo", "--runtime", "new-local"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=rolled-back"), "{output}");
    assert!(output.contains("rollback=restored"), "{output}");
    assert!(
        output.contains("openclaw update finalize failed"),
        "{output}"
    );

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let env_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(env_json["defaultRuntime"], "old-local");
    assert_eq!(
        fs::read_to_string(secondary_skill).unwrap(),
        "skill before upgrade\n"
    );
}

#[test]
fn upgrade_keeps_a_stopped_installed_service_stopped() {
    let root = TestDir::new("upgrade-stopped-service-start");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();

    let tarball = openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
    );
    let packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        tarball_server.url(),
        integrity
    );
    let packument_server =
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 2);
    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    install_fake_launchctl(&root, &mut env);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );

    let start = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "hacking",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &project_dir.display().to_string(),
        ],
    );
    assert!(start.status.success(), "{}", stderr(&start));

    let stop = run_ocm(&cwd, &env, &["service", "stop", "hacking"]);
    assert!(stop.status.success(), "{}", stderr(&stop));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "hacking", "--channel", "stable"]);
    assert!(upgrade.status.success(), "{}", stderr(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("from=launcher:hacking.local"), "{output}");
    assert!(output.contains("to=runtime:stable"), "{output}");
    assert!(!output.contains("service="), "{output}");
}

#[test]
fn upgrade_all_updates_safe_envs_and_skips_local_or_pinned_ones() {
    let root = TestDir::new("upgrade-all");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();

    let old_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.24"), "2026.3.24");
    let old_integrity = sha512_integrity(&old_tarball);
    let old_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &old_tarball,
        10,
    );
    let new_tarball =
        openclaw_package_tarball(&recording_openclaw_script("2026.3.25"), "2026.3.25");
    let new_integrity = sha512_integrity(&new_tarball);
    let new_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &new_tarball,
        10,
    );

    let initial_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity
    );
    let updated_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T09:00:00.000Z\"}}}}",
        old_tarball_server.url(),
        old_integrity,
        new_tarball_server.url(),
        new_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![
            initial_packument.as_bytes().to_vec(),
            initial_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
            updated_packument.as_bytes().to_vec(),
        ],
    );

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.22.3");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let stable = run_ocm(&cwd, &env, &["start", "stable-env", "--no-service"]);
    assert!(stable.status.success(), "{}", stderr(&stable));
    let pinned = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "pinned-env",
            "--version",
            "2026.3.24",
            "--no-service",
        ],
    );
    assert!(pinned.status.success(), "{}", stderr(&pinned));
    let local = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "local-env",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &project_dir.display().to_string(),
            "--no-service",
        ],
    );
    assert!(local.status.success(), "{}", stderr(&local));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "--all", "--json"]);
    assert!(
        upgrade.status.success(),
        "stderr:\n{}\nstdout:\n{}",
        stderr(&upgrade),
        stdout(&upgrade)
    );
    let json: Value = serde_json::from_str(&stdout(&upgrade)).unwrap();
    assert_eq!(json["count"], 3);
    assert_eq!(json["changed"], 1);
    assert_eq!(json["current"], 0);
    assert_eq!(json["skipped"], 2);
    assert_eq!(json["failed"], 0);
}

#[test]
fn upgrade_all_json_exits_failed_when_any_env_fails() {
    let root = TestDir::new("upgrade-all-json-failed");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let runtime_path = root.child("openclaw");
    write_executable_script(&runtime_path, "#!/bin/sh\necho fake-openclaw\n");

    let env = ocm_env(&root);
    let add_runtime = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "missing-soon",
            "--path",
            &runtime_path.display().to_string(),
        ],
    );
    assert!(add_runtime.status.success(), "{}", stderr(&add_runtime));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "broken", "--runtime", "missing-soon"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/runtimes/missing-soon.json")).unwrap();

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "--all", "--json"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let json: Value = serde_json::from_str(&stdout(&upgrade)).unwrap();
    assert_eq!(json["count"], 1);
    assert_eq!(json["failed"], 1);
    assert_eq!(json["results"][0]["outcome"], "failed");
    assert!(
        json["results"][0]["note"]
            .as_str()
            .unwrap()
            .contains("runtime \"missing-soon\" does not exist")
    );
}
