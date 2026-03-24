mod support;

use std::fs;

use ocm::paths::{env_meta_path, launcher_meta_path, runtime_meta_path};
use ocm::store::{
    add_launcher, add_runtime, create_environment, get_environment, get_launcher, get_runtime,
    list_environments, list_launchers, list_runtimes, remove_environment, remove_launcher,
    remove_runtime,
};
use ocm::types::{
    AddLauncherOptions, AddRuntimeOptions, CreateEnvironmentOptions, RuntimeSourceKind,
};

use crate::support::{TestDir, ocm_env};

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

    let listed = list_runtimes(&env, &cwd).unwrap();
    let names = listed.into_iter().map(|meta| meta.name).collect::<Vec<_>>();
    assert_eq!(names, vec!["nightly".to_string(), "stable".to_string()]);

    let nightly_path = runtime_meta_path("nightly", &env, &cwd).unwrap();
    assert!(nightly_path.exists());

    let removed = remove_runtime("nightly", &env, &cwd).unwrap();
    assert_eq!(removed.name, nightly.name);
    assert!(!nightly_path.exists());
}
