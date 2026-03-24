mod support;

use std::fs;

use ocm::paths::{env_meta_path, launcher_meta_path};
use ocm::store::{
    add_launcher, create_environment, get_environment, get_launcher, list_environments,
    list_launchers, remove_environment, remove_launcher,
};
use ocm::types::{AddLauncherOptions, CreateEnvironmentOptions};

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
