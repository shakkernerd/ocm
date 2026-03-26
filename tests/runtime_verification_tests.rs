mod support;

use std::fs;

use ocm::infra::download::file_sha256;
use ocm::paths::runtime_install_root;

use crate::support::{TestDir, TestHttpServer, ocm_env, run_ocm, stderr, write_executable_script};

#[test]
fn runtime_show_and_which_fail_when_the_installed_binary_is_missing() {
    let root = TestDir::new("runtime-missing-installed-binary");
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
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let installed_binary = runtime_install_root("stable", &env, &cwd)
        .unwrap()
        .join("files/openclaw");
    fs::remove_file(&installed_binary).unwrap();

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert_eq!(show.status.code(), Some(1));
    assert!(stderr(&show).contains("runtime \"stable\" binary path does not exist:"));

    let which = run_ocm(&cwd, &env, &["runtime", "which", "stable"]);
    assert_eq!(which.status.code(), Some(1));
    assert!(stderr(&which).contains("runtime \"stable\" binary path does not exist:"));
}

#[test]
fn env_resolve_and_run_fail_when_the_registered_runtime_binary_is_missing() {
    let root = TestDir::new("runtime-missing-run-binary");
    let cwd = root.child("workspace");
    let bin_dir = cwd.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let runtime_path = bin_dir.join("stable");
    write_executable_script(&runtime_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/stable"],
    );
    assert!(add.status.success(), "{}", stderr(&add));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(&runtime_path).unwrap();

    let resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo"]);
    assert_eq!(resolve.status.code(), Some(1));
    assert!(stderr(&resolve).contains("runtime \"stable\" binary path does not exist:"));

    let run = run_ocm(&cwd, &env, &["env", "run", "demo", "--"]);
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("runtime \"stable\" binary path does not exist:"));
}

#[test]
fn runtime_show_and_which_fail_when_the_installed_binary_checksum_drifted() {
    let root = TestDir::new("runtime-checksum-drift-show");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let artifact_body = b"expected-runtime";
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
            "--version",
            "0.2.0",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let installed_binary = runtime_install_root("stable", &env, &cwd)
        .unwrap()
        .join("files/openclaw-stable");
    fs::write(&installed_binary, b"tampered-runtime").unwrap();

    let show = run_ocm(&cwd, &env, &["runtime", "show", "stable"]);
    assert_eq!(show.status.code(), Some(1));
    assert!(stderr(&show).contains("runtime \"stable\" sha256 mismatch:"));

    let which = run_ocm(&cwd, &env, &["runtime", "which", "stable"]);
    assert_eq!(which.status.code(), Some(1));
    assert!(stderr(&which).contains("runtime \"stable\" sha256 mismatch:"));
}

#[test]
fn env_resolve_and_run_fail_when_the_installed_runtime_checksum_drifted() {
    let root = TestDir::new("runtime-checksum-drift-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let artifact_body = b"expected-runtime";
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
            "--version",
            "0.2.0",
        ],
    );
    assert!(install.status.success(), "{}", stderr(&install));

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--runtime", "stable"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    let installed_binary = runtime_install_root("stable", &env, &cwd)
        .unwrap()
        .join("files/openclaw-stable");
    fs::write(&installed_binary, b"tampered-runtime").unwrap();

    let resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo"]);
    assert_eq!(resolve.status.code(), Some(1));
    assert!(stderr(&resolve).contains("runtime \"stable\" sha256 mismatch:"));

    let run = run_ocm(&cwd, &env, &["env", "run", "demo", "--"]);
    assert_eq!(run.status.code(), Some(1));
    assert!(stderr(&run).contains("runtime \"stable\" sha256 mismatch:"));
}
