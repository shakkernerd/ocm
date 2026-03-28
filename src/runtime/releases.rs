use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::RuntimeService;
use crate::infra::download::fetch_json;

const DEFAULT_OPENCLAW_RELEASES_URL: &str = "https://registry.npmjs.org/openclaw";
const INTERNAL_OPENCLAW_RELEASES_URL_ENV: &str = "OCM_INTERNAL_OPENCLAW_RELEASES_URL";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReleaseManifest {
    #[serde(default)]
    pub kind: Option<String>,
    pub releases: Vec<RuntimeRelease>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRelease {
    pub version: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub url: String,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawRelease {
    pub version: String,
    #[serde(default)]
    pub channel: Option<String>,
    pub tarball_url: String,
    #[serde(default)]
    pub shasum: Option<String>,
    #[serde(default)]
    pub integrity: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Debug, Deserialize)]
struct OpenClawPackageManifest {
    #[serde(rename = "dist-tags", default)]
    dist_tags: BTreeMap<String, String>,
    #[serde(default)]
    versions: BTreeMap<String, OpenClawPackageVersion>,
    #[serde(default)]
    time: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct OpenClawPackageVersion {
    version: String,
    dist: OpenClawPackageDist,
}

#[derive(Debug, Deserialize)]
struct OpenClawPackageDist {
    tarball: String,
    #[serde(default)]
    shasum: Option<String>,
    #[serde(default)]
    integrity: Option<String>,
}

pub fn load_release_manifest(url: &str) -> Result<RuntimeReleaseManifest, String> {
    let manifest: RuntimeReleaseManifest = fetch_json(url)?;
    validate_release_manifest(manifest)
}

pub fn load_official_openclaw_releases(url: &str) -> Result<Vec<OpenClawRelease>, String> {
    let manifest: OpenClawPackageManifest = fetch_json(url)?;
    validate_official_openclaw_releases(manifest)
}

pub fn validate_release_manifest(
    manifest: RuntimeReleaseManifest,
) -> Result<RuntimeReleaseManifest, String> {
    if manifest.releases.is_empty() {
        return Err("runtime release manifest does not contain any releases".to_string());
    }

    let releases = manifest
        .releases
        .into_iter()
        .map(validate_release)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RuntimeReleaseManifest {
        kind: manifest.kind,
        releases,
    })
}

pub fn select_release_by_version(
    manifest: &RuntimeReleaseManifest,
    version: &str,
) -> Result<RuntimeRelease, String> {
    let version = version.trim();
    if version.is_empty() {
        return Err("runtime release version is required".to_string());
    }

    manifest
        .releases
        .iter()
        .find(|release| release.version == version)
        .cloned()
        .ok_or_else(|| format!("runtime release version \"{version}\" was not found"))
}

pub fn select_release_by_channel(
    manifest: &RuntimeReleaseManifest,
    channel: &str,
) -> Result<RuntimeRelease, String> {
    let channel = channel.trim();
    if channel.is_empty() {
        return Err("runtime release channel is required".to_string());
    }

    manifest
        .releases
        .iter()
        .find(|release| release.channel.as_deref() == Some(channel))
        .cloned()
        .ok_or_else(|| format!("runtime release channel \"{channel}\" was not found"))
}

pub fn select_release(
    manifest: &RuntimeReleaseManifest,
    version: Option<&str>,
    channel: Option<&str>,
) -> Result<RuntimeRelease, String> {
    let version = version.map(str::trim).filter(|value| !value.is_empty());
    let channel = channel.map(str::trim).filter(|value| !value.is_empty());
    match (version, channel) {
        (Some(version), None) => select_release_by_version(manifest, version),
        (None, Some(channel)) => select_release_by_channel(manifest, channel),
        (Some(_), Some(_)) => {
            Err("runtime release selection accepts only one of --version or --channel".to_string())
        }
        (None, None) => {
            Err("runtime release selection requires --version or --channel".to_string())
        }
    }
}

pub fn query_releases(
    manifest: &RuntimeReleaseManifest,
    version: Option<&str>,
    channel: Option<&str>,
) -> Result<Vec<RuntimeRelease>, String> {
    let version = version.map(str::trim).filter(|value| !value.is_empty());
    let channel = channel.map(str::trim).filter(|value| !value.is_empty());

    match (version, channel) {
        (None, None) => Ok(manifest.releases.clone()),
        (Some(version), None) => Ok(vec![select_release_by_version(manifest, version)?]),
        (None, Some(channel)) => Ok(vec![select_release_by_channel(manifest, channel)?]),
        (Some(_), Some(_)) => {
            Err("runtime release selection accepts only one of --version or --channel".to_string())
        }
    }
}

pub fn select_official_openclaw_release_by_version(
    releases: &[OpenClawRelease],
    version: &str,
) -> Result<OpenClawRelease, String> {
    let version = version.trim();
    if version.is_empty() {
        return Err("OpenClaw release version is required".to_string());
    }

    releases
        .iter()
        .find(|release| release.version == version)
        .cloned()
        .ok_or_else(|| format!("OpenClaw release version \"{version}\" was not found"))
}

pub fn select_official_openclaw_release_by_channel(
    releases: &[OpenClawRelease],
    channel: &str,
) -> Result<OpenClawRelease, String> {
    let channel = channel.trim();
    if channel.is_empty() {
        return Err("OpenClaw release channel is required".to_string());
    }

    releases
        .iter()
        .find(|release| release.channel.as_deref() == Some(channel))
        .cloned()
        .ok_or_else(|| format!("OpenClaw release channel \"{channel}\" was not found"))
}

pub fn query_official_openclaw_releases(
    releases: &[OpenClawRelease],
    version: Option<&str>,
    channel: Option<&str>,
) -> Result<Vec<OpenClawRelease>, String> {
    let version = version.map(str::trim).filter(|value| !value.is_empty());
    let channel = channel.map(str::trim).filter(|value| !value.is_empty());

    match (version, channel) {
        (None, None) => Ok(releases.to_vec()),
        (Some(version), None) => Ok(vec![select_official_openclaw_release_by_version(
            releases, version,
        )?]),
        (None, Some(channel)) => Ok(vec![select_official_openclaw_release_by_channel(
            releases, channel,
        )?]),
        (Some(_), Some(_)) => {
            Err("OpenClaw release selection accepts only one of --version or --channel"
                .to_string())
        }
    }
}

pub fn official_openclaw_releases_url(env: &BTreeMap<String, String>) -> String {
    env.get(INTERNAL_OPENCLAW_RELEASES_URL_ENV)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_OPENCLAW_RELEASES_URL)
        .to_string()
}

pub fn is_official_openclaw_releases_url(
    url: Option<&str>,
    env: &BTreeMap<String, String>,
) -> bool {
    let Some(url) = url.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let official = official_openclaw_releases_url(env);
    url == official || url == DEFAULT_OPENCLAW_RELEASES_URL
}

impl<'a> RuntimeService<'a> {
    pub fn releases_from_manifest(
        &self,
        url: &str,
        version: Option<&str>,
        channel: Option<&str>,
    ) -> Result<Vec<RuntimeRelease>, String> {
        let manifest = load_release_manifest(url)?;
        query_releases(&manifest, version, channel)
    }

    pub fn official_openclaw_releases(
        &self,
        version: Option<&str>,
        channel: Option<&str>,
    ) -> Result<Vec<OpenClawRelease>, String> {
        let releases = load_official_openclaw_releases(&official_openclaw_releases_url(self.env))?;
        query_official_openclaw_releases(&releases, version, channel)
    }
}

fn validate_release(mut release: RuntimeRelease) -> Result<RuntimeRelease, String> {
    let version = release.version.trim();
    if version.is_empty() {
        return Err("runtime release version is required".to_string());
    }

    let url = release.url.trim();
    if url.is_empty() {
        return Err(format!("runtime release \"{version}\" URL is required"));
    }

    release.version = version.to_string();
    release.url = url.to_string();
    release.channel = release
        .channel
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    release.sha256 = release
        .sha256
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    release.description = release
        .description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Ok(release)
}

fn validate_official_openclaw_releases(
    manifest: OpenClawPackageManifest,
) -> Result<Vec<OpenClawRelease>, String> {
    if manifest.versions.is_empty() {
        return Err("OpenClaw package source does not contain any published versions".to_string());
    }

    let mut channel_by_version = BTreeMap::<String, String>::new();
    for (tag, version) in manifest.dist_tags {
        let Some(channel) = map_openclaw_dist_tag(&tag) else {
            continue;
        };
        match channel_by_version.get(version.as_str()) {
            Some(existing) if openclaw_channel_priority(existing) <= openclaw_channel_priority(channel) => {}
            _ => {
                channel_by_version.insert(version, channel.to_string());
            }
        }
    }

    let mut releases = Vec::with_capacity(manifest.versions.len());
    for (version_key, version_meta) in manifest.versions {
        let version = version_meta.version.trim();
        if version.is_empty() {
            return Err("OpenClaw published version is required".to_string());
        }

        let tarball_url = version_meta.dist.tarball.trim();
        if tarball_url.is_empty() {
            return Err(format!(
                "OpenClaw release \"{version}\" tarball URL is required"
            ));
        }

        let published_at = manifest
            .time
            .get(version_key.as_str())
            .and_then(|value| OffsetDateTime::parse(
                value,
                &time::format_description::well_known::Rfc3339,
            ).ok());

        releases.push(OpenClawRelease {
            version: version.to_string(),
            channel: channel_by_version.get(version).cloned(),
            tarball_url: tarball_url.to_string(),
            shasum: version_meta
                .dist
                .shasum
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            integrity: version_meta
                .dist
                .integrity
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty()),
            published_at,
        });
    }

    releases.sort_by(|left, right| {
        right
            .published_at
            .cmp(&left.published_at)
            .then_with(|| right.version.cmp(&left.version))
    });
    Ok(releases)
}

fn map_openclaw_dist_tag(tag: &str) -> Option<&'static str> {
    match tag.trim() {
        "latest" => Some("stable"),
        "beta" => Some("beta"),
        "dev" => Some("dev"),
        _ => None,
    }
}

fn openclaw_channel_priority(channel: &str) -> usize {
    match channel {
        "stable" => 0,
        "beta" => 1,
        "dev" => 2,
        _ => 3,
    }
}
