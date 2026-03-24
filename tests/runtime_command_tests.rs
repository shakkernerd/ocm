mod support;

use std::fs;

use ocm::paths::runtime_install_root;

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
