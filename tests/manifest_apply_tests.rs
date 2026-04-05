mod support;

use std::fs;

use ocm::manifest::{ensure_manifest_env, parse_manifest};

use crate::support::{TestDir, ocm_env, run_ocm, stderr};

#[test]
fn ensure_manifest_env_creates_a_missing_environment() {
    let root = TestDir::new("manifest-apply-create");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let manifest = parse_manifest("schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

    let summary = ensure_manifest_env(&manifest, &env, &cwd).unwrap();
    assert!(summary.created);
    assert_eq!(summary.env.name, "mira");

    let show = run_ocm(&cwd, &env, &["env", "show", "mira", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
}

#[test]
fn ensure_manifest_env_reuses_an_existing_environment() {
    let root = TestDir::new("manifest-apply-reuse");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let create = run_ocm(&cwd, &env, &["env", "create", "mira"]);
    assert!(create.status.success(), "{}", stderr(&create));
    let manifest = parse_manifest("schema: ocm/v1\nenv:\n  name: mira\n").unwrap();

    let summary = ensure_manifest_env(&manifest, &env, &cwd).unwrap();
    assert!(!summary.created);
    assert_eq!(summary.env.name, "mira");
}
