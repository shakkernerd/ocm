mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn env_create_prints_the_effective_gateway_port_for_fresh_envs() {
    let root = TestDir::new("env-create-output-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));
    let output = stdout(&created);
    assert!(output.contains("Created env demo"));
    assert!(output.contains("effective gateway port: 18789 (computed)"));
}

#[test]
fn env_clone_prints_the_effective_gateway_port_for_the_cloned_env() {
    let root = TestDir::new("env-clone-output-port");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let cloned = run_ocm(&cwd, &env, &["env", "clone", "demo", "copy"]);
    assert!(cloned.status.success(), "{}", stderr(&cloned));
    let output = stdout(&cloned);
    assert!(output.contains("Cloned env copy from demo"));
    assert!(output.contains("effective gateway port: 18790 (computed)"));
}
