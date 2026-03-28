mod support;

use std::fs;

use base64::Engine;
use flate2::{Compression, write::GzEncoder};
use ocm::infra::download::file_sha256;
use ocm::store::runtime_install_root;
use serde_json::Value;
use sha2::{Digest, Sha512};
use tar::{Builder, Header};

use crate::support::{
    TestDir, TestHttpServer, ocm_env, path_string, run_ocm, stderr, stdout, write_executable_script,
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
fn runtime_list_uses_runtime_wording_when_empty() {
    let root = TestDir::new("runtime-list-empty");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let list = run_ocm(&cwd, &env, &["runtime", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert_eq!(stdout(&list), "No runtimes.\n");
}

#[test]
fn runtime_add_and_list_use_runtime_storage() {
    let root = TestDir::new("runtime-add-list");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("stable"), "#!/bin/sh\n").unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "add",
            "stable",
            "--path",
            "./bin/stable",
            "--description",
            "stable runtime",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    assert!(stdout(&add).contains("Added runtime stable"));

    let list = run_ocm(&cwd, &env, &["runtime", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("stable"));
    assert!(stdout(&list).contains("/bin/stable"));

    let show_json = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(show_json.status.success(), "{}", stderr(&show_json));
    let output = stdout(&show_json);
    assert!(output.contains("\"name\": \"stable\""));
    assert!(output.contains("\"description\": \"stable runtime\""));
}

#[test]
fn runtime_show_and_remove_use_runtime_metadata() {
    let root = TestDir::new("runtime-show-remove");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::write(bin_dir.join("stable"), "#!/bin/sh\n").unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("kind: ocm-runtime"));
    assert!(show_stdout.contains("name: stable"));
    assert!(show_stdout.contains("binaryPath:"));
    assert!(show_stdout.contains("sourceKind: registered"));

    let remove = run_ocm(&cwd, &env, &["runtime", "remove", "stable"]);
    assert!(remove.status.success(), "{}", stderr(&remove));
    assert!(stdout(&remove).contains("Removed runtime stable"));

    let runtime_list = run_ocm(&cwd, &env, &["runtime", "list"]);
    assert!(runtime_list.status.success(), "{}", stderr(&runtime_list));
    assert_eq!(stdout(&runtime_list), "No runtimes.\n");
}

#[test]
fn runtime_install_and_which_use_the_managed_binary_path() {
    let root = TestDir::new("runtime-install-which");
    let cwd = root.child("workspace");
    let source_dir = cwd.join("downloads");
    let source_path = source_dir.join("openclaw");
    fs::create_dir_all(&source_dir).unwrap();
    write_executable_script(&source_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--path",
            "./downloads/openclaw",
            "--description",
            "managed runtime",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));
    assert!(stdout(&install).contains("Installed runtime stable"));

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw");
    let expected_source_path = fs::canonicalize(&source_path).unwrap();
    let which = run_ocm(&cwd, &env, &["runtime", "which", "stable"]);
    assert!(which.status.success(), "{}", stderr(&which));
    assert_eq!(
        stdout(&which),
        format!("{}\n", path_string(&expected_binary))
    );

    let which_json = run_ocm(&cwd, &env, &["runtime", "which", "stable", "--json"]);
    assert!(which_json.status.success(), "{}", stderr(&which_json));
    let which_json_stdout = stdout(&which_json);
    assert!(which_json_stdout.contains("\"name\": \"stable\""));
    assert!(which_json_stdout.contains(&format!(
        "\"binaryPath\": \"{}\"",
        path_string(&expected_binary)
    )));
    assert!(which_json_stdout.contains("\"sourceKind\": \"installed\""));

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("sourceKind: installed"));
    assert!(show_stdout.contains(&format!(
        "sourcePath: {}",
        path_string(&expected_source_path)
    )));
    assert!(show_stdout.contains(&format!("installRoot: {}", path_string(&install_root))));
}

#[test]
fn runtime_install_from_url_downloads_into_the_managed_store() {
    let root = TestDir::new("runtime-install-url");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let server = TestHttpServer::serve_bytes(
        "/releases/openclaw-nightly",
        "application/octet-stream",
        b"downloaded-runtime",
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "nightly",
            "--url",
            &server.url(),
            "--description",
            "downloaded runtime",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));
    assert!(stdout(&install).contains("Installed runtime nightly"));

    let install_root = runtime_install_root("nightly", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw-nightly");
    assert_eq!(fs::read(&expected_binary).unwrap(), b"downloaded-runtime");

    let show = run_ocm(&cwd, &env, &["runtime", "show", "nightly"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("sourceKind: installed"));
    assert!(show_stdout.contains(&format!("sourceUrl: {}", server.url())));
    assert!(show_stdout.contains("description: downloaded runtime"));
}

#[test]
fn runtime_install_from_manifest_version_downloads_and_records_release_metadata() {
    let root = TestDir::new("runtime-install-manifest-version");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let artifact_body = b"release-binary";
    let digest_path = root.child("sha256/openclaw-0.2.0");
    fs::create_dir_all(digest_path.parent().unwrap()).unwrap();
    fs::write(&digest_path, artifact_body).unwrap();
    let sha256 = file_sha256(&digest_path).unwrap();

    let artifact_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.2.0",
        "application/octet-stream",
        artifact_body,
    );
    let manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\",\"description\":\"stable manifest runtime\"}}]}}",
        artifact_server.url(),
        sha256
    );
    let manifest_server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        manifest_body.as_bytes(),
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            &manifest_server.url(),
            "--version",
            "0.2.0",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));
    assert!(stdout(&install).contains("Installed runtime stable"));

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw-0.2.0");
    assert_eq!(fs::read(&expected_binary).unwrap(), artifact_body);

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("sourceKind: installed"));
    assert!(show_stdout.contains(&format!("sourceManifestUrl: {}", manifest_server.url())));
    assert!(show_stdout.contains(&format!("sourceSha256: {sha256}")));
    assert!(show_stdout.contains("releaseVersion: 0.2.0"));
    assert!(show_stdout.contains("releaseChannel: stable"));
    assert!(show_stdout.contains("releaseSelectorKind: version"));
    assert!(show_stdout.contains("releaseSelectorValue: 0.2.0"));
    assert!(show_stdout.contains("description: stable manifest runtime"));
}

#[test]
fn runtime_install_from_official_release_downloads_and_extracts_the_openclaw_package() {
    let root = TestDir::new("runtime-install-official-release");
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
        TestHttpServer::serve_bytes("/openclaw", "application/json", packument.as_bytes());
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let install = run_ocm(&cwd, &env, &["runtime", "install", "--channel", "stable"]);
    assert!(install.status.success(), "{}", stderr(&install));
    assert!(stdout(&install).contains("Installed runtime stable"));

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/package/openclaw.mjs");
    assert_eq!(
        fs::read_to_string(&expected_binary).unwrap(),
        "#!/usr/bin/env node\nconsole.log('stable');\n"
    );

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("releaseVersion: 2026.3.24"));
    assert!(show_stdout.contains("releaseChannel: stable"));
    assert!(show_stdout.contains("sourceManifestUrl:"));
    assert!(show_stdout.contains("sourceKind: installed"));

    let which = run_ocm(&cwd, &env, &["runtime", "which", "stable"]);
    assert!(which.status.success(), "{}", stderr(&which));
    assert_eq!(
        stdout(&which),
        format!("{}\n", path_string(&expected_binary))
    );
}

#[test]
fn runtime_install_rejects_non_canonical_names_for_official_releases() {
    let root = TestDir::new("runtime-install-official-canonical-name");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let packument = br#"{"dist-tags":{"latest":"2026.3.24"},"versions":{"2026.3.24":{"version":"2026.3.24","dist":{"tarball":"https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz"}}}}"#;
    let server = TestHttpServer::serve_bytes("/openclaw", "application/json", packument);
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        server.url(),
    );

    let install = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "latest", "--channel", "stable"],
    );
    assert_eq!(install.status.code(), Some(1));
    assert!(
        stderr(&install).contains(
            "official runtime installs use the canonical name \"stable\" for this selector"
        )
    );
}

#[test]
fn runtime_install_reuses_a_matching_official_runtime() {
    let root = TestDir::new("runtime-install-official-reuse");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let tarball = openclaw_package_tarball("#!/usr/bin/env node\nconsole.log('stable');\n");
    let integrity = sha512_integrity(&tarball);
    let tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &tarball,
        2,
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

    let first = run_ocm(&cwd, &env, &["runtime", "install", "--channel", "stable"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stdout(&first).contains("Installed runtime stable"));

    let second = run_ocm(&cwd, &env, &["runtime", "install", "--channel", "stable"]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert!(stdout(&second).contains("Using installed runtime stable"));
}

#[test]
fn runtime_install_refreshes_a_channel_runtime_when_the_published_release_moves() {
    let root = TestDir::new("runtime-install-official-refresh");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let stable_tar = openclaw_package_tarball("#!/usr/bin/env node\nconsole.log('stable-1');\n");
    let stable_integrity = sha512_integrity(&stable_tar);
    let next_tar = openclaw_package_tarball("#!/usr/bin/env node\nconsole.log('stable-2');\n");
    let next_integrity = sha512_integrity(&next_tar);
    let first_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &stable_tar,
        2,
    );
    let next_tarball_server = TestHttpServer::serve_bytes_times(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &next_tar,
        2,
    );
    let first_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\"}}}}",
        first_tarball_server.url(),
        stable_integrity
    );
    let second_packument = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.24\":\"2026-03-25T16:35:52.000Z\",\"2026.3.25\":\"2026-03-26T10:00:00.000Z\"}}}}",
        first_tarball_server.url(),
        stable_integrity,
        next_tarball_server.url(),
        next_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![first_packument.into_bytes(), second_packument.into_bytes()],
    );
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let first = run_ocm(&cwd, &env, &["runtime", "install", "--channel", "stable"]);
    assert!(first.status.success(), "{}", stderr(&first));
    assert!(stdout(&first).contains("Installed runtime stable"));

    let second = run_ocm(&cwd, &env, &["runtime", "install", "--channel", "stable"]);
    assert!(second.status.success(), "{}", stderr(&second));
    assert!(stdout(&second).contains("Updated runtime stable"));

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("\"releaseVersion\": \"2026.3.25\""));
    assert!(show_stdout.contains("\"releaseSelectorValue\": \"stable\""));
}

#[test]
fn runtime_releases_without_manifest_url_use_the_official_openclaw_source() {
    let root = TestDir::new("runtime-releases-official");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let packument = br#"{"dist-tags":{"latest":"2026.3.24","beta":"2026.3.24-beta.2"},"versions":{"2026.3.24":{"version":"2026.3.24","dist":{"tarball":"https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz"}},"2026.3.24-beta.2":{"version":"2026.3.24-beta.2","dist":{"tarball":"https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24-beta.2.tgz"}}}}"#;
    let server = TestHttpServer::serve_bytes("/openclaw", "application/json", packument);
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        server.url(),
    );

    let output = run_ocm(&cwd, &env, &["runtime", "releases", "--channel", "stable"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output = stdout(&output);
    assert!(output.contains("2026.3.24"));
    assert!(output.contains("channel=stable"));
}

#[test]
fn runtime_update_reuses_the_official_release_selector() {
    let root = TestDir::new("runtime-update-official-release");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let stable_tar = openclaw_package_tarball("#!/usr/bin/env node\nconsole.log('stable');\n");
    let updated_tar = openclaw_package_tarball("#!/usr/bin/env node\nconsole.log('updated');\n");
    let stable_integrity = sha512_integrity(&stable_tar);
    let updated_integrity = sha512_integrity(&updated_tar);
    let stable_tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.24.tgz",
        "application/octet-stream",
        &stable_tar,
    );
    let updated_tarball_server = TestHttpServer::serve_bytes(
        "/openclaw-2026.3.25.tgz",
        "application/octet-stream",
        &updated_tar,
    );
    let packument_v1 = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.24\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}}}}",
        stable_tarball_server.url(),
        stable_integrity
    );
    let packument_v2 = format!(
        "{{\"dist-tags\":{{\"latest\":\"2026.3.25\"}},\"versions\":{{\"2026.3.24\":{{\"version\":\"2026.3.24\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}},\"2026.3.25\":{{\"version\":\"2026.3.25\",\"dist\":{{\"tarball\":\"{}\",\"integrity\":\"{}\"}}}}}},\"time\":{{\"2026.3.25\":\"2026-03-26T10:00:00.000Z\"}}}}",
        stable_tarball_server.url(),
        stable_integrity,
        updated_tarball_server.url(),
        updated_integrity
    );
    let packument_server = TestHttpServer::serve_bytes_sequence(
        "/openclaw",
        "application/json",
        vec![packument_v1.into_bytes(), packument_v2.into_bytes()],
    );
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        packument_server.url(),
    );

    let install = run_ocm(
        &cwd,
        &env,
        &["runtime", "install", "stable", "--channel", "stable"],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let update = run_ocm(&cwd, &env, &["runtime", "update", "stable"]);
    assert!(update.status.success(), "{}", stderr(&update));
    assert!(stdout(&update).contains("Updated runtime stable"));

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/package/openclaw.mjs");
    assert_eq!(
        fs::read_to_string(&expected_binary).unwrap(),
        "#!/usr/bin/env node\nconsole.log('updated');\n"
    );

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    assert!(stdout(&show).contains("releaseVersion: 2026.3.25"));
}

#[test]
fn runtime_install_from_manifest_channel_selects_the_matching_release() {
    let root = TestDir::new("runtime-install-manifest-channel");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let stable_body = b"stable-binary";
    let stable_digest_path = root.child("sha256/openclaw-stable");
    fs::create_dir_all(stable_digest_path.parent().unwrap()).unwrap();
    fs::write(&stable_digest_path, stable_body).unwrap();
    let stable_sha256 = file_sha256(&stable_digest_path).unwrap();

    let nightly_body = b"nightly-binary";
    let nightly_digest_path = root.child("sha256/openclaw-nightly");
    fs::write(&nightly_digest_path, nightly_body).unwrap();
    let nightly_sha256 = file_sha256(&nightly_digest_path).unwrap();

    let stable_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-stable",
        "application/octet-stream",
        stable_body,
    );
    let nightly_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-nightly",
        "application/octet-stream",
        nightly_body,
    );
    let manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}},{{\"version\":\"0.3.0-dev\",\"channel\":\"nightly\",\"url\":\"{}\",\"sha256\":\"{}\",\"description\":\"nightly manifest runtime\"}}]}}",
        stable_server.url(),
        stable_sha256,
        nightly_server.url(),
        nightly_sha256
    );
    let manifest_server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        manifest_body.as_bytes(),
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "nightly",
            "--manifest-url",
            &manifest_server.url(),
            "--channel",
            "nightly",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));
    assert!(stdout(&install).contains("Installed runtime nightly"));

    let install_root = runtime_install_root("nightly", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw-nightly");
    assert_eq!(fs::read(&expected_binary).unwrap(), nightly_body);

    let show = run_ocm(&cwd, &env, &["runtime", "show", "nightly"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("releaseVersion: 0.3.0-dev"));
    assert!(show_stdout.contains("releaseChannel: nightly"));
    assert!(show_stdout.contains("releaseSelectorKind: channel"));
    assert!(show_stdout.contains("releaseSelectorValue: nightly"));
    assert!(show_stdout.contains(&format!("sourceSha256: {nightly_sha256}")));
    assert!(show_stdout.contains("description: nightly manifest runtime"));
}

#[test]
fn runtime_list_surfaces_release_metadata_for_manifest_installs() {
    let root = TestDir::new("runtime-list-release-metadata");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let artifact_body = b"release-runtime";
    let digest_path = root.child("sha256/openclaw-stable");
    fs::create_dir_all(digest_path.parent().unwrap()).unwrap();
    fs::write(&digest_path, artifact_body).unwrap();
    let sha256 = file_sha256(&digest_path).unwrap();

    let artifact_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-stable",
        "application/octet-stream",
        artifact_body,
    );
    let manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        artifact_server.url(),
        sha256
    );
    let manifest_server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        manifest_body.as_bytes(),
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            &manifest_server.url(),
            "--channel",
            "stable",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let list = run_ocm(&cwd, &env, &["runtime", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("stable"));
    assert!(output.contains("source=installed"));
    assert!(output.contains("release=0.2.0"));
    assert!(output.contains("channel=stable"));
}

#[test]
fn runtime_update_reinstalls_a_manifest_backed_runtime_with_a_new_version() {
    let root = TestDir::new("runtime-update-version");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let stable_body = b"runtime-v0.2.0";
    let stable_digest_path = root.child("sha256/openclaw-0.2.0");
    fs::create_dir_all(stable_digest_path.parent().unwrap()).unwrap();
    fs::write(&stable_digest_path, stable_body).unwrap();
    let stable_sha256 = file_sha256(&stable_digest_path).unwrap();

    let next_body = b"runtime-v0.3.0";
    let next_digest_path = root.child("sha256/openclaw-0.3.0");
    fs::write(&next_digest_path, next_body).unwrap();
    let next_sha256 = file_sha256(&next_digest_path).unwrap();

    let stable_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.2.0",
        "application/octet-stream",
        stable_body,
    );
    let next_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.3.0",
        "application/octet-stream",
        next_body,
    );
    let manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}},{{\"version\":\"0.3.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        stable_server.url(),
        stable_sha256,
        next_server.url(),
        next_sha256
    );
    let manifest_server = TestHttpServer::serve_bytes_times(
        "/manifests/releases.json",
        "application/json",
        manifest_body.as_bytes(),
        2,
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            &manifest_server.url(),
            "--version",
            "0.2.0",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let update = run_ocm(
        &cwd,
        &env,
        &["runtime", "update", "stable", "--version", "0.3.0"],
    );
    assert!(update.status.success(), "{}", stderr(&update));
    assert!(stdout(&update).contains("Updated runtime stable"));

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw-0.3.0");
    assert_eq!(fs::read(&expected_binary).unwrap(), next_body);

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let output = stdout(&show);
    assert!(output.contains("releaseVersion: 0.3.0"));
    assert!(output.contains("releaseChannel: stable"));
    assert!(output.contains(&format!("sourceSha256: {next_sha256}")));
}

#[test]
fn runtime_update_without_arguments_reuses_the_stored_version_selector() {
    let root = TestDir::new("runtime-update-stored-version-selector");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let stable_body = b"runtime-v0.2.0";
    let stable_digest_path = root.child("sha256/openclaw-0.2.0");
    fs::create_dir_all(stable_digest_path.parent().unwrap()).unwrap();
    fs::write(&stable_digest_path, stable_body).unwrap();
    let stable_sha256 = file_sha256(&stable_digest_path).unwrap();

    let next_body = b"runtime-v0.3.0";
    let next_digest_path = root.child("sha256/openclaw-0.3.0");
    fs::write(&next_digest_path, next_body).unwrap();
    let next_sha256 = file_sha256(&next_digest_path).unwrap();

    let stable_server = TestHttpServer::serve_bytes_times(
        "/artifacts/openclaw-0.2.0",
        "application/octet-stream",
        stable_body,
        2,
    );
    let next_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.3.0",
        "application/octet-stream",
        next_body,
    );
    let first_manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        stable_server.url(),
        stable_sha256
    );
    let second_manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}},{{\"version\":\"0.3.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        stable_server.url(),
        stable_sha256,
        next_server.url(),
        next_sha256
    );
    let manifest_server = TestHttpServer::serve_bytes_sequence(
        "/manifests/releases.json",
        "application/json",
        vec![
            first_manifest_body.into_bytes(),
            second_manifest_body.into_bytes(),
        ],
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            &manifest_server.url(),
            "--version",
            "0.2.0",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let update = run_ocm(&cwd, &env, &["runtime", "update", "stable"]);
    assert!(update.status.success(), "{}", stderr(&update));
    assert!(stdout(&update).contains("Updated runtime stable"));

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let output = stdout(&show);
    assert!(output.contains("releaseVersion: 0.2.0"));
    assert!(output.contains("releaseSelectorKind: version"));
    assert!(output.contains("releaseSelectorValue: 0.2.0"));
}

#[test]
fn runtime_update_without_arguments_reuses_the_stored_channel_selector() {
    let root = TestDir::new("runtime-update-stored-channel-selector");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let first_body = b"runtime-v0.2.0";
    let first_digest_path = root.child("sha256/openclaw-0.2.0");
    fs::create_dir_all(first_digest_path.parent().unwrap()).unwrap();
    fs::write(&first_digest_path, first_body).unwrap();
    let first_sha256 = file_sha256(&first_digest_path).unwrap();

    let second_body = b"runtime-v0.3.0";
    let second_digest_path = root.child("sha256/openclaw-0.3.0");
    fs::write(&second_digest_path, second_body).unwrap();
    let second_sha256 = file_sha256(&second_digest_path).unwrap();

    let first_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.2.0",
        "application/octet-stream",
        first_body,
    );
    let second_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.3.0",
        "application/octet-stream",
        second_body,
    );
    let first_manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        first_server.url(),
        first_sha256
    );
    let second_manifest_body = format!(
        "{{\"releases\":[{{\"version\":\"0.3.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
        second_server.url(),
        second_sha256
    );
    let manifest_server = TestHttpServer::serve_bytes_sequence(
        "/manifests/releases.json",
        "application/json",
        vec![
            first_manifest_body.into_bytes(),
            second_manifest_body.into_bytes(),
        ],
    );
    let env = ocm_env(&root);

    let install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--manifest-url",
            &manifest_server.url(),
            "--channel",
            "stable",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let update = run_ocm(&cwd, &env, &["runtime", "update", "stable"]);
    assert!(update.status.success(), "{}", stderr(&update));
    assert!(stdout(&update).contains("Updated runtime stable"));

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let output = stdout(&show);
    assert!(output.contains("releaseVersion: 0.3.0"));
    assert!(output.contains("releaseChannel: stable"));
    assert!(output.contains("releaseSelectorKind: channel"));
    assert!(output.contains("releaseSelectorValue: stable"));
}

#[test]
fn runtime_update_all_uses_stored_selectors_and_skips_registered_runtimes() {
    let root = TestDir::new("runtime-update-all");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let external_path = bin_dir.join("external");
    write_executable_script(&external_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "external", "--path", "./bin/external"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let pinned_body = b"runtime-v0.2.0";
    let pinned_digest_path = root.child("sha256/openclaw-0.2.0");
    fs::create_dir_all(pinned_digest_path.parent().unwrap()).unwrap();
    fs::write(&pinned_digest_path, pinned_body).unwrap();
    let pinned_sha256 = file_sha256(&pinned_digest_path).unwrap();

    let pinned_next_body = b"runtime-v0.3.0";
    let pinned_next_digest_path = root.child("sha256/openclaw-0.3.0");
    fs::write(&pinned_next_digest_path, pinned_next_body).unwrap();
    let pinned_next_sha256 = file_sha256(&pinned_next_digest_path).unwrap();

    let pinned_server = TestHttpServer::serve_bytes_times(
        "/artifacts/openclaw-0.2.0",
        "application/octet-stream",
        pinned_body,
        3,
    );
    let pinned_next_server = TestHttpServer::serve_bytes_times(
        "/artifacts/openclaw-0.3.0",
        "application/octet-stream",
        pinned_next_body,
        2,
    );
    let pinned_manifest_server = TestHttpServer::serve_bytes_sequence(
        "/manifests/pinned.json",
        "application/json",
        vec![
            format!(
                "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
                pinned_server.url(),
                pinned_sha256
            )
            .into_bytes(),
            format!(
                "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}},{{\"version\":\"0.3.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
                pinned_server.url(),
                pinned_sha256,
                pinned_next_server.url(),
                pinned_next_sha256
            )
            .into_bytes(),
            format!(
                "{{\"releases\":[{{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}},{{\"version\":\"0.3.0\",\"channel\":\"stable\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
                pinned_server.url(),
                pinned_sha256,
                pinned_next_server.url(),
                pinned_next_sha256
            )
            .into_bytes(),
        ],
    );

    let tracked_first_body = b"runtime-nightly-v0.2.0";
    let tracked_first_digest_path = root.child("sha256/openclaw-nightly-0.2.0");
    fs::write(&tracked_first_digest_path, tracked_first_body).unwrap();
    let tracked_first_sha256 = file_sha256(&tracked_first_digest_path).unwrap();

    let tracked_second_body = b"runtime-nightly-v0.3.0";
    let tracked_second_digest_path = root.child("sha256/openclaw-nightly-0.3.0");
    fs::write(&tracked_second_digest_path, tracked_second_body).unwrap();
    let tracked_second_sha256 = file_sha256(&tracked_second_digest_path).unwrap();

    let tracked_first_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-nightly-0.2.0",
        "application/octet-stream",
        tracked_first_body,
    );
    let tracked_second_server = TestHttpServer::serve_bytes_times(
        "/artifacts/openclaw-nightly-0.3.0",
        "application/octet-stream",
        tracked_second_body,
        2,
    );
    let tracked_manifest_server = TestHttpServer::serve_bytes_sequence(
        "/manifests/nightly.json",
        "application/json",
        vec![
            format!(
                "{{\"releases\":[{{\"version\":\"0.2.0-dev\",\"channel\":\"nightly\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
                tracked_first_server.url(),
                tracked_first_sha256
            )
            .into_bytes(),
            format!(
                "{{\"releases\":[{{\"version\":\"0.3.0-dev\",\"channel\":\"nightly\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
                tracked_second_server.url(),
                tracked_second_sha256
            )
            .into_bytes(),
            format!(
                "{{\"releases\":[{{\"version\":\"0.3.0-dev\",\"channel\":\"nightly\",\"url\":\"{}\",\"sha256\":\"{}\"}}]}}",
                tracked_second_server.url(),
                tracked_second_sha256
            )
            .into_bytes(),
        ],
    );

    let install_pinned = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable-pinned",
            "--manifest-url",
            &pinned_manifest_server.url(),
            "--version",
            "0.2.0",
        ],
    );
    assert!(
        install_pinned.status.success(),
        "{}",
        stderr(&install_pinned)
    );

    let install_tracked = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "nightly",
            "--manifest-url",
            &tracked_manifest_server.url(),
            "--channel",
            "nightly",
        ],
    );
    assert!(
        install_tracked.status.success(),
        "{}",
        stderr(&install_tracked)
    );

    let update = run_ocm(&cwd, &env, &["runtime", "update", "--all", "--json"]);
    assert!(update.status.success(), "{}", stderr(&update));
    let value: Value = serde_json::from_str(&stdout(&update)).unwrap();
    assert_eq!(value["count"], 3);
    assert_eq!(value["updated"], 2);
    assert_eq!(value["skipped"], 1);
    assert_eq!(value["failed"], 0);
    let array = value["results"].as_array().unwrap();
    assert_eq!(array.len(), 3);
    assert!(array.iter().any(|item| {
        item["name"] == "external"
            && item["outcome"] == "skipped"
            && item["issue"]
                .as_str()
                .unwrap()
                .contains("not backed by a release manifest")
    }));
    assert!(array.iter().any(|item| {
        item["name"] == "stable-pinned"
            && item["outcome"] == "updated"
            && item["releaseVersion"] == "0.2.0"
    }));
    assert!(array.iter().any(|item| {
        item["name"] == "nightly"
            && item["outcome"] == "updated"
            && item["releaseVersion"] == "0.3.0-dev"
            && item["releaseChannel"] == "nightly"
    }));

    let update_plain = run_ocm(&cwd, &env, &["runtime", "update", "--all"]);
    assert!(update_plain.status.success(), "{}", stderr(&update_plain));
    let plain_output = stdout(&update_plain);
    assert!(plain_output.contains("Runtime update summary: total=3 updated=2 skipped=1 failed=0"));
    assert!(plain_output.contains("external  outcome=skipped"));
    assert!(plain_output.contains("stable-pinned  outcome=updated"));
    assert!(plain_output.contains("nightly  outcome=updated"));
}

#[test]
fn runtime_install_from_url_cleans_up_failed_install_roots_for_retry() {
    let root = TestDir::new("runtime-install-url-retry");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let missing_server = TestHttpServer::serve_bytes(
        "/releases/openclaw-nightly",
        "application/octet-stream",
        b"downloaded-runtime",
    );
    let env = ocm_env(&root);

    let failed_install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "nightly",
            "--url",
            &format!("{}-missing", missing_server.url()),
        ],
    );
    assert_eq!(failed_install.status.code(), Some(1));
    assert!(stderr(&failed_install).contains("failed to download runtime URL"));

    let install_root = runtime_install_root("nightly", &env, &cwd).unwrap();
    assert!(!install_root.exists());

    let retry_server = TestHttpServer::serve_bytes(
        "/releases/openclaw-nightly",
        "application/octet-stream",
        b"downloaded-runtime",
    );
    let retry_install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "nightly",
            "--url",
            &retry_server.url(),
        ],
    );
    assert!(retry_install.status.success(), "{}", stderr(&retry_install));
    assert!(stdout(&retry_install).contains("Installed runtime nightly"));
}

#[test]
fn runtime_install_force_replaces_an_existing_runtime_definition() {
    let root = TestDir::new("runtime-install-force");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    let source_dir = cwd.join("downloads");
    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&source_dir).unwrap();
    let external_path = bin_dir.join("stable");
    let managed_source_path = source_dir.join("openclaw");
    write_executable_script(&external_path, "#!/bin/sh\nexit 0\n");
    write_executable_script(&managed_source_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let duplicate_install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--path",
            "./downloads/openclaw",
        ],
    );
    assert_eq!(duplicate_install.status.code(), Some(1));
    assert!(stderr(&duplicate_install).contains("runtime \"stable\" already exists"));

    let force_install = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "install",
            "stable",
            "--path",
            "./downloads/openclaw",
            "--force",
        ],
    );
    assert!(force_install.status.success(), "{}", stderr(&force_install));
    assert!(stdout(&force_install).contains("Installed runtime stable"));

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw");
    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("\"sourceKind\": \"installed\""));
    assert!(show_stdout.contains(&format!(
        "\"binaryPath\": \"{}\"",
        path_string(&expected_binary)
    )));
}

#[test]
fn runtime_releases_lists_manifest_entries() {
    let root = TestDir::new("runtime-releases");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let manifest_body = b"{\"releases\":[{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"https://example.test/openclaw-stable\",\"sha256\":\"abc123\"},{\"version\":\"0.3.0-dev\",\"channel\":\"nightly\",\"url\":\"https://example.test/openclaw-nightly\"}]}";
    let server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        manifest_body,
    );
    let env = ocm_env(&root);

    let list = run_ocm(
        &cwd,
        &env,
        &["runtime", "releases", "--manifest-url", &server.url()],
    );
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("0.2.0"));
    assert!(output.contains("channel=stable"));
    assert!(output.contains("sha256=abc123"));
    assert!(output.contains("0.3.0-dev"));

    let json_server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        manifest_body,
    );
    let json = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "releases",
            "--manifest-url",
            &json_server.url(),
            "--json",
        ],
    );
    assert!(json.status.success(), "{}", stderr(&json));
    let json_output = stdout(&json);
    assert!(json_output.contains("\"version\": \"0.2.0\""));
    assert!(json_output.contains("\"channel\": \"stable\""));
}

#[test]
fn runtime_releases_can_filter_by_version_or_channel() {
    let root = TestDir::new("runtime-releases-filtered");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let manifest_body = b"{\"releases\":[{\"version\":\"0.2.0\",\"channel\":\"stable\",\"url\":\"https://example.test/openclaw-stable\",\"sha256\":\"abc123\"},{\"version\":\"0.3.0-dev\",\"channel\":\"nightly\",\"url\":\"https://example.test/openclaw-nightly\"}]}";
    let env = ocm_env(&root);

    let version_server = TestHttpServer::serve_bytes(
        "/manifests/releases-version.json",
        "application/json",
        manifest_body,
    );
    let by_version = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "releases",
            "--manifest-url",
            &version_server.url(),
            "--version",
            "0.2.0",
        ],
    );
    assert!(by_version.status.success(), "{}", stderr(&by_version));
    let version_output = stdout(&by_version);
    assert!(version_output.contains("0.2.0"));
    assert!(version_output.contains("channel=stable"));
    assert!(!version_output.contains("0.3.0-dev"));

    let channel_server = TestHttpServer::serve_bytes(
        "/manifests/releases-channel.json",
        "application/json",
        manifest_body,
    );
    let by_channel = run_ocm(
        &cwd,
        &env,
        &[
            "runtime",
            "releases",
            "--manifest-url",
            &channel_server.url(),
            "--channel",
            "nightly",
            "--json",
        ],
    );
    assert!(by_channel.status.success(), "{}", stderr(&by_channel));
    let value: Value = serde_json::from_str(&stdout(&by_channel)).unwrap();
    let array = value.as_array().unwrap();
    assert_eq!(array.len(), 1);
    assert_eq!(array[0]["version"], "0.3.0-dev");
    assert_eq!(array[0]["channel"], "nightly");
}
