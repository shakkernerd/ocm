mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_text};

#[test]
fn env_snapshot_create_captures_the_current_environment_state() {
    let root = TestDir::new("env-snapshot-create");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "source", "--port", "19789", "--protect"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    write_text(
        &root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"),
        "hello snapshot",
    );

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "source",
            "--label",
            "before-upgrade",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));
    let output = stdout(&snapshot);
    assert!(output.contains("Created snapshot"));
    assert!(output.contains("for env source"));
    assert!(output.contains("label: before-upgrade"));
    assert!(root.child("ocm-home/snapshots/source").exists());
}

#[test]
fn env_snapshot_create_json_reports_snapshot_metadata() {
    let root = TestDir::new("env-snapshot-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let snapshot = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--json"],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));
    let output = stdout(&snapshot);
    assert!(output.contains("\"envName\": \"source\""));
    assert!(output.contains("\"archivePath\":"));
    assert!(output.contains("\"id\":"));
}

#[test]
fn env_snapshot_list_reports_env_scoped_snapshots_in_newest_first_order() {
    let root = TestDir::new("env-snapshot-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let first = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--label", "first"],
    );
    assert!(first.status.success(), "{}", stderr(&first));

    let second = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--label", "second"],
    );
    assert!(second.status.success(), "{}", stderr(&second));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    let second_index = output.find("label=second").unwrap();
    let first_index = output.find("label=first").unwrap();
    assert!(second_index < first_index);
}

#[test]
fn env_snapshot_list_json_supports_the_global_view() {
    let root = TestDir::new("env-snapshot-list-all");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    for name in ["alpha", "beta"] {
        let create = run_ocm(&cwd, &env, &["env", "create", name]);
        assert!(create.status.success(), "{}", stderr(&create));
        let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", name]);
        assert!(snapshot.status.success(), "{}", stderr(&snapshot));
    }

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "--all", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("\"envName\": \"alpha\""));
    assert!(output.contains("\"envName\": \"beta\""));
}
