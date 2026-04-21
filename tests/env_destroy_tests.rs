mod support;

use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::net::{Ipv4Addr, SocketAddrV4, TcpStream};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::thread::sleep;
#[cfg(unix)]
use std::time::{Duration, Instant};

use crate::support::{
    TestDir, managed_service_definition_path, ocm_env, path_string, run_ocm, stderr, stdout,
    write_executable_script,
};

fn install_fake_launchctl(root: &TestDir, env: &mut BTreeMap<String, String>) {
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

fn ocm_launchd_env(root: &TestDir) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    env
}

fn init_openclaw_repo(root: &TestDir) -> PathBuf {
    let repo = root.child("repo/openclaw");
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"name":"openclaw","version":"2026.4.19"}"#,
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

fn prepend_fake_bin(env: &mut BTreeMap<String, String>, bin_dir: &Path) {
    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
}

fn install_fake_dev_runners(root: &TestDir, env: &mut BTreeMap<String, String>) {
    let bin_dir = root.child("fake-dev-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let pnpm_log = root.child("pnpm.log");
    let node_log = root.child("node.log");
    let pnpm = format!(
        "#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$PWD\" \"$OPENCLAW_CONFIG_PATH\" \"$OPENCLAW_GATEWAY_PORT\" \"$*\" >> \"{}\"\n",
        path_string(&pnpm_log)
    );
    let node = format!(
        "#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$PWD\" \"$OPENCLAW_CONFIG_PATH\" \"$OPENCLAW_GATEWAY_PORT\" \"$*\" >> \"{}\"\n",
        path_string(&node_log)
    );
    write_executable_script(&bin_dir.join("pnpm"), &pnpm);
    write_executable_script(&bin_dir.join("node"), &node);
    prepend_fake_bin(env, &bin_dir);
}

#[cfg(unix)]
fn wait_for_listener(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok() {
            return;
        }
        sleep(Duration::from_millis(50));
    }
    panic!("listener on port {port} did not start in time");
}

#[test]
fn env_destroy_preview_reports_service_snapshot_and_env_steps() {
    let root = TestDir::new("env-destroy-preview");
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

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "demo",
            "--label",
            "before-destroy",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let preview = run_ocm(&cwd, &env, &["env", "destroy", "demo"]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let output = stdout(&preview);
    assert!(output.contains("Destroy preview for env demo"));
    assert!(output.contains("snapshots: 1"));
    assert!(output.contains("service: ocm"));
    assert!(output.contains("snapshots: remove 1 env snapshot(s)"));
    assert!(output.contains("service: disable env gateway in the OCM background service"));
    assert!(output.contains("env: remove env root and metadata"));
    assert!(output.contains("re-run with --yes to destroy it"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
}

#[test]
fn env_destroy_yes_uninstalls_service_removes_snapshots_and_deletes_env() {
    let root = TestDir::new("env-destroy-apply");
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

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "demo",
            "--label",
            "before-destroy",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let service_path = managed_service_definition_path(&env, &cwd, "ocm");

    let destroy = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes"]);
    assert!(destroy.status.success(), "{}", stderr(&destroy));
    let output = stdout(&destroy);
    assert!(output.contains("Destroyed env demo"));
    assert!(output.contains("snapshots removed: 1"));
    assert!(output.contains("service removed: ocm"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(!show.status.success());
    assert!(stderr(&show).contains("environment \"demo\" does not exist"));

    assert!(service_path.exists());
    assert!(!root.child("ocm-home/snapshots/demo").exists());
}

#[test]
fn env_destroy_requires_force_for_protected_envs() {
    let root = TestDir::new("env-destroy-protected");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--protect"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let blocked = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes"]);
    assert!(!blocked.status.success());
    let output = stdout(&blocked);
    assert!(output.contains("Destroy preview for env demo"));
    assert!(output.contains("env is protected; re-run with --force to destroy it"));

    let forced = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(!show.status.success());
}

#[test]
fn env_destroy_yes_removes_dev_worktree() {
    let root = TestDir::new("env-destroy-dev-worktree");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);
    install_fake_dev_runners(&root, &mut env);

    let run = run_ocm(&cwd, &env, &["dev", "demo", "--repo", &path_string(&repo)]);
    assert!(run.status.success(), "{}", stderr(&run));

    let worktree = repo.join(".worktrees/demo");
    assert!(worktree.exists());

    let destroy = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes"]);
    assert!(destroy.status.success(), "{}", stderr(&destroy));
    assert!(!worktree.exists(), "dev worktree should be removed");
}

#[cfg(unix)]
#[test]
fn env_destroy_yes_terminates_live_listener_processes() {
    let root = TestDir::new("env-destroy-live-processes");
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

    let server = root.child("listener.py");
    fs::write(
        &server,
        r#"import argparse, socket, time
parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, required=True)
args = parser.parse_args()
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", args.port))
s.listen(1)
time.sleep(60)
"#,
    )
    .unwrap();

    let mut child = Command::new("sh")
        .current_dir(root.child("ocm-home/envs/demo"))
        .arg("-c")
        .arg(format!(
            "python3 {} --port 18789 >/dev/null 2>&1 & wait",
            path_string(&server)
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    wait_for_listener(18789);

    let destroy = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes"]);
    assert!(destroy.status.success(), "{}", stderr(&destroy));

    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        sleep(Duration::from_millis(50));
    }
    assert!(
        child.try_wait().unwrap().is_some(),
        "listener wrapper should exit after destroy"
    );
    assert!(
        TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 18789)).is_err(),
        "listener port should be closed after destroy"
    );
}
