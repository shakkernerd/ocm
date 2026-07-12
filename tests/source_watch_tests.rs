mod support;

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde_json::{Value, json};

use crate::support::{
    TestDir, install_fake_service_manager, ocm_env, path_string, run_ocm, stderr, stdout,
    write_executable_script,
};

fn install_fake_node(root: &TestDir, env: &mut BTreeMap<String, String>) -> PathBuf {
    let bin_dir = root.child("fake-node-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let node_log = root.child("node.log");
    let node = format!(
        "#!/bin/sh\nprintf 'pwd=%s|args=%s|bundled=%s|devroot=%s\\n' \"$PWD\" \"$*\" \"$OPENCLAW_BUNDLED_PLUGINS_DIR\" \"$OPENCLAW_DEV_SOURCE_ROOT\" >> \"{}\"\nprintf 'node-ok\\n'\n",
        path_string(&node_log)
    );
    write_executable_script(&bin_dir.join("node"), &node);

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    env.insert(
        "PATH".to_string(),
        if existing_path.is_empty() {
            path_string(&bin_dir)
        } else {
            format!("{}:{existing_path}", path_string(&bin_dir))
        },
    );
    node_log
}

fn create_source_repo(root: &TestDir) -> PathBuf {
    let repo = root.child("source/openclaw");
    fs::create_dir_all(repo.join("extensions/codex")).unwrap();
    fs::write(repo.join("openclaw.mjs"), "console.log('openclaw');\n").unwrap();
    fs::write(
        repo.join("extensions/codex/openclaw.plugin.json"),
        r#"{"id":"codex"}"#,
    )
    .unwrap();
    repo
}

fn create_runtime_backed_env(
    root: &TestDir,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> PathBuf {
    let runtime_path = root.child("bin/stable-openclaw");
    fs::create_dir_all(runtime_path.parent().unwrap()).unwrap();
    write_executable_script(&runtime_path, "#!/bin/sh\nprintf 'runtime-ok\\n'\n");

    let runtime = run_ocm(
        cwd,
        env,
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
        cwd,
        env,
        &[
            "env",
            "create",
            "demo",
            "--runtime",
            "stable",
            "--port",
            "21901",
        ],
    );
    assert!(created.status.success(), "{}", stderr(&created));
    runtime_path
}

struct SourceWatchFixture {
    lock_file: File,
}

impl Drop for SourceWatchFixture {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock_file);
    }
}

fn write_active_source_watch_override(root: &TestDir, source_repo: &Path) -> SourceWatchFixture {
    let path = root.child("ocm-home/source-watch/demo.json");
    let lock_path = root.child("ocm-home/source-watch/demo.lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)
        .unwrap();
    FileExt::lock_exclusive(&lock_file).unwrap();
    fs::write(
        path,
        json!({
            "kind": "ocm-source-watch-override",
            "envName": "demo",
            "repoRoot": path_string(source_repo),
            "watchPid": std::process::id(),
            "token": "<redacted>",
            "startedAt": "2026-06-17T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();
    SourceWatchFixture { lock_file }
}

#[test]
fn source_watch_override_takes_precedence_for_resolve_and_run() {
    let root = TestDir::new("source-watch-resolve-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let source_repo = create_source_repo(&root);
    let mut env = ocm_env(&root);
    let node_log = install_fake_node(&root, &mut env);
    let runtime_path = create_runtime_backed_env(&root, &cwd, &env);
    let _source_watch = write_active_source_watch_override(&root, &source_repo);

    let entry = source_repo.join("openclaw.mjs");
    let resolve = run_ocm(
        &cwd,
        &env,
        &["env", "resolve", "demo", "--json", "--", "status"],
    );
    assert!(resolve.status.success(), "{}", stderr(&resolve));
    let resolved: Value = serde_json::from_str(&stdout(&resolve)).unwrap();
    assert_eq!(resolved["bindingKind"], "source-watch");
    assert_eq!(resolved["bindingName"], "source-watch");
    assert_eq!(
        resolved["command"],
        format!("node {} status", path_string(&entry))
    );
    assert_eq!(resolved["binaryPath"], path_string(&entry));
    assert_eq!(resolved["runDir"], path_string(&source_repo));
    assert_eq!(
        resolved["forwardedArgs"],
        Value::Array(vec![Value::String("status".to_string())])
    );

    let explicit = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "resolve",
            "demo",
            "--runtime",
            "stable",
            "--json",
            "--",
            "status",
        ],
    );
    assert!(explicit.status.success(), "{}", stderr(&explicit));
    let explicit: Value = serde_json::from_str(&stdout(&explicit)).unwrap();
    assert_eq!(explicit["bindingKind"], "runtime");
    assert_eq!(explicit["bindingName"], "stable");
    assert_eq!(explicit["binaryPath"], path_string(&runtime_path));

    let run = run_ocm(&cwd, &env, &["@demo", "--", "status"]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run), "node-ok\n");

    let node_log = fs::read_to_string(node_log).unwrap();
    assert!(node_log.contains(&format!(
        "args={} status|bundled={}|devroot={}",
        path_string(&entry),
        path_string(&source_repo.join("extensions")),
        path_string(&source_repo)
    )));
}

#[test]
fn source_watch_override_with_a_live_reused_pid_is_removed_without_its_lease() {
    let root = TestDir::new("source-watch-stale-pid");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let source_repo = create_source_repo(&root);
    let env = ocm_env(&root);
    let runtime_path = create_runtime_backed_env(&root, &cwd, &env);
    let override_path = root.child("ocm-home/source-watch/demo.json");
    fs::create_dir_all(override_path.parent().unwrap()).unwrap();
    fs::write(
        &override_path,
        json!({
            "kind": "ocm-source-watch-override",
            "envName": "demo",
            "repoRoot": path_string(&source_repo),
            "watchPid": std::process::id(),
            "token": "<redacted>",
            "startedAt": "2026-06-17T00:00:00Z"
        })
        .to_string(),
    )
    .unwrap();

    let resolve = run_ocm(
        &cwd,
        &env,
        &["env", "resolve", "demo", "--json", "--", "status"],
    );
    assert!(resolve.status.success(), "{}", stderr(&resolve));
    let resolved: Value = serde_json::from_str(&stdout(&resolve)).unwrap();
    assert_eq!(resolved["bindingKind"], "runtime");
    assert_eq!(resolved["binaryPath"], path_string(&runtime_path));
    assert!(!override_path.exists());
}

#[test]
fn source_watch_override_flows_to_env_exec_and_status_surfaces() {
    let root = TestDir::new("source-watch-exec-status");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let source_repo = create_source_repo(&root);
    let mut env = ocm_env(&root);
    install_fake_service_manager(&root, &mut env);
    let node_log = install_fake_node(&root, &mut env);
    create_runtime_backed_env(&root, &cwd, &env);
    let _source_watch = write_active_source_watch_override(&root, &source_repo);

    let entry = source_repo.join("openclaw.mjs");
    let exec = run_ocm(
        &cwd,
        &env,
        &["env", "exec", "demo", "--", "openclaw", "status"],
    );
    assert!(exec.status.success(), "{}", stderr(&exec));
    assert_eq!(stdout(&exec), "node-ok\n");

    let inherited = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "exec",
            "demo",
            "--",
            "sh",
            "-lc",
            "printf '%s|%s' \"$OPENCLAW_DEV_SOURCE_ROOT\" \"$OPENCLAW_BUNDLED_PLUGINS_DIR\"",
        ],
    );
    assert!(inherited.status.success(), "{}", stderr(&inherited));
    assert_eq!(
        stdout(&inherited),
        format!(
            "{}|{}",
            path_string(&source_repo),
            path_string(&source_repo.join("extensions"))
        )
    );

    let env_status = run_ocm(&cwd, &env, &["env", "status", "demo", "--json"]);
    assert!(env_status.status.success(), "{}", stderr(&env_status));
    let env_status: Value = serde_json::from_str(&stdout(&env_status)).unwrap();
    assert_eq!(env_status["resolvedKind"], "source-watch");
    assert_eq!(env_status["binaryPath"], path_string(&entry));
    assert_eq!(env_status["runDir"], path_string(&source_repo));

    let service_status = run_ocm(&cwd, &env, &["service", "status", "demo", "--json"]);
    assert!(
        service_status.status.success(),
        "{}",
        stderr(&service_status)
    );
    let service_status: Value = serde_json::from_str(&stdout(&service_status)).unwrap();
    assert_eq!(service_status["bindingKind"], "source-watch");
    assert_eq!(
        service_status["command"],
        format!("node {} gateway run --port 21901", path_string(&entry))
    );
    assert_eq!(service_status["binaryPath"], "node");
    assert_eq!(service_status["args"][0], path_string(&entry));
    assert_eq!(service_status["args"][1], "gateway");
    assert_eq!(service_status["runDir"], path_string(&source_repo));

    let node_log = fs::read_to_string(node_log).unwrap();
    assert!(node_log.contains(&format!(
        "args={} status|bundled={}|devroot={}",
        path_string(&entry),
        path_string(&source_repo.join("extensions")),
        path_string(&source_repo)
    )));
}
