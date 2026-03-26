mod support;

use serde_json::json;

use ocm::runtime::releases::{
    load_release_manifest, query_releases, select_release, select_release_by_channel,
    select_release_by_version,
};

use crate::support::TestHttpServer;

#[test]
fn load_release_manifest_fetches_and_validates_remote_json() {
    let server = TestHttpServer::serve_bytes(
        "/manifests/stable.json",
        "application/json",
        json!({
            "kind": "ocm-runtime-manifest",
            "releases": [
                {
                    "version": "0.2.0",
                    "channel": "stable",
                    "url": "https://example.test/openclaw-0.2.0",
                    "sha256": "abc123",
                    "description": "stable release"
                }
            ]
        })
        .to_string()
        .as_bytes(),
    );

    let manifest = load_release_manifest(&server.url()).unwrap();
    assert_eq!(manifest.kind.as_deref(), Some("ocm-runtime-manifest"));
    assert_eq!(manifest.releases.len(), 1);
    assert_eq!(manifest.releases[0].version, "0.2.0");
    assert_eq!(manifest.releases[0].channel.as_deref(), Some("stable"));
}

#[test]
fn load_release_manifest_requires_release_entries() {
    let server = TestHttpServer::serve_bytes(
        "/manifests/empty.json",
        "application/json",
        json!({
            "kind": "ocm-runtime-manifest",
            "releases": []
        })
        .to_string()
        .as_bytes(),
    );

    let error = load_release_manifest(&server.url()).unwrap_err();
    assert!(error.contains("does not contain any releases"));
}

#[test]
fn release_selection_supports_version_and_channel_queries() {
    let server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        json!({
            "releases": [
                {
                    "version": "0.3.0-dev",
                    "channel": "nightly",
                    "url": "https://example.test/openclaw-nightly"
                },
                {
                    "version": "0.2.0",
                    "channel": "stable",
                    "url": "https://example.test/openclaw-stable"
                }
            ]
        })
        .to_string()
        .as_bytes(),
    );

    let manifest = load_release_manifest(&server.url()).unwrap();
    let stable = select_release_by_version(&manifest, "0.2.0").unwrap();
    assert_eq!(stable.channel.as_deref(), Some("stable"));

    let nightly = select_release_by_channel(&manifest, "nightly").unwrap();
    assert_eq!(nightly.version, "0.3.0-dev");

    let selected_by_version = select_release(&manifest, Some("0.2.0"), None).unwrap();
    assert_eq!(selected_by_version.channel.as_deref(), Some("stable"));

    let selected_by_channel = select_release(&manifest, None, Some("nightly")).unwrap();
    assert_eq!(selected_by_channel.version, "0.3.0-dev");
}

#[test]
fn release_selection_requires_exactly_one_selector() {
    let server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        json!({
            "releases": [
                {
                    "version": "0.2.0",
                    "channel": "stable",
                    "url": "https://example.test/openclaw-stable"
                }
            ]
        })
        .to_string()
        .as_bytes(),
    );

    let manifest = load_release_manifest(&server.url()).unwrap();
    let missing = select_release(&manifest, None, None).unwrap_err();
    assert!(missing.contains("requires --version or --channel"));

    let conflicting = select_release(&manifest, Some("0.2.0"), Some("stable")).unwrap_err();
    assert!(conflicting.contains("accepts only one of --version or --channel"));
}

#[test]
fn release_queries_support_listing_and_selector_scoped_views() {
    let server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        json!({
            "releases": [
                {
                    "version": "0.3.0-dev",
                    "channel": "nightly",
                    "url": "https://example.test/openclaw-nightly"
                },
                {
                    "version": "0.2.0",
                    "channel": "stable",
                    "url": "https://example.test/openclaw-stable"
                }
            ]
        })
        .to_string()
        .as_bytes(),
    );

    let manifest = load_release_manifest(&server.url()).unwrap();

    let all = query_releases(&manifest, None, None).unwrap();
    assert_eq!(all.len(), 2);

    let by_version = query_releases(&manifest, Some("0.2.0"), None).unwrap();
    assert_eq!(by_version.len(), 1);
    assert_eq!(by_version[0].channel.as_deref(), Some("stable"));

    let by_channel = query_releases(&manifest, None, Some("nightly")).unwrap();
    assert_eq!(by_channel.len(), 1);
    assert_eq!(by_channel[0].version, "0.3.0-dev");
}

#[test]
fn release_queries_reject_conflicting_selectors() {
    let server = TestHttpServer::serve_bytes(
        "/manifests/releases.json",
        "application/json",
        json!({
            "releases": [
                {
                    "version": "0.2.0",
                    "channel": "stable",
                    "url": "https://example.test/openclaw-stable"
                }
            ]
        })
        .to_string()
        .as_bytes(),
    );

    let manifest = load_release_manifest(&server.url()).unwrap();
    let error = query_releases(&manifest, Some("0.2.0"), Some("stable")).unwrap_err();
    assert!(error.contains("accepts only one of --version or --channel"));
}
