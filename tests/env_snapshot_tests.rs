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
fn env_snapshot_show_reports_snapshot_metadata() {
    let root = TestDir::new("env-snapshot-show");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "source", "--port", "19789", "--protect"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

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

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let snapshot_id = stdout(&list)
        .split("\"id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();

    let show = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "show", "source", &snapshot_id],
    );
    assert!(show.status.success(), "{}", stderr(&show));
    let output = stdout(&show);
    assert!(output.contains("snapshotId:"));
    assert!(output.contains("envName: source"));
    assert!(output.contains("label: before-upgrade"));
    assert!(output.contains("gatewayPort: 19789"));
    assert!(output.contains("protected: true"));
}

#[test]
fn env_snapshot_show_json_reports_the_snapshot_shape() {
    let root = TestDir::new("env-snapshot-show-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "source"]);
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let snapshot_id = stdout(&list)
        .split("\"id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();

    let show = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "show", "source", &snapshot_id, "--json"],
    );
    assert!(show.status.success(), "{}", stderr(&show));
    let output = stdout(&show);
    assert!(output.contains("\"envName\": \"source\""));
    assert!(output.contains("\"archivePath\":"));
    assert!(output.contains("\"createdAt\":"));
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

#[test]
fn env_snapshot_restore_reverts_state_from_the_selected_snapshot() {
    let root = TestDir::new("env-snapshot-restore");
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
        "before restore",
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

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let snapshot_list = stdout(&list);
    let snapshot_id = snapshot_list
        .split("\"id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();

    write_text(
        &root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"),
        "after drift",
    );

    let restore = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "restore", "source", &snapshot_id],
    );
    assert!(restore.status.success(), "{}", stderr(&restore));
    let output = stdout(&restore);
    assert!(output.contains("Restored env source from snapshot"));
    assert!(output.contains("label: before-upgrade"));
    assert_eq!(
        fs::read_to_string(root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"))
            .unwrap(),
        "before restore"
    );
}

#[test]
fn env_snapshot_restore_json_reports_the_restored_binding_shape() {
    let root = TestDir::new("env-snapshot-restore-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source", "--protect"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "source"]);
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let snapshot_list = stdout(&list);
    let snapshot_id = snapshot_list
        .split("\"id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();

    let restore = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "restore",
            "source",
            &snapshot_id,
            "--json",
        ],
    );
    assert!(restore.status.success(), "{}", stderr(&restore));
    let output = stdout(&restore);
    assert!(output.contains("\"envName\": \"source\""));
    assert!(output.contains("\"snapshotId\":"));
    assert!(output.contains("\"protected\": true"));
}

#[test]
fn env_snapshot_remove_deletes_the_named_snapshot() {
    let root = TestDir::new("env-snapshot-remove");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "source",
            "--label",
            "before-cleanup",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let snapshot_id = stdout(&list)
        .split("\"id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();

    let remove = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "remove", "source", &snapshot_id],
    );
    assert!(remove.status.success(), "{}", stderr(&remove));
    let output = stdout(&remove);
    assert!(output.contains("Removed snapshot"));
    assert!(output.contains("for env source"));
    assert!(output.contains("label: before-cleanup"));

    let list_after = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source"]);
    assert!(list_after.status.success(), "{}", stderr(&list_after));
    assert!(stdout(&list_after).contains("No snapshots."));
}

#[test]
fn env_snapshot_remove_json_reports_removed_snapshot_metadata() {
    let root = TestDir::new("env-snapshot-remove-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "source"]);
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let snapshot_id = stdout(&list)
        .split("\"id\": \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_string();

    let remove = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "remove",
            "source",
            &snapshot_id,
            "--json",
        ],
    );
    assert!(remove.status.success(), "{}", stderr(&remove));
    let output = stdout(&remove);
    assert!(output.contains("\"envName\": \"source\""));
    assert!(output.contains("\"snapshotId\":"));
    assert!(output.contains("\"archivePath\":"));
}

#[test]
fn env_snapshot_prune_previews_candidates_without_removing_them() {
    let root = TestDir::new("env-snapshot-prune-preview");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let old = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--label", "old"],
    );
    assert!(old.status.success(), "{}", stderr(&old));
    let new = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--label", "new"],
    );
    assert!(new.status.success(), "{}", stderr(&new));

    let prune = run_ocm(&cwd, &env, &["env", "snapshot", "prune", "source", "--keep", "1"]);
    assert!(prune.status.success(), "{}", stderr(&prune));
    let output = stdout(&prune);
    assert!(output.contains("Snapshot prune preview (source): 1 candidate(s)"));
    assert!(output.contains("label=old"));
    assert!(output.contains("Re-run with --yes to remove them."));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let listed = stdout(&list);
    assert!(listed.contains("\"label\": \"old\""));
    assert!(listed.contains("\"label\": \"new\""));
}

#[test]
fn env_snapshot_prune_yes_removes_selected_snapshots() {
    let root = TestDir::new("env-snapshot-prune-apply");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let old = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--label", "old"],
    );
    assert!(old.status.success(), "{}", stderr(&old));
    let new = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "create", "source", "--label", "new"],
    );
    assert!(new.status.success(), "{}", stderr(&new));

    let prune = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "prune", "source", "--keep", "1", "--yes"],
    );
    assert!(prune.status.success(), "{}", stderr(&prune));
    let output = stdout(&prune);
    assert!(output.contains("Pruned 1 snapshot(s)."));
    assert!(output.contains("label=old"));

    let list = run_ocm(&cwd, &env, &["env", "snapshot", "list", "source", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let listed = stdout(&list);
    assert!(!listed.contains("\"label\": \"old\""));
    assert!(listed.contains("\"label\": \"new\""));
}

#[test]
fn env_snapshot_prune_json_supports_the_global_view() {
    let root = TestDir::new("env-snapshot-prune-json-all");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    for name in ["alpha", "beta"] {
        let create = run_ocm(&cwd, &env, &["env", "create", name]);
        assert!(create.status.success(), "{}", stderr(&create));
        let old = run_ocm(
            &cwd,
            &env,
            &["env", "snapshot", "create", name, "--label", "old"],
        );
        assert!(old.status.success(), "{}", stderr(&old));
        let new = run_ocm(
            &cwd,
            &env,
            &["env", "snapshot", "create", name, "--label", "new"],
        );
        assert!(new.status.success(), "{}", stderr(&new));
    }

    let prune = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "prune",
            "--all",
            "--keep",
            "1",
            "--json",
        ],
    );
    assert!(prune.status.success(), "{}", stderr(&prune));
    let output = stdout(&prune);
    assert!(output.contains("\"apply\": false"));
    assert!(output.contains("\"scope\": \"all\""));
    assert!(output.contains("\"count\": 2"));
    assert!(output.contains("\"envName\": \"alpha\""));
    assert!(output.contains("\"envName\": \"beta\""));
}
