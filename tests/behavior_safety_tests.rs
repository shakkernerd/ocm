mod support;

use std::fs;

use ocm::paths::{clean_path, env_meta_path};

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

    let meta_path = env_meta_path("demo", &env, &cwd).unwrap();
    assert!(!meta_path.exists());
}

#[test]
fn removing_an_environment_without_the_marker_file_requires_force() {
    let root = TestDir::new("behavior-marker-remove");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(create.status.success(), "{}", stderr(&create));

    let env_root = clean_path(&root.child("ocm-home/envs/demo"));
    fs::remove_file(env_root.join(".ocm-env.json")).unwrap();

    let remove = run_ocm(&cwd, &env, &["env", "remove", "demo"]);
    assert!(!remove.status.success());
    let error = stderr(&remove);
    assert!(error.contains(".ocm-env.json"));
    assert!(error.contains("--force"));

    let force_remove = run_ocm(&cwd, &env, &["env", "remove", "demo", "--force"]);
    assert!(force_remove.status.success(), "{}", stderr(&force_remove));
    assert!(!env_root.exists());
}
