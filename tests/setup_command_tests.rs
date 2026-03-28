mod support;

use std::fs;

use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use serde_json::Value;
use sha2::{Digest, Sha512};
use tar::{Builder, Header};

use crate::support::{
    TestDir, TestHttpServer, ocm_env, run_ocm, run_ocm_with_stdin, stderr, stdout,
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
fn setup_can_prepare_latest_stable_without_onboarding() {
    let root = TestDir::new("setup-stable");
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

    let setup = run_ocm_with_stdin(&cwd, &env, &["setup"], "1\n\nn\nn\n");
    assert!(setup.status.success(), "{}", stderr(&setup));
    let output = stdout(&setup);
    assert!(output.contains("OpenClaw setup"));
    assert!(output.contains("Started env default"));

    let show = run_ocm(&cwd, &env, &["env", "show", "default", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["defaultRuntime"], "stable");
}

#[test]
fn setup_can_prepare_a_local_command_launcher() {
    let root = TestDir::new("setup-local-command");
    let cwd = root.child("workspace");
    let project_dir = cwd.join("openclaw");
    fs::create_dir_all(&project_dir).unwrap();
    let env = ocm_env(&root);

    let input = format!(
        "4\nhacking\npnpm openclaw\n{}\nn\nn\n",
        project_dir.display()
    );
    let setup = run_ocm_with_stdin(&cwd, &env, &["setup"], &input);
    assert!(setup.status.success(), "{}", stderr(&setup));
    let output = stdout(&setup);
    assert!(output.contains("Started env hacking"));
    assert!(output.contains("launcher: hacking.local"));

    let show = run_ocm(&cwd, &env, &["env", "show", "hacking", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["defaultLauncher"], "hacking.local");
}

#[test]
fn setup_detects_a_local_openclaw_checkout_and_uses_default_local_values() {
    let root = TestDir::new("setup-detect-local-checkout");
    let repo = root.child("workspace/openclaw");
    let scripts_dir = repo.join("scripts");
    fs::create_dir_all(&scripts_dir).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"name":"openclaw","version":"2026.3.28"}"#,
    )
    .unwrap();
    fs::write(scripts_dir.join("run-node.mjs"), "console.log('run');\n").unwrap();
    let env = ocm_env(&root);

    let setup = run_ocm_with_stdin(&repo, &env, &["setup"], "4\n\n\n\nn\nn\n");
    assert!(setup.status.success(), "{}", stderr(&setup));
    let output = stdout(&setup);
    assert!(output.contains("Detected local OpenClaw checkout:"));
    assert!(output.contains("Started env dev"));
    assert!(output.contains("launcher: dev.local"));

    let show = run_ocm(&repo, &env, &["launcher", "show", "dev.local", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["command"], "pnpm openclaw");
    assert_eq!(
        show_json["cwd"],
        fs::canonicalize(&repo).unwrap().display().to_string()
    );
}
