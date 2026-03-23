mod support;

use std::fs;

use crate::support::{ocm_env, run_ocm, stderr, stdout, TestDir};

#[test]
fn launcher_list_uses_launcher_wording_when_empty() {
    let root = TestDir::new("launcher-list-empty");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let list = run_ocm(&cwd, &env, &["launcher", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert_eq!(stdout(&list), "No launchers.\n");
}

#[test]
fn launcher_add_and_list_use_the_existing_version_store() {
    let root = TestDir::new("launcher-add-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let add = run_ocm(
        &cwd,
        &env,
        &[
            "launcher",
            "add",
            "stable",
            "--command",
            "sh",
            "--description",
            "launcher alias",
        ],
    );
    assert!(add.status.success(), "{}", stderr(&add));
    assert!(stdout(&add).contains("Added launcher stable"));

    let list = run_ocm(&cwd, &env, &["launcher", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert!(stdout(&list).contains("stable  sh"));

    let version_list = run_ocm(&cwd, &env, &["version", "list", "--json"]);
    assert!(version_list.status.success(), "{}", stderr(&version_list));
    assert!(stdout(&version_list).contains("\"name\": \"stable\""));
    assert!(stdout(&version_list).contains("\"description\": \"launcher alias\""));
}
