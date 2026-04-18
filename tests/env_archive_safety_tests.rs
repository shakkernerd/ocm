mod support;

use std::fs;

use ocm::infra::archive::{EnvArchiveMetadata, extract_env_archive, write_env_archive};

use crate::support::{TestDir, ocm_env, run_ocm, stderr, write_text};

#[test]
fn env_export_refuses_to_archive_a_root_without_the_marker_file() {
    let root = TestDir::new("env-export-marker-safety");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));

    fs::remove_file(root.child("ocm-home/envs/source/.ocm-env.json")).unwrap();

    let export = run_ocm(&cwd, &env, &["env", "export", "source"]);
    assert_eq!(export.status.code(), Some(1));
    let error = stderr(&export);
    assert!(error.contains("refusing to export"));
    assert!(error.contains(".ocm-env.json"));
}

#[test]
fn env_import_refuses_archives_without_the_marker_file() {
    let root = TestDir::new("env-import-marker-safety");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "source"]);
    assert!(create.status.success(), "{}", stderr(&create));
    write_text(
        &root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"),
        "hello import",
    );

    let export = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "export",
            "source",
            "--output",
            "./archives/source-backup.tar",
        ],
    );
    assert!(export.status.success(), "{}", stderr(&export));

    let extract_dir = root.child("tampered-archive");
    let extracted = extract_env_archive::<EnvArchiveMetadata>(
        &cwd.join("archives/source-backup.tar"),
        &extract_dir,
    )
    .unwrap();
    fs::remove_file(extracted.root_dir.join(".ocm-env.json")).unwrap();
    let broken_archive_path = cwd.join("archives/source-without-marker.tar");
    write_env_archive(
        &extracted.metadata,
        &extracted.root_dir,
        &broken_archive_path,
    )
    .unwrap();

    let import = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "import",
            "./archives/source-without-marker.tar",
            "--name",
            "target",
        ],
    );
    assert_eq!(import.status.code(), Some(1));
    let error = stderr(&import);
    assert!(error.contains("archive environment root is missing .ocm-env.json"));
}
