mod support;

use std::fs;

use ocm::store::env_registry_path;

use crate::support::{TestDir, ocm_env, run_ocm, stderr};

#[test]
fn removing_a_protected_environment_requires_force() {
    let root = TestDir::new("behavior-protected-remove");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo", "--protect"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let remove = run_ocm(&cwd, &env, &["env", "remove", "demo"]);
    assert!(!remove.status.success());
    assert!(stderr(&remove).contains("is protected"));

    let force_remove = run_ocm(&cwd, &env, &["env", "remove", "demo", "--force"]);
    assert!(force_remove.status.success(), "{}", stderr(&force_remove));

    let registry_path = env_registry_path(&env, &cwd).unwrap();
    let registry_raw = fs::read_to_string(registry_path).unwrap();
    assert!(!registry_raw.contains("\"name\": \"demo\""));
}
