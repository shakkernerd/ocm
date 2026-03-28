mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::Builder;

use crate::support::{
    TestDir, TestHttpServer, ocm_env, path_string, run_ocm, run_ocm_binary, stderr, stdout,
};

fn current_release_asset_name() -> String {
    let os = match std::env::consts::OS {
        "macos" => "apple-darwin",
        "linux" => "unknown-linux-gnu",
        other => panic!("unsupported test OS for self update: {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        other => panic!("unsupported test arch for self update: {other}"),
    };
    format!("ocm-{arch}-{os}.tar.gz")
}

fn write_release_archive(path: &Path, shell_script: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }

    let file = fs::File::create(path).unwrap();
    let encoder = GzEncoder::new(file, Compression::default());
    let mut builder = Builder::new(encoder);

    let script_bytes = shell_script.as_bytes();
    let mut header = tar::Header::new_gnu();
    header.set_size(script_bytes.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "ocm", script_bytes)
        .unwrap();
    builder.finish().unwrap();
}

fn release_env(root: &TestDir, release_url: &str) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    env.insert(
        "OCM_INTERNAL_SELF_UPDATE_RELEASE_URL".to_string(),
        release_url.to_string(),
    );
    env
}

#[test]
fn self_update_check_reports_when_a_newer_release_exists() {
    let root = TestDir::new("self-update-check");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let asset_name = current_release_asset_name();
    let metadata = format!(
        "{{\"tag_name\":\"v9.9.9\",\"assets\":[{{\"name\":\"{asset_name}\",\"browser_download_url\":\"https://example.test/{asset_name}\"}}]}}"
    );
    let release =
        TestHttpServer::serve_bytes("/release.json", "application/json", metadata.as_bytes());
    let env = release_env(&root, &release.url());

    let output = run_ocm(&cwd, &env, &["self", "update", "--check", "--raw"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("mode: check"));
    assert!(text.contains("status: update_available"));
    assert!(text.contains("currentVersion: 0.2.0"));
    assert!(text.contains("targetVersion: 9.9.9"));
    assert!(text.contains(&format!("assetName: {asset_name}")));
}

#[test]
fn self_update_replaces_a_copied_binary_in_place() {
    let root = TestDir::new("self-update-install");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let copied_binary = root.child("bin/ocm");
    fs::create_dir_all(copied_binary.parent().unwrap()).unwrap();
    fs::copy(env!("CARGO_BIN_EXE_ocm"), &copied_binary).unwrap();

    let asset_name = current_release_asset_name();
    let archive_path = root.child(&asset_name);
    write_release_archive(
        &archive_path,
        "#!/usr/bin/env bash\nif [[ \"$1\" == \"--version\" ]]; then\n  printf '0.2.1\\n'\nelse\n  printf 'updated ocm\\n'\nfi\n",
    );
    let asset = TestHttpServer::serve_bytes(
        "/download.tar.gz",
        "application/gzip",
        &fs::read(&archive_path).unwrap(),
    );
    let metadata = format!(
        "{{\"tag_name\":\"v0.2.1\",\"assets\":[{{\"name\":\"{asset_name}\",\"browser_download_url\":\"{}\"}}]}}",
        asset.url()
    );
    let release =
        TestHttpServer::serve_bytes("/release.json", "application/json", metadata.as_bytes());

    let env = release_env(&root, &release.url());
    let output = run_ocm_binary(
        &copied_binary,
        &cwd,
        &env,
        &["self", "update", "--version", "0.2.1", "--raw"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("mode: update"));
    assert!(text.contains("status: updated"));
    assert!(text.contains(&format!("binaryPath: {}", path_string(&copied_binary))));

    let updated = Command::new(&copied_binary)
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    assert!(updated.status.success());
    assert_eq!(String::from_utf8(updated.stdout).unwrap(), "0.2.1\n");
}
