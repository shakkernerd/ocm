mod support;

use std::fs;
use std::path::Path;

use ocm::infra::archive::{EnvArchiveMetadata, extract_env_archive, write_env_archive};
use ocm::store::list_env_snapshots;

use crate::support::{TestDir, ocm_env, run_ocm, stderr};

#[test]
fn env_snapshot_create_refuses_to_capture_a_root_without_the_marker_file() {
    let root = TestDir::new("env-snapshot-create-safety");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/envs/source/.ocm-env.json")).unwrap();

    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "source"]);
    assert_eq!(snapshot.status.code(), Some(1));
    let error = stderr(&snapshot);
    assert!(error.contains("refusing to snapshot"));
    assert!(error.contains(".ocm-env.json"));
}

#[test]
fn env_snapshot_restore_refuses_to_overwrite_a_root_without_the_marker_file() {
    let root = TestDir::new("env-snapshot-restore-safety");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "source"]);
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let snapshots = list_env_snapshots("source", &env, &cwd).unwrap();
    let snapshot_id = snapshots[0].id.clone();
    fs::remove_file(root.child("ocm-home/envs/source/.ocm-env.json")).unwrap();

    let restore = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "restore", "source", &snapshot_id],
    );
    assert_eq!(restore.status.code(), Some(1));
    let error = stderr(&restore);
    assert!(error.contains("refusing to restore"));
    assert!(error.contains(".ocm-env.json"));
}

#[test]
fn env_snapshot_restore_refuses_tampered_snapshot_archives_without_markers() {
    let root = TestDir::new("env-snapshot-archive-safety");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let snapshot = run_ocm(&cwd, &env, &["env", "snapshot", "create", "source"]);
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let snapshots = list_env_snapshots("source", &env, &cwd).unwrap();
    let snapshot_meta = &snapshots[0];
    let extract_dir = root.child("snapshot-extracted");
    let extracted = extract_env_archive::<EnvArchiveMetadata>(
        Path::new(&snapshot_meta.archive_path),
        &extract_dir,
    )
    .unwrap();
    fs::remove_file(extracted.root_dir.join(".ocm-env.json")).unwrap();
    write_env_archive(
        &extracted.metadata,
        &extracted.root_dir,
        Path::new(&snapshot_meta.archive_path),
    )
    .unwrap();

    let restore = run_ocm(
        &cwd,
        &env,
        &["env", "snapshot", "restore", "source", &snapshot_meta.id],
    );
    assert_eq!(restore.status.code(), Some(1));
    assert!(stderr(&restore).contains("snapshot archive is missing .ocm-env.json"));
}
