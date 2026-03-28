mod support;

use serde_json::json;

use ocm::runtime::releases::{
    load_official_openclaw_releases, load_release_manifest, query_official_openclaw_releases,
    query_releases, select_official_openclaw_release_by_channel,
    select_official_openclaw_release_by_version, select_release, select_release_by_channel,
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

#[test]
fn official_openclaw_releases_load_from_published_package_metadata() {
    let server = TestHttpServer::serve_bytes(
        "/openclaw",
        "application/json",
        json!({
            "dist-tags": {
                "latest": "2026.3.24",
                "beta": "2026.3.24-beta.2",
                "dev": "2026.3.27-dev.1"
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
                },
                "2026.3.27-dev.1": {
                    "version": "2026.3.27-dev.1",
                    "dist": {
                        "tarball": "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.27-dev.1.tgz"
                    }
                }
            },
            "time": {
                "2026.3.24": "2026-03-25T16:35:52.000Z",
                "2026.3.24-beta.2": "2026-03-25T14:11:48.000Z",
                "2026.3.27-dev.1": "2026-03-27T09:00:00.000Z"
            }
        })
        .to_string()
        .as_bytes(),
    );

    let releases = load_official_openclaw_releases(&server.url()).unwrap();
    assert_eq!(releases.len(), 3);
    assert_eq!(releases[0].version, "2026.3.27-dev.1");
    assert_eq!(releases[0].channel.as_deref(), Some("dev"));
    assert_eq!(releases[1].version, "2026.3.24");
    assert_eq!(releases[1].channel.as_deref(), Some("stable"));
    assert_eq!(
        releases[1].tarball_url,
        "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz"
    );
    assert_eq!(releases[1].integrity.as_deref(), Some("sha512-stable"));
    assert_eq!(releases[2].channel.as_deref(), Some("beta"));
}

#[test]
fn official_openclaw_release_queries_support_version_and_channel() {
    let server = TestHttpServer::serve_bytes(
        "/openclaw",
        "application/json",
        json!({
            "dist-tags": {
                "latest": "2026.3.24",
                "beta": "2026.3.24-beta.2"
            },
            "versions": {
                "2026.3.24": {
                    "version": "2026.3.24",
                    "dist": {
                        "tarball": "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz"
                    }
                },
                "2026.3.24-beta.2": {
                    "version": "2026.3.24-beta.2",
                    "dist": {
                        "tarball": "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24-beta.2.tgz"
                    }
                }
            }
        })
        .to_string()
        .as_bytes(),
    );

    let releases = load_official_openclaw_releases(&server.url()).unwrap();

    let stable = select_official_openclaw_release_by_version(&releases, "2026.3.24").unwrap();
    assert_eq!(stable.channel.as_deref(), Some("stable"));

    let beta = select_official_openclaw_release_by_channel(&releases, "beta").unwrap();
    assert_eq!(beta.version, "2026.3.24-beta.2");

    let latest = select_official_openclaw_release_by_channel(&releases, "latest").unwrap();
    assert_eq!(latest.version, "2026.3.24");

    let all = query_official_openclaw_releases(&releases, None, None).unwrap();
    assert_eq!(all.len(), 2);

    let by_channel = query_official_openclaw_releases(&releases, None, Some("stable")).unwrap();
    assert_eq!(by_channel.len(), 1);
    assert_eq!(by_channel[0].version, "2026.3.24");

    let by_version =
        query_official_openclaw_releases(&releases, Some("2026.3.24-beta.2"), None).unwrap();
    assert_eq!(by_version.len(), 1);
    assert_eq!(by_version[0].channel.as_deref(), Some("beta"));
}
