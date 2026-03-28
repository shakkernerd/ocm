mod support;

use std::fs;

use serde_json::json;

use crate::support::{TestDir, TestHttpServer, ocm_env, run_ocm, stderr, stdout};

fn packument_body() -> Vec<u8> {
    json!({
        "dist-tags": {
            "latest": "2026.3.24",
            "beta": "2026.3.24-beta.2"
        },
        "versions": {
            "2026.3.24": {
                "version": "2026.3.24",
                "dist": {
                    "tarball": "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz",
                    "shasum": "abc123",
                    "integrity": "sha512-stable"
                }
            },
            "2026.3.24-beta.2": {
                "version": "2026.3.24-beta.2",
                "dist": {
                    "tarball": "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24-beta.2.tgz",
                    "shasum": "def456",
                    "integrity": "sha512-beta"
                }
            }
        },
        "time": {
            "2026.3.24": "2026-03-25T16:35:52.000Z",
            "2026.3.24-beta.2": "2026-03-25T14:11:48.000Z"
        }
    })
    .to_string()
    .into_bytes()
}

#[test]
fn release_list_uses_the_official_openclaw_source() {
    let root = TestDir::new("release-list-official");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let server = TestHttpServer::serve_bytes("/openclaw", "application/json", &packument_body());
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        server.url(),
    );

    let output = run_ocm(&cwd, &env, &["release", "list", "--raw"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let stdout = stdout(&output);
    assert!(stdout.contains("2026.3.24"));
    assert!(stdout.contains("channel=stable"));
    assert!(stdout.contains("2026.3.24-beta.2"));
    assert!(stdout.contains("channel=beta"));
}

#[test]
fn release_list_can_filter_by_channel_and_release_show_prints_one_version() {
    let root = TestDir::new("release-list-show");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let list_server = TestHttpServer::serve_bytes("/openclaw-list", "application/json", &packument_body());
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        list_server.url(),
    );

    let stable = run_ocm(&cwd, &env, &["release", "list", "--channel", "stable", "--json"]);
    assert!(stable.status.success(), "{}", stderr(&stable));
    let stable_stdout = stdout(&stable);
    assert!(stable_stdout.contains("\"version\": \"2026.3.24\""));
    assert!(!stable_stdout.contains("2026.3.24-beta.2"));

    let show_server = TestHttpServer::serve_bytes("/openclaw-show", "application/json", &packument_body());
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        show_server.url(),
    );
    let show = run_ocm(&cwd, &env, &["release", "show", "2026.3.24"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("version: 2026.3.24"));
    assert!(show_stdout.contains("channel: stable"));
    assert!(
        show_stdout.contains("tarballUrl: https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz")
    );
}

#[test]
fn release_list_rejects_conflicting_selectors() {
    let root = TestDir::new("release-list-conflicting");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let server = TestHttpServer::serve_bytes("/openclaw", "application/json", &packument_body());
    let mut env = ocm_env(&root);
    env.insert(
        "OCM_INTERNAL_OPENCLAW_RELEASES_URL".to_string(),
        server.url(),
    );

    let output = run_ocm(
        &cwd,
        &env,
        &[
            "release",
            "list",
            "--version",
            "2026.3.24",
            "--channel",
            "stable",
        ],
    );
    assert!(!output.status.success());
    assert!(stderr(&output).contains("release list accepts only one of --version or --channel"));
}
