mod support;

use std::fs;

use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use serde_json::Value;
use sha2::{Digest, Sha512};
use tar::{Builder, Header};

use crate::support::{
    TestDir, TestHttpServer, ocm_env, path_string, run_ocm, stderr, stdout,
    write_executable_script,
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

fn openclaw_package_tarball(script_body: &str) -> Vec<u8> {
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
            br#"{"name":"openclaw","version":"2026.3.24","bin":{"openclaw":"openclaw.mjs"}}"#,
            0o644,
        );
        builder.finish().unwrap();
    }
    encoder.finish().unwrap()
}

fn sha512_integrity(body: &[u8]) -> String {
    let digest = Sha512::digest(body);
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    )
}

#[test]
fn start_defaults_to_default_env_and_latest_stable_runtime() {
    let root = TestDir::new("start-default-stable");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let tarball = openclaw_package_tarball("#!/usr/bin/env node\nconsole.log('stable');\n");
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
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let start = run_ocm(&cwd, &env, &["start", "--no-onboard"]);
    assert!(start.status.success(), "{}", stderr(&start));
    let output = stdout(&start);
    assert!(output.contains("Started env default"));
    assert!(output.contains("runtime: stable"));
    assert!(output.contains("onboard: ocm @default -- onboard"));

    let show = run_ocm(&cwd, &env, &["env", "show", "default", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["name"], "default");
    assert_eq!(show_json["defaultRuntime"], "stable");

    let runtime = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let runtime_json: Value = serde_json::from_str(&stdout(&runtime)).unwrap();
    assert_eq!(runtime_json["name"], "stable");
    assert_eq!(runtime_json["releaseSelectorKind"], "channel");
    assert_eq!(runtime_json["releaseSelectorValue"], "stable");
}

#[test]
fn start_can_create_a_local_command_launcher() {
    let root = TestDir::new("start-command-launcher");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();
    let env = ocm_env(&root);

    let start = run_ocm(
        &cwd,
        &env,
        &[
            "start",
            "hacking",
            "--command",
            "pnpm openclaw",
            "--cwd",
            &project_dir.to_string_lossy(),
            "--no-onboard",
        ],
    );
    assert!(start.status.success(), "{}", stderr(&start));
    let output = stdout(&start);
    assert!(output.contains("Started env hacking"));
    assert!(output.contains("launcher: hacking.local"));

    let launcher = run_ocm(&cwd, &env, &["launcher", "show", "hacking.local", "--json"]);
    assert!(launcher.status.success(), "{}", stderr(&launcher));
    let launcher_json: Value = serde_json::from_str(&stdout(&launcher)).unwrap();
    assert_eq!(launcher_json["command"], "pnpm openclaw");
    assert_eq!(launcher_json["cwd"], project_dir.to_string_lossy().to_string());

    let show = run_ocm(&cwd, &env, &["env", "show", "hacking", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["defaultLauncher"], "hacking.local");
}

#[test]
fn start_reuses_existing_env_without_forcing_onboarding() {
    let root = TestDir::new("start-existing");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let start = run_ocm(&cwd, &env, &["start", "demo", "--no-onboard"]);
    assert!(start.status.success(), "{}", stderr(&start));
    let output = stdout(&start);
    assert!(output.contains("Using env demo"));
    assert!(output.contains("launcher: stable"));
    assert!(output.contains("onboard: ocm @demo -- onboard"));
    assert!(!output.contains("onboarding: running now"));
}

#[test]
fn start_rejects_json_when_onboarding_would_run() {
    let root = TestDir::new("start-json-onboarding");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["start", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("start cannot combine --json with interactive onboarding"));
}

#[test]
fn start_reports_recovery_steps_when_onboarding_fails() {
    let root = TestDir::new("start-onboarding-failure");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let failing_openclaw = root.child("bin/failing-openclaw");
    write_executable_script(&failing_openclaw, "#!/bin/sh\nexit 1\n");

    let start = run_ocm(
        &cwd,
        &env,
        &["start", "demo", "--command", &path_string(&failing_openclaw)],
    );
    assert_eq!(start.status.code(), Some(1));
    assert!(stdout(&start).contains("Started env demo"));
    let error = stderr(&start);
    assert!(error.contains("env demo is ready, but onboarding exited with code 1"));
    assert!(error.contains("retry: ocm @demo -- onboard"));
    assert!(error.contains("run: ocm @demo -- status"));
    assert!(error.contains("keep running: ocm service install demo"));
    assert!(!error.contains("Run \"ocm help\" for usage."));
}
