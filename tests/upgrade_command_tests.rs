mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use base64::Engine;
use flate2::{Compression, write::GzEncoder};
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

fn recording_openclaw_script(version: &str) -> String {
    format!(
        r#"#!/bin/sh
home="${{OPENCLAW_HOME:-$PWD}}"
mkdir -p "$home"
printf '%s\n' "$*" >> "$home/sim-commands.log"
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
    printf '{{"dryRun":true}}\n'
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
    printf '{{"gatewayState":"simulation-ok"}}\n'
    exit 0
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
    printf '{{"gatewayState":"simulation-ok"}}\n'
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
        command_log.contains("doctor --non-interactive --fix"),
        "{command_log}"
    );
    assert!(
        command_log.contains("plugins update --all"),
        "{command_log}"
    );
    assert!(
        !command_log.contains("plugins update --all --dry-run"),
        "{command_log}"
    );

    let snapshots = run_ocm(&cwd, &env, &["env", "snapshot", "list", "demo", "--json"]);
    assert!(snapshots.status.success(), "{}", stderr(&snapshots));
    let snapshot_json: Value = serde_json::from_str(&stdout(&snapshots)).unwrap();
    assert_eq!(snapshot_json.as_array().unwrap().len(), 1);
    assert_eq!(snapshot_json[0]["label"], "pre-upgrade");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
        TestHttpServer::serve_bytes_times("/openclaw", "application/json", packument.as_bytes(), 1);

    let mut env = ocm_env(&root);
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    assert!(output.contains("outcome=rolled-back"), "{output}");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-service"]);
    assert!(start.status.success(), "{}", stderr(&start));

    let upgrade = run_ocm(&cwd, &env, &["upgrade", "demo"]);
    assert!(!upgrade.status.success(), "{}", stdout(&upgrade));
    let output = stdout(&upgrade);
    assert!(output.contains("outcome=rolled-back"), "{output}");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    write_executable_script(&new_runtime, &recording_openclaw_script("new-openclaw"));

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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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
    install_fake_node_and_npm(&root, &mut env, "22.14.0");
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

    let remove_runtime = run_ocm(&cwd, &env, &["runtime", "remove", "missing-soon"]);
    assert!(
        remove_runtime.status.success(),
        "{}",
        stderr(&remove_runtime)
    );

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
