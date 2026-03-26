mod support;

use std::fs;

use ocm::download::file_sha256;
use ocm::paths::runtime_install_root;
use serde_json::Value;

use crate::support::{
    TestDir, TestHttpServer, ocm_env, path_string, run_ocm, stderr, stdout, write_executable_script,
};

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
        2,
    );
    let pinned_next_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-0.3.0",
        "application/octet-stream",
        pinned_next_body,
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
    let tracked_second_server = TestHttpServer::serve_bytes(
        "/artifacts/openclaw-nightly-0.3.0",
        "application/octet-stream",
        tracked_second_body,
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
    let array = value.as_array().unwrap();
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
