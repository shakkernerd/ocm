mod support;

use std::fs;

use ocm::env::{
    CloneEnvironmentOptions, CreateEnvSnapshotOptions, CreateEnvironmentOptions,
    EnvironmentService, ExportEnvironmentOptions, ImportEnvironmentOptions,
    RemoveEnvSnapshotOptions, RestoreEnvSnapshotOptions,
};
use ocm::infra::archive::{EnvArchiveManifest, extract_env_archive};
use ocm::launcher::AddLauncherOptions;
use ocm::runtime::{AddRuntimeOptions, InstallRuntimeOptions, RuntimeSourceKind};
use ocm::store::{
    add_launcher, add_runtime, clone_environment, create_env_snapshot, create_environment,
    env_meta_path, export_environment, get_env_snapshot, get_environment, get_launcher,
    get_runtime, import_environment, install_runtime, launcher_meta_path, list_all_env_snapshots,
    list_env_snapshots, list_environments, list_launchers, list_runtimes, remove_env_snapshot,
    remove_environment, remove_launcher, remove_runtime, repair_environment_marker,
    restore_env_snapshot, runtime_install_root, runtime_meta_path,
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
    assert_ne!(cloned.gateway_port, Some(19789));
    assert!(cloned.gateway_port.unwrap() >= 19790);
    assert_eq!(cloned.default_runtime.as_deref(), Some("stable"));
    assert_eq!(cloned.default_launcher.as_deref(), Some("stable"));
    assert!(cloned.protected);
    assert!(cloned.last_used_at.is_none());
    assert_ne!(cloned.root, source.root);

    let marker_raw = fs::read_to_string(target_root.join(".ocm-env.json")).unwrap();
    assert!(marker_raw.contains("\"name\": \"target\""));
}

#[test]
fn clone_environment_skips_busy_ports_when_assigning_a_new_identity() {
    let root = TestDir::new("store-env-clone-port-busy");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let source = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: None,
            default_launcher: None,
            protected: false,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 19790)).unwrap();

    let cloned = clone_environment(
        CloneEnvironmentOptions {
            source_name: source.name,
            name: "target".to_string(),
            root: None,
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_ne!(cloned.gateway_port, Some(19789));
    assert_ne!(cloned.gateway_port, Some(19790));
    assert!(cloned.gateway_port.unwrap() >= 19791);
    drop(occupied);
}

#[test]
fn clone_environment_assigns_a_new_port_when_the_source_only_had_a_computed_port() {
    let root = TestDir::new("store-env-clone-computed-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let source = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: None,
            default_runtime: None,
            default_launcher: Some("stable".to_string()),
            protected: false,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let cloned = clone_environment(
        CloneEnvironmentOptions {
            source_name: source.name,
            name: "target".to_string(),
            root: None,
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_ne!(cloned.gateway_port, Some(18789));
    assert!(cloned.gateway_port.unwrap() >= 18790);
}

#[test]
fn clone_environment_rewrites_env_scoped_config_paths_and_ports() {
    let root = TestDir::new("store-env-clone-config-rewrite");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let source = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: None,
            default_launcher: Some("stable".to_string()),
            protected: false,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let source_root = std::path::Path::new(&source.root);
    let source_workspace = source_root.join(".openclaw/workspace");
    write_text(
        &source_root.join(".openclaw/openclaw.json"),
        &format!(
            concat!(
                "{{\n",
                "  \"agents\": {{\n",
                "    \"defaults\": {{\n",
                "      \"workspace\": \"{}\"\n",
                "    }}\n",
                "  }},\n",
                "  \"gateway\": {{\n",
                "    \"port\": 19789\n",
                "  }},\n",
                "  \"outside\": \"/tmp/keep-me\"\n",
                "}}\n"
            ),
            source_workspace.display()
        ),
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

    let cloned_root = std::path::Path::new(&cloned.root);
    let config_raw = fs::read_to_string(cloned_root.join(".openclaw/openclaw.json")).unwrap();
    let config: serde_json::Value = serde_json::from_str(&config_raw).unwrap();
    let expected_workspace = cloned_root
        .join(".openclaw/workspace")
        .display()
        .to_string();
    assert_eq!(
        config["agents"]["defaults"]["workspace"].as_str(),
        Some(expected_workspace.as_str())
    );
    assert_eq!(
        config["gateway"]["port"].as_u64(),
        Some(cloned.gateway_port.unwrap() as u64)
    );
    assert_eq!(config["outside"].as_str(), Some("/tmp/keep-me"));
}

#[test]
fn clone_environment_clears_copied_runtime_state_outside_workspace_and_config() {
    let root = TestDir::new("store-env-clone-clears-runtime-state");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let source = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: None,
            default_launcher: Some("stable".to_string()),
            protected: false,
        },
        &env,
        &cwd,
    )
    .unwrap();
    let source_root = std::path::Path::new(&source.root);
    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "keep workspace",
    );
    write_text(
        &source_root.join(".openclaw/agents/main/agent/auth-profiles.json"),
        "{\n  \"profiles\": {\"local\": {\"provider\": \"openai-codex\"}}\n}\n",
    );
    write_text(
        &source_root.join(".openclaw/agents/main/sessions/main.jsonl"),
        &format!(
            "{{\"cwd\":\"{}\"}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
    );
    write_text(
        &source_root.join(".openclaw/logs/gateway.log"),
        &format!("root={}\n", source_root.display()),
    );
    write_text(
        &source_root.join(".openclaw/openclaw.json.bak"),
        &format!(
            "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
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

    let cloned_root = std::path::Path::new(&cloned.root);
    assert_eq!(
        fs::read_to_string(cloned_root.join(".openclaw/workspace/notes.txt")).unwrap(),
        "keep workspace"
    );
    assert!(
        cloned_root
            .join(".openclaw/agents/main/agent/auth-profiles.json")
            .exists()
    );
    assert!(!cloned_root.join(".openclaw/agents/main/sessions").exists());
    assert!(!cloned_root.join(".openclaw/logs").exists());
    assert!(!cloned_root.join(".openclaw/openclaw.json.bak").exists());
    if let Ok(config_raw) = fs::read_to_string(cloned_root.join(".openclaw/openclaw.json")) {
        assert!(!config_raw.contains(&source_root.display().to_string()));
    }
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
    fs::write(
        source_root.join(".openclaw/openclaw.json"),
        format!(
            "{{\n  \"agents\": {{\n    \"defaults\": {{\n      \"workspace\": \"{}\"\n    }}\n  }},\n  \"gateway\": {{\n    \"port\": 19789\n  }}\n}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
    )
    .unwrap();

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
    let imported_config =
        fs::read_to_string(root.child("workspace/imports/target-root/.openclaw/openclaw.json"))
            .unwrap();
    let imported_config: serde_json::Value = serde_json::from_str(&imported_config).unwrap();
    let actual_workspace = fs::canonicalize(std::path::Path::new(
        imported_config["agents"]["defaults"]["workspace"]
            .as_str()
            .unwrap(),
    ))
    .unwrap();
    let expected_workspace = fs::canonicalize(root.child("workspace/imports/target-root"))
        .unwrap()
        .join(".openclaw/workspace");
    assert_eq!(actual_workspace, expected_workspace);
    assert_eq!(imported_config["gateway"]["port"].as_u64(), Some(19789));

    let marker_raw =
        fs::read_to_string(root.child("workspace/imports/target-root/.ocm-env.json")).unwrap();
    assert!(marker_raw.contains("\"name\": \"target\""));
}

#[test]
fn environment_import_clears_copied_runtime_state_outside_workspace_and_config() {
    let root = TestDir::new("store-env-import-runtime-cleanup");
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
    fs::create_dir_all(source_root.join(".openclaw/agents/main/agent")).unwrap();
    fs::create_dir_all(source_root.join(".openclaw/agents/main/sessions")).unwrap();
    fs::create_dir_all(source_root.join(".openclaw/logs")).unwrap();
    write_text(
        &source_root.join(".openclaw/agents/main/agent/auth-profiles.json"),
        "{\"default\":\"ok\"}\n",
    );
    write_text(
        &source_root.join(".openclaw/agents/main/agent/models.json"),
        "{\"primary\":\"gpt-5.4\"}\n",
    );
    write_text(
        &source_root.join(".openclaw/agents/main/sessions/main.jsonl"),
        "{\"type\":\"session\"}\n",
    );
    write_text(
        &source_root.join(".openclaw/logs/gateway.log"),
        "copied log\n",
    );
    write_text(&source_root.join(".openclaw/openclaw.json.bak"), "{}\n");

    let exported = export_environment(
        ExportEnvironmentOptions {
            name: "source".to_string(),
            output: Some("./archives/source-runtime.tar".to_string()),
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

    let imported_root = std::path::Path::new(&imported.root);
    assert!(
        imported_root
            .join(".openclaw/agents/main/agent/auth-profiles.json")
            .exists()
    );
    assert!(
        imported_root
            .join(".openclaw/agents/main/agent/models.json")
            .exists()
    );
    assert!(
        !imported_root
            .join(".openclaw/agents/main/sessions")
            .exists()
    );
    assert!(!imported_root.join(".openclaw/logs").exists());
    assert!(!imported_root.join(".openclaw/openclaw.json.bak").exists());
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

#[test]
fn environment_snapshot_restore_replaces_env_state_from_the_snapshot() {
    let root = TestDir::new("store-env-snapshot-restore");
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
        "before restore",
    );
    fs::write(
        source_root.join(".openclaw/openclaw.json"),
        format!(
            "{{\n  \"agents\": {{\n    \"defaults\": {{\n      \"workspace\": \"{}\"\n    }}\n  }},\n  \"gateway\": {{\n    \"port\": 19789\n  }}\n}}\n",
            source_root.join(".openclaw/workspace").display()
        ),
    )
    .unwrap();

    let snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "source".to_string(),
            label: Some("before-upgrade".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "after drift",
    );
    fs::write(
        source_root.join(".openclaw/openclaw.json"),
        "{\n  \"agents\": {\n    \"defaults\": {\n      \"workspace\": \"/tmp/foreign/.openclaw/workspace\"\n    }\n  },\n  \"gateway\": {\n    \"port\": 20000\n  }\n}\n",
    )
    .unwrap();
    let mut drifted = get_environment("source", &env, &cwd).unwrap();
    drifted.default_launcher = None;
    drifted.default_runtime = None;
    drifted.gateway_port = Some(20000);
    drifted.protected = false;
    ocm::store::save_environment(drifted, &env, &cwd).unwrap();

    let restored = restore_env_snapshot(
        RestoreEnvSnapshotOptions {
            env_name: "source".to_string(),
            snapshot_id: snapshot.id.clone(),
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_eq!(restored.snapshot_id, snapshot.id);
    assert_eq!(restored.label.as_deref(), Some("before-upgrade"));
    assert_eq!(
        fs::read_to_string(source_root.join(".openclaw/workspace/notes.txt")).unwrap(),
        "before restore"
    );
    let restored_config = fs::read_to_string(source_root.join(".openclaw/openclaw.json")).unwrap();
    let restored_config: serde_json::Value = serde_json::from_str(&restored_config).unwrap();
    let actual_workspace = fs::canonicalize(std::path::Path::new(
        restored_config["agents"]["defaults"]["workspace"]
            .as_str()
            .unwrap(),
    ))
    .unwrap();
    let expected_workspace = fs::canonicalize(source_root)
        .unwrap()
        .join(".openclaw/workspace");
    assert_eq!(actual_workspace, expected_workspace);
    assert_eq!(restored_config["gateway"]["port"].as_u64(), Some(19789));

    let restored_meta = get_environment("source", &env, &cwd).unwrap();
    assert_eq!(restored_meta.gateway_port, Some(19789));
    assert_eq!(restored_meta.default_runtime.as_deref(), Some("stable"));
    assert_eq!(restored_meta.default_launcher.as_deref(), Some("shell"));
    assert!(restored_meta.protected);
}

#[test]
fn environment_snapshot_restore_clears_broken_foreign_runtime_state_but_keeps_agent_auth() {
    let root = TestDir::new("store-env-snapshot-restore-runtime-cleanup");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let foreign = create_environment(
        CreateEnvironmentOptions {
            name: "foreign".to_string(),
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
    let created = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
            root: None,
            gateway_port: Some(19789),
            default_runtime: Some("stable".to_string()),
            default_launcher: Some("shell".to_string()),
            protected: false,
        },
        &env,
        &cwd,
    )
    .unwrap();

    let source_root = std::path::Path::new(&created.root);
    let agent_dir = source_root.join(".openclaw/agents/main/agent");
    let sessions_dir = source_root.join(".openclaw/agents/main/sessions");
    fs::create_dir_all(&agent_dir).unwrap();
    fs::create_dir_all(&sessions_dir).unwrap();
    write_text(&agent_dir.join("auth-profiles.json"), "{\"ok\":true}");
    write_text(
        &sessions_dir.join("main.jsonl"),
        &format!("{{\"cwd\":\"{}/.openclaw/workspace\"}}\n", foreign.root),
    );

    let snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "source".to_string(),
            label: Some("before-repair".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    write_text(
        &source_root.join(".openclaw/workspace/notes.txt"),
        "after drift",
    );

    let restored = restore_env_snapshot(
        RestoreEnvSnapshotOptions {
            env_name: "source".to_string(),
            snapshot_id: snapshot.id.clone(),
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_eq!(restored.snapshot_id, snapshot.id);
    assert!(
        source_root
            .join(".openclaw/agents/main/agent/auth-profiles.json")
            .exists()
    );
    assert!(
        !source_root
            .join(".openclaw/agents/main/sessions/main.jsonl")
            .exists()
    );
    assert!(
        !source_root.join(".openclaw/logs").exists(),
        "restore should clear only broken non-portable runtime residue"
    );
}

#[test]
fn environment_snapshot_remove_deletes_snapshot_artifacts_and_metadata() {
    let root = TestDir::new("store-env-snapshot-remove");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
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

    let snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "source".to_string(),
            label: Some("before-cleanup".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    let removed = remove_env_snapshot(
        RemoveEnvSnapshotOptions {
            env_name: "source".to_string(),
            snapshot_id: snapshot.id.clone(),
        },
        &env,
        &cwd,
    )
    .unwrap();

    assert_eq!(removed.env_name, "source");
    assert_eq!(removed.snapshot_id, snapshot.id);
    assert_eq!(removed.label.as_deref(), Some("before-cleanup"));
    assert!(!std::path::Path::new(&removed.archive_path).exists());
    assert!(list_env_snapshots("source", &env, &cwd).unwrap().is_empty());
    assert!(!root.child("ocm-home/snapshots/source").exists());
}

#[test]
fn environment_marker_repair_rewrites_the_marker_for_the_current_env_name() {
    let root = TestDir::new("store-env-marker-repair");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
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
    let marker_path = std::path::Path::new(&created.root).join(".ocm-env.json");
    fs::write(
        &marker_path,
        "{\n  \"kind\": \"ocm-env-marker\",\n  \"name\": \"other\",\n  \"createdAt\": \"2026-03-25T00:00:00Z\"\n}\n",
    )
    .unwrap();

    let repaired = repair_environment_marker("source", &env, &cwd).unwrap();
    assert_eq!(repaired.env_name, "source");
    assert_eq!(repaired.marker_path, marker_path.display().to_string());

    let marker_raw = fs::read_to_string(marker_path).unwrap();
    assert!(marker_raw.contains("\"name\": \"source\""));
}

#[test]
fn environment_snapshot_prune_keeps_the_newest_snapshot_in_scope() {
    let root = TestDir::new("store-env-snapshot-prune");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
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

    let old_snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "source".to_string(),
            label: Some("old".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();
    let new_snapshot = create_env_snapshot(
        CreateEnvSnapshotOptions {
            env_name: "source".to_string(),
            label: Some("new".to_string()),
        },
        &env,
        &cwd,
    )
    .unwrap();

    let service = EnvironmentService::new(&env, &cwd);
    let candidates = service
        .prune_snapshot_candidates(Some("source"), Some(1), None)
        .unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].id, old_snapshot.id);

    let removed = service
        .prune_snapshots(Some("source"), Some(1), None)
        .unwrap();
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].snapshot_id, old_snapshot.id);

    let remaining = list_env_snapshots("source", &env, &cwd).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, new_snapshot.id);
}

#[test]
fn environment_cleanup_preview_identifies_safe_repairs_without_mutating_state() {
    let root = TestDir::new("store-env-cleanup-preview");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
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

    let marker_path = root.child("ocm-home/envs/source/.ocm-env.json");
    fs::remove_file(&marker_path).unwrap();

    let mut drifted = get_environment("source", &env, &cwd).unwrap();
    drifted.default_runtime = Some("ghost-runtime".to_string());
    drifted.default_launcher = Some("ghost-launcher".to_string());
    ocm::store::save_environment(drifted, &env, &cwd).unwrap();

    let service = EnvironmentService::new(&env, &cwd);
    let preview = service.cleanup_preview("source").unwrap();
    assert!(!preview.apply);
    assert_eq!(preview.actions.len(), 3);
    assert_eq!(preview.actions[0].kind, "repair-marker");
    assert_eq!(preview.actions[1].kind, "clear-missing-runtime");
    assert_eq!(preview.actions[2].kind, "clear-missing-launcher");
    assert!(preview.actions.iter().all(|action| !action.applied));

    let current = get_environment("source", &env, &cwd).unwrap();
    assert_eq!(current.default_runtime.as_deref(), Some("ghost-runtime"));
    assert_eq!(current.default_launcher.as_deref(), Some("ghost-launcher"));
    assert!(!marker_path.exists());
}

#[test]
fn environment_cleanup_applies_marker_and_binding_repairs() {
    let root = TestDir::new("store-env-cleanup-apply");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    create_environment(
        CreateEnvironmentOptions {
            name: "source".to_string(),
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

    let marker_path = root.child("ocm-home/envs/source/.ocm-env.json");
    fs::write(
        &marker_path,
        "{\n  \"kind\": \"ocm-env-marker\",\n  \"name\": \"other\",\n  \"createdAt\": \"2026-03-25T00:00:00Z\"\n}\n",
    )
    .unwrap();

    let mut drifted = get_environment("source", &env, &cwd).unwrap();
    drifted.default_runtime = Some("ghost-runtime".to_string());
    drifted.default_launcher = Some("ghost-launcher".to_string());
    ocm::store::save_environment(drifted, &env, &cwd).unwrap();

    let service = EnvironmentService::new(&env, &cwd);
    let applied = service.cleanup("source").unwrap();
    assert!(applied.apply);
    assert_eq!(applied.actions.len(), 3);
    assert!(applied.actions.iter().all(|action| action.applied));

    let current = get_environment("source", &env, &cwd).unwrap();
    assert_eq!(current.default_runtime, None);
    assert_eq!(current.default_launcher, None);

    let marker_raw = fs::read_to_string(marker_path).unwrap();
    assert!(marker_raw.contains("\"name\": \"source\""));
    assert_eq!(applied.healthy_after, Some(false));
    assert!(
        applied
            .issues_after
            .unwrap()
            .iter()
            .any(|issue| issue.contains("has no default runtime or launcher"))
    );
}

#[test]
fn environment_cleanup_all_limits_results_to_envs_with_safe_repairs() {
    let root = TestDir::new("store-env-cleanup-all");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    create_environment(
        CreateEnvironmentOptions {
            name: "broken".to_string(),
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
    create_environment(
        CreateEnvironmentOptions {
            name: "healthy".to_string(),
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

    fs::remove_file(root.child("ocm-home/envs/broken/.ocm-env.json")).unwrap();

    let service = EnvironmentService::new(&env, &cwd);
    let preview = service.cleanup_all_preview().unwrap();
    assert!(!preview.apply);
    assert_eq!(preview.count, 1);
    assert_eq!(preview.results.len(), 1);
    assert_eq!(preview.results[0].env_name, "broken");

    let applied = service.cleanup_all().unwrap();
    assert!(applied.apply);
    assert_eq!(applied.count, 1);
    assert!(root.child("ocm-home/envs/broken/.ocm-env.json").exists());
    assert!(root.child("ocm-home/envs/healthy/.ocm-env.json").exists());
}
