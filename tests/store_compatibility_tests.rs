mod support;

use std::fs;

use ocm::store::{ensure_store, get_environment, get_version, now_utc, select_prune_candidates};
use ocm::types::EnvMeta;
use time::Duration;

use crate::support::{ocm_env, path_string, write_text, TestDir};

#[test]
fn get_environment_accepts_legacy_json_without_last_used_at() {
    let root = TestDir::new("legacy-env-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let stores = ensure_store(&env, &cwd).unwrap();

    write_text(
        &stores.envs_dir.join("legacy.json"),
        &format!(
            "{{\n  \"kind\": \"ocm-env\",\n  \"name\": \"legacy\",\n  \"root\": \"{}\",\n  \"gatewayPort\": 19789,\n  \"defaultVersion\": \"stable\",\n  \"protected\": false,\n  \"createdAt\": \"2026-03-20T10:00:00Z\",\n  \"updatedAt\": \"2026-03-20T10:00:00Z\"\n}}\n",
            path_string(&root.child("env-root"))
        ),
    );

    let meta = get_environment("legacy", &env, &cwd).unwrap();
    assert_eq!(meta.name, "legacy");
    assert_eq!(meta.gateway_port, Some(19789));
    assert_eq!(meta.default_version.as_deref(), Some("stable"));
    assert_eq!(meta.last_used_at, None);
}

#[test]
fn get_version_accepts_legacy_json_without_optional_fields() {
    let root = TestDir::new("legacy-version-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);
    let stores = ensure_store(&env, &cwd).unwrap();

    write_text(
        &stores.versions_dir.join("legacy.json"),
        "{\n  \"kind\": \"ocm-version\",\n  \"name\": \"legacy\",\n  \"command\": \"openclaw\",\n  \"createdAt\": \"2026-03-20T10:00:00Z\",\n  \"updatedAt\": \"2026-03-20T10:00:00Z\"\n}\n",
    );

    let meta = get_version("legacy", &env, &cwd).unwrap();
    assert_eq!(meta.name, "legacy");
    assert_eq!(meta.command, "openclaw");
    assert_eq!(meta.cwd, None);
    assert_eq!(meta.description, None);
}

#[test]
fn prune_selection_uses_last_used_at_and_skips_protected_envs() {
    let now = now_utc();
    let envs = vec![
        EnvMeta {
            kind: "ocm-env".to_string(),
            name: "old".to_string(),
            root: "/tmp/old".to_string(),
            gateway_port: None,
            default_version: None,
            protected: false,
            created_at: now - Duration::days(30),
            updated_at: now - Duration::days(30),
            last_used_at: None,
        },
        EnvMeta {
            kind: "ocm-env".to_string(),
            name: "recently-used".to_string(),
            root: "/tmp/recent".to_string(),
            gateway_port: None,
            default_version: None,
            protected: false,
            created_at: now - Duration::days(30),
            updated_at: now - Duration::days(30),
            last_used_at: Some(now - Duration::days(1)),
        },
        EnvMeta {
            kind: "ocm-env".to_string(),
            name: "protected-old".to_string(),
            root: "/tmp/protected".to_string(),
            gateway_port: None,
            default_version: None,
            protected: true,
            created_at: now - Duration::days(30),
            updated_at: now - Duration::days(30),
            last_used_at: None,
        },
    ];

    let candidates = select_prune_candidates(&envs, 14);
    let names = candidates
        .into_iter()
        .map(|meta| meta.name)
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["old".to_string()]);
}
