mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::support::{
    TestDir, base_env, install_fake_launchctl, managed_service_definition_path,
    managed_service_label, path_string, run_ocm, stderr, stdout,
};

fn scoped_launchd_env(root: &TestDir, store_name: &str) -> BTreeMap<String, String> {
    let home = root.child("home");
    let ocm_home = root.child(store_name);
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&ocm_home).unwrap();

    let mut env = base_env(&home);
    env.insert("OCM_HOME".to_string(), path_string(&ocm_home));
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    install_fake_launchctl(root, &mut env);
    env
}

fn add_launcher_and_env(cwd: &Path, env: &BTreeMap<String, String>, name: &str) {
    let launcher = run_ocm(
        cwd,
        env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(cwd, env, &["env", "create", name, "--launcher", "stable"]);
    assert!(created.status.success(), "{}", stderr(&created));
}

#[test]
fn service_commands_stay_store_scoped_for_shared_home_and_env_name() {
    let root = TestDir::new("service-store-scope");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let env_a = scoped_launchd_env(&root, "ocm-home-a");
    let env_b = scoped_launchd_env(&root, "ocm-home-b");
    add_launcher_and_env(&cwd, &env_a, "demo");
    add_launcher_and_env(&cwd, &env_b, "demo");

    let install_a = run_ocm(&cwd, &env_a, &["service", "install", "demo", "--json"]);
    assert!(install_a.status.success(), "{}", stderr(&install_a));
    let install_b = run_ocm(&cwd, &env_b, &["service", "install", "demo", "--json"]);
    assert!(install_b.status.success(), "{}", stderr(&install_b));

    let label_a = managed_service_label(&env_a, &cwd, "demo");
    let label_b = managed_service_label(&env_b, &cwd, "demo");
    let path_a = managed_service_definition_path(&env_a, &cwd, "demo");
    let path_b = managed_service_definition_path(&env_b, &cwd, "demo");

    assert_ne!(label_a, label_b);
    assert_ne!(path_a, path_b);
    assert!(path_a.exists());
    assert!(path_b.exists());

    let status_a = run_ocm(&cwd, &env_a, &["service", "status", "demo", "--json"]);
    assert!(status_a.status.success(), "{}", stderr(&status_a));
    let status_a_json: Value = serde_json::from_str(&stdout(&status_a)).unwrap();
    assert_eq!(status_a_json["managedLabel"], label_a);
    assert_eq!(status_a_json["managedPlistPath"], path_string(&path_a));

    let status_b = run_ocm(&cwd, &env_b, &["service", "status", "demo", "--json"]);
    assert!(status_b.status.success(), "{}", stderr(&status_b));
    let status_b_json: Value = serde_json::from_str(&stdout(&status_b)).unwrap();
    assert_eq!(status_b_json["managedLabel"], label_b);
    assert_eq!(status_b_json["managedPlistPath"], path_string(&path_b));

    let list_a = run_ocm(&cwd, &env_a, &["service", "list", "--json"]);
    assert!(list_a.status.success(), "{}", stderr(&list_a));
    let list_a_json: Value = serde_json::from_str(&stdout(&list_a)).unwrap();
    let service_a = list_a_json["services"].as_array().unwrap();
    assert_eq!(service_a.len(), 1);
    assert_eq!(service_a[0]["managedLabel"], label_a);

    let list_b = run_ocm(&cwd, &env_b, &["service", "list", "--json"]);
    assert!(list_b.status.success(), "{}", stderr(&list_b));
    let list_b_json: Value = serde_json::from_str(&stdout(&list_b)).unwrap();
    let service_b = list_b_json["services"].as_array().unwrap();
    assert_eq!(service_b.len(), 1);
    assert_eq!(service_b[0]["managedLabel"], label_b);

    let discover_a = run_ocm(&cwd, &env_a, &["service", "discover", "--json"]);
    assert!(discover_a.status.success(), "{}", stderr(&discover_a));
    let discover_a_json: Value = serde_json::from_str(&stdout(&discover_a)).unwrap();
    let discovered = discover_a_json["services"].as_array().unwrap();
    assert!(discovered.iter().any(|service| service["label"] == label_a));
    assert!(discovered.iter().any(|service| service["label"] == label_b));
}

#[test]
fn start_installs_store_scoped_services_for_shared_home_and_env_name() {
    let root = TestDir::new("start-store-scope");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let env_a = scoped_launchd_env(&root, "ocm-home-a");
    let env_b = scoped_launchd_env(&root, "ocm-home-b");

    let launcher_a = run_ocm(
        &cwd,
        &env_a,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher_a.status.success(), "{}", stderr(&launcher_a));
    let launcher_b = run_ocm(
        &cwd,
        &env_b,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher_b.status.success(), "{}", stderr(&launcher_b));

    let start_a = run_ocm(
        &cwd,
        &env_a,
        &["start", "demo", "--launcher", "stable", "--no-onboard"],
    );
    assert!(start_a.status.success(), "{}", stderr(&start_a));
    let start_b = run_ocm(
        &cwd,
        &env_b,
        &["start", "demo", "--launcher", "stable", "--no-onboard"],
    );
    assert!(start_b.status.success(), "{}", stderr(&start_b));

    let path_a = managed_service_definition_path(&env_a, &cwd, "demo");
    let path_b = managed_service_definition_path(&env_b, &cwd, "demo");
    assert!(path_a.exists());
    assert!(path_b.exists());
    assert_ne!(path_a, path_b);

    let status_a = run_ocm(&cwd, &env_a, &["service", "status", "demo", "--json"]);
    assert!(status_a.status.success(), "{}", stderr(&status_a));
    let status_a_json: Value = serde_json::from_str(&stdout(&status_a)).unwrap();
    assert_eq!(
        status_a_json["managedLabel"],
        managed_service_label(&env_a, &cwd, "demo")
    );

    let status_b = run_ocm(&cwd, &env_b, &["service", "status", "demo", "--json"]);
    assert!(status_b.status.success(), "{}", stderr(&status_b));
    let status_b_json: Value = serde_json::from_str(&stdout(&status_b)).unwrap();
    assert_eq!(
        status_b_json["managedLabel"],
        managed_service_label(&env_b, &cwd, "demo")
    );
}

#[test]
fn env_destroy_only_removes_the_current_stores_service_for_shared_names() {
    let root = TestDir::new("destroy-store-scope");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();

    let env_a = scoped_launchd_env(&root, "ocm-home-a");
    let env_b = scoped_launchd_env(&root, "ocm-home-b");
    add_launcher_and_env(&cwd, &env_a, "demo");
    add_launcher_and_env(&cwd, &env_b, "demo");

    let install_a = run_ocm(&cwd, &env_a, &["service", "install", "demo"]);
    assert!(install_a.status.success(), "{}", stderr(&install_a));
    let install_b = run_ocm(&cwd, &env_b, &["service", "install", "demo"]);
    assert!(install_b.status.success(), "{}", stderr(&install_b));

    let path_a = managed_service_definition_path(&env_a, &cwd, "demo");
    let path_b = managed_service_definition_path(&env_b, &cwd, "demo");
    assert!(path_a.exists());
    assert!(path_b.exists());

    let destroy_a = run_ocm(&cwd, &env_a, &["env", "destroy", "demo", "--yes"]);
    assert!(destroy_a.status.success(), "{}", stderr(&destroy_a));

    assert!(!path_a.exists());
    assert!(path_b.exists());

    let show_a = run_ocm(&cwd, &env_a, &["env", "show", "demo", "--json"]);
    assert!(!show_a.status.success());

    let show_b = run_ocm(&cwd, &env_b, &["env", "show", "demo", "--json"]);
    assert!(show_b.status.success(), "{}", stderr(&show_b));

    let status_b = run_ocm(&cwd, &env_b, &["service", "status", "demo", "--json"]);
    assert!(status_b.status.success(), "{}", stderr(&status_b));
    let status_b_json: Value = serde_json::from_str(&stdout(&status_b)).unwrap();
    assert_eq!(status_b_json["installed"], true);
    assert_eq!(
        status_b_json["managedLabel"],
        managed_service_label(&env_b, &cwd, "demo")
    );
}
