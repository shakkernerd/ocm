mod support;

use std::fs;

use ocm::archive::{EnvArchiveManifest, extract_env_archive};
use ocm::paths::{env_meta_path, launcher_meta_path, runtime_install_root, runtime_meta_path};
use ocm::store::{
    add_launcher, add_runtime, clone_environment, create_env_snapshot, create_environment,
    export_environment, get_env_snapshot, get_environment, get_launcher, get_runtime,
    import_environment, install_runtime, list_all_env_snapshots, list_env_snapshots,
    list_environments, list_launchers, list_runtimes, remove_environment, remove_launcher,
    remove_runtime,
};
use ocm::types::{
    AddLauncherOptions, AddRuntimeOptions, CloneEnvironmentOptions, CreateEnvSnapshotOptions,
    CreateEnvironmentOptions, ExportEnvironmentOptions, ImportEnvironmentOptions,
    InstallRuntimeOptions, RuntimeSourceKind,
};

use crate::support::{TestDir, ocm_env, path_string, write_executable_script, write_text};

#[test]
fn environment_store_round_trip_covers_create_read_list_and_remove() {
    let root = TestDir::new("store-env-flow");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = create_environment(
        CreateEnvironmentOptions {
            name: "alpha".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: None,
            default_launcher: Some("stable".to_string()),
            protected: true,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let meta_path = env_meta_path("alpha", &env, &cwd).unwrap();
    assert!(meta_path.exists());
    assert!(created.protected);
    assert_eq!(created.gateway_port, Some(19789));
    assert_eq!(created.default_launcher.as_deref(), Some("stable"));

    let fetched = get_environment("alpha", &env, &cwd).unwrap();
    assert_eq!(fetched.name, "alpha");
    assert!(fetched.root.ends_with("/alpha"));

    let listed = list_environments(&env, &cwd).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "alpha");

    let removed = remove_environment("alpha", true, &env, &cwd).unwrap();
    assert_eq!(removed.name, "alpha");
    assert!(!meta_path.exists());
    assert!(!std::path::Path::new(&removed.root).exists());
}

#[test]
fn launcher_store_round_trip_covers_add_show_list_and_remove() {
    let root = TestDir::new("store-launcher-flow");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let alpha = add_launcher(
        AddLauncherOptions {
            name: "alpha".to_string(),
            command: "sh".to_string(),
            cwd: None,
            description: Some("alpha launcher".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();
    let beta = add_launcher(
        AddLauncherOptions {
            name: "beta".to_string(),
            command: "openclaw".to_string(),
            cwd: Some("./launchers/beta".to_string()),
            description: None,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let fetched = get_launcher("beta", &env, &cwd).unwrap();
    assert_eq!(alpha.description.as_deref(), Some("alpha launcher"));
    assert_eq!(fetched.name, "beta");
    assert_eq!(fetched.command, "openclaw");
    assert!(fetched.cwd.as_deref().unwrap().ends_with("/launchers/beta"));

    let listed = list_launchers(&env, &cwd).unwrap();
    let names = listed.into_iter().map(|meta| meta.name).collect::<Vec<_>>();
    assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);

    let beta_path = launcher_meta_path("beta", &env, &cwd).unwrap();
    assert!(beta_path.exists());

    let removed = remove_launcher("beta", &env, &cwd).unwrap();
    assert_eq!(removed.name, beta.name);
    assert!(!beta_path.exists());
}

#[test]
fn runtime_store_round_trip_covers_add_show_list_and_remove() {
    let root = TestDir::new("store-runtime-flow");
    let cwd = root.child("workspace");
    let runtime_dir = cwd.join("runtime-bin");
    fs::create_dir_all(&runtime_dir).unwrap();
    fs::write(runtime_dir.join("stable"), "#!/bin/sh\n").unwrap();
    fs::write(runtime_dir.join("nightly"), "#!/bin/sh\n").unwrap();
    let env = ocm_env(&root);

    let stable = add_runtime(
        AddRuntimeOptions {
            name: "stable".to_string(),
            path: "./runtime-bin/stable".to_string(),
            description: Some("stable runtime".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();
    let nightly = add_runtime(
        AddRuntimeOptions {
            name: "nightly".to_string(),
            path: "./runtime-bin/nightly".to_string(),
            description: None,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let fetched = get_runtime("nightly", &env, &cwd).unwrap();
    assert_eq!(stable.description.as_deref(), Some("stable runtime"));
    assert_eq!(fetched.name, "nightly");
    assert_eq!(fetched.source_kind, RuntimeSourceKind::Registered);
    assert_eq!(fetched.install_root, None);
    assert!(fetched.binary_path.ends_with("/runtime-bin/nightly"));
    assert_eq!(
        fetched.source_path.as_deref(),
        Some(fetched.binary_path.as_str())
    );
    assert_eq!(fetched.source_url, None);

    let listed = list_runtimes(&env, &cwd).unwrap();
    let names = listed.into_iter().map(|meta| meta.name).collect::<Vec<_>>();
    assert_eq!(names, vec!["nightly".to_string(), "stable".to_string()]);

    let nightly_path = runtime_meta_path("nightly", &env, &cwd).unwrap();
    assert!(nightly_path.exists());

    let removed = remove_runtime("nightly", &env, &cwd).unwrap();
    assert_eq!(removed.name, nightly.name);
    assert!(!nightly_path.exists());
}

#[test]
fn runtime_install_copies_binary_into_the_managed_store() {
    let root = TestDir::new("store-runtime-install");
    let cwd = root.child("workspace");
    let source_dir = cwd.join("downloads");
    let source_path = source_dir.join("openclaw");
    fs::create_dir_all(&source_dir).unwrap();
    write_executable_script(&source_path, "#!/bin/sh\nexit 0\n");
    let env = ocm_env(&root);

    let installed = install_runtime(
        InstallRuntimeOptions {
            name: "stable".to_string(),
            path: "./downloads/openclaw".to_string(),
            description: Some("managed runtime".to_string()),
            force: false,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let install_root = runtime_install_root("stable", &env, &cwd).unwrap();
    let expected_binary = install_root.join("files/openclaw");
    let meta_path = runtime_meta_path("stable", &env, &cwd).unwrap();
    assert_eq!(installed.source_kind, RuntimeSourceKind::Installed);
    assert_eq!(
        installed.source_path.as_deref(),
        Some(path_string(&source_path).as_str())
    );
    assert_eq!(installed.source_url, None);
    assert_eq!(
        installed.install_root.as_deref(),
        Some(path_string(&install_root).as_str())
    );
    assert_eq!(installed.binary_path, path_string(&expected_binary));
    assert!(expected_binary.exists());
    assert!(meta_path.exists());

    let fetched = get_runtime("stable", &env, &cwd).unwrap();
    assert_eq!(fetched.source_kind, RuntimeSourceKind::Installed);
    assert_eq!(fetched.binary_path, path_string(&expected_binary));

    let removed = remove_runtime("stable", &env, &cwd).unwrap();
    assert_eq!(removed.name, "stable");
    assert!(!meta_path.exists());
    assert!(!install_root.exists());
}

#[test]
fn environment_clone_copies_the_root_and_resets_identity_metadata() {
    let root = TestDir::new("store-env-clone");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let source = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: Some("stable".to_string()),
            default_launcher: Some("stable".to_string()),
            protected: true,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let source_root = std::path::Path::new(&source.root);
    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "hello clone",
    );

    let cloned = clone_environment(
        CloneEnvironmentOptions {
            source_name: "source".to_string(),
            name: "target".to_string(),
            root: None,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let target_root = std::path::Path::new(&cloned.root);
    assert_eq!(
        fs::read_to_string(target_root.join(".openclaw/workspace/notes.txt")).unwrap(),
        "hello clone"
    );
    assert_eq!(cloned.name, "target");
    assert_eq!(cloned.gateway_port, Some(19789));
    assert_eq!(cloned.default_runtime.as_deref(), Some("stable"));
    assert_eq!(cloned.default_launcher.as_deref(), Some("stable"));
    assert!(cloned.protected);
    assert!(cloned.last_used_at.is_none());
    assert_ne!(cloned.root, source.root);

    let marker_raw = fs::read_to_string(target_root.join(".ocm-env.json")).unwrap();
    assert!(marker_raw.contains("\"name\": \"target\""));
}

#[test]
fn environment_export_writes_a_portable_archive() {
    let root = TestDir::new("store-env-export");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: Some("stable".to_string()),
            default_launcher: Some("shell".to_string()),
            protected: true,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let source_root = std::path::Path::new(&created.root);
    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "hello export",
    );

    let summary = export_environment(
        ExportEnvironmentOptions {
            name: "source".to_string(),
            output: Some("./archives/source-backup.tar".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_eq!(summary.name, "source");
    assert!(
        summary
            .archive_path
            .ends_with("/archives/source-backup.tar")
    );

    let extract_dir = root.child("extracted");
    let extracted = extract_env_archive::<EnvArchiveManifest>(
        std::path::Path::new(&summary.archive_path),
        &extract_dir,
    )
    .unwrap();
    assert_eq!(extracted.manifest.env.name, "source");
    assert_eq!(extracted.manifest.env.gateway_port, Some(19789));
    assert_eq!(
        fs::read_to_string(extracted.root_dir.join(".openclaw/workspace/notes.txt")).unwrap(),
        "hello export"
    );
}

#[test]
fn environment_import_restores_a_portable_archive_with_a_new_identity() {
    let root = TestDir::new("store-env-import");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: Some("stable".to_string()),
            default_launcher: Some("shell".to_string()),
            protected: true,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let source_root = std::path::Path::new(&created.root);
    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "hello import",
    );

    let exported = export_environment(
        ExportEnvironmentOptions {
            name: "source".to_string(),
            output: Some("./archives/source-backup.tar".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    let imported = import_environment(
        ImportEnvironmentOptions {
            archive: exported.archive_path.clone(),
            name: Some("target".to_string()),
            root: Some("./imports/target-root".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_eq!(imported.name, "target");
    assert_eq!(imported.source_name, "source");
    assert!(imported.root.ends_with("/imports/target-root"));

    let imported_meta = get_environment("target", &env, &cwd).unwrap();
    assert_eq!(imported_meta.default_runtime.as_deref(), Some("stable"));
    assert_eq!(imported_meta.default_launcher.as_deref(), Some("shell"));
    assert!(imported_meta.protected);
    assert!(imported_meta.last_used_at.is_none());
    assert_eq!(
        fs::read_to_string(
            root.child("workspace/imports/target-root/.openclaw/workspace/notes.txt")
        )
        .unwrap(),
        "hello import"
    );

    let marker_raw =
        fs::read_to_string(root.child("workspace/imports/target-root/.ocm-env.json")).unwrap();
    assert!(marker_raw.contains("\"name\": \"target\""));
}

#[test]
fn environment_snapshot_captures_a_named_point_in_time() {
    let root = TestDir::new("store-env-snapshot");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: Some("stable".to_string()),
            default_launcher: Some("shell".to_string()),
            protected: true,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let source_root = std::path::Path::new(&created.root);
    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "hello snapshot",
    );

    let snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "source".to_string(),
            label: Some("before-upgrade".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_eq!(snapshot.env_name, "source");
    assert_eq!(snapshot.label.as_deref(), Some("before-upgrade"));
    assert!(snapshot.archive_path.ends_with(".tar"));

    let fetched = get_env_snapshot("source", &snapshot.id, &env, &cwd).unwrap();
    assert_eq!(fetched.id, snapshot.id);

    let extract_dir = root.child("snapshot-extracted");
    let extracted = extract_env_archive::<EnvArchiveManifest>(
        std::path::Path::new(&snapshot.archive_path),
        &extract_dir,
    )
    .unwrap();
    assert_eq!(extracted.manifest.env.name, "source");
    assert_eq!(
        fs::read_to_string(extracted.root_dir.join(".openclaw/workspace/notes.txt")).unwrap(),
        "hello snapshot"
    );
}

#[test]
fn environment_snapshot_listing_supports_env_scoped_and_global_views() {
    let root = TestDir::new("store-env-snapshot-list");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    for name in ["alpha", "beta"] {
        create_environment(
            CreateEnvironmentOptions {
                name: name.to_string(),
                root: None,
                gateway_port: None,
                default_runtime: None,
                default_launcher: None,
                protected: false,
            },
            &env,
            &cwd,
        )
        .unwrap();
    }

    let alpha_old = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "alpha".to_string(),
            label: Some("old".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();
    let alpha_new = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "alpha".to_string(),
            label: Some("new".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();
    let beta_snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "beta".to_string(),
            label: None,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let alpha_snapshots = list_env_snapshots("alpha", &env, &cwd).unwrap();
    assert_eq!(alpha_snapshots.len(), 2);
    assert_eq!(alpha_snapshots[0].id, alpha_new.id);
    assert_eq!(alpha_snapshots[1].id, alpha_old.id);

    let all_snapshots = list_all_env_snapshots(&env, &cwd).unwrap();
    assert_eq!(all_snapshots.len(), 3);
    let ids = all_snapshots
        .into_iter()
        .map(|meta| meta.id)
        .collect::<Vec<_>>();
    assert!(ids.contains(&alpha_old.id));
    assert!(ids.contains(&alpha_new.id));
    assert!(ids.contains(&beta_snapshot.id));
}
