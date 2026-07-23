use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::RuntimeService;
use crate::infra::download::{fetch_json, fetch_json_with_accept};
use crate::store::list_runtimes;

const DEFAULT_OPENCLAW_RELEASES_URL: &str = "https://registry.npmjs.org/openclaw";
const INTERNAL_OPENCLAW_RELEASES_URL_ENV: &str = "OCM_INTERNAL_OPENCLAW_RELEASES_URL";
const NPM_INSTALL_METADATA_ACCEPT: &str =
    "application/vnd.npm.install-v1+json; q=1.0, application/json; q=0.8, */*";

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
    #[serde(skip)]
    pub channels: Vec<String>,
    pub tarball_url: String,
    #[serde(default)]
    pub shasum: Option<String>,
    #[serde(default)]
    pub integrity: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub published_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawReleaseCatalogEntry {
    #[serde(flatten)]
    pub release: OpenClawRelease,
    #[serde(default)]
    pub installed_runtime_names: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ParsedOpenClawReleaseVersion {
    year: u64,
    month: u64,
    patch: u64,
    channel_rank: u8,
    sequence: u64,
}

fn parse_openclaw_release_version(value: &str) -> Option<ParsedOpenClawReleaseVersion> {
    let value = value.trim().strip_prefix('v').unwrap_or(value.trim());
    if value.contains('+') {
        return None;
    }

    let (base, suffix) = value
        .split_once('-')
        .map_or((value, None), |(base, suffix)| (base, Some(suffix)));
    let mut parts = base.split('.');
    let year_raw = parts.next()?;
    let month_raw = parts.next()?;
    let patch_raw = parts.next()?;
    if parts.next().is_some() || year_raw.len() != 4 {
        return None;
    }

    let year = year_raw.parse().ok()?;
    let month = month_raw.parse().ok()?;
    let patch = patch_raw.parse().ok()?;
    if !(1..=12).contains(&month) || patch == 0 {
        return None;
    }

    let (channel_rank, sequence) = match suffix {
        None => (2, 0),
        Some(suffix) => {
            if let Ok(correction) = suffix.parse::<u64>() {
                if correction == 0 {
                    return None;
                }
                (3, correction)
            } else {
                let (channel, sequence) = suffix.split_once('.')?;
                let sequence = sequence.parse::<u64>().ok()?;
                if sequence == 0 {
                    return None;
                }
                match channel {
                    "alpha" => (0, sequence),
                    "beta" => (1, sequence),
                    _ => return None,
                }
            }
        }
    };

    Some(ParsedOpenClawReleaseVersion {
        year,
        month,
        patch,
        channel_rank,
        sequence,
    })
}

pub fn compare_openclaw_release_versions(left: &str, right: &str) -> Option<Ordering> {
    Some(parse_openclaw_release_version(left)?.cmp(&parse_openclaw_release_version(right)?))
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

pub(crate) fn load_official_openclaw_release_selection(
    url: &str,
) -> Result<Vec<OpenClawRelease>, String> {
    let manifest: OpenClawPackageManifest =
        fetch_json_with_accept(url, NPM_INSTALL_METADATA_ACCEPT)?;
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
    let channel = normalize_openclaw_channel_selector(channel)?;
    if channel.is_empty() {
        return Err("OpenClaw release channel is required".to_string());
    }

    let mut release = releases
        .iter()
        .find(|release| {
            release.channel.as_deref() == Some(channel.as_str())
                || release.channels.iter().any(|value| value == &channel)
        })
        .cloned()
        .ok_or_else(|| format!("OpenClaw release channel \"{channel}\" was not found"))?;
    release.channel = Some(channel);
    Ok(release)
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
            Err("OpenClaw release selection accepts only one of --version or --channel".to_string())
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
        let url = official_openclaw_releases_url(self.env);
        let releases = if version.is_some() || channel.is_some() {
            load_official_openclaw_release_selection(&url)?
        } else {
            load_official_openclaw_releases(&url)?
        };
        query_official_openclaw_releases(&releases, version, channel)
    }

    pub fn official_openclaw_release_catalog(
        &self,
        version: Option<&str>,
        channel: Option<&str>,
    ) -> Result<Vec<OpenClawReleaseCatalogEntry>, String> {
        let releases = load_official_openclaw_releases(&official_openclaw_releases_url(self.env))?;
        let releases = query_official_openclaw_releases(&releases, version, channel)?;
        let runtimes = list_runtimes(self.env, self.cwd)?;

        Ok(releases
            .into_iter()
            .map(|release| {
                let mut installed_runtime_names = runtimes
                    .iter()
                    .filter(|runtime| {
                        runtime.release_version.as_deref() == Some(release.version.as_str())
                    })
                    .map(|runtime| runtime.name.clone())
                    .collect::<Vec<_>>();
                installed_runtime_names.sort();

                OpenClawReleaseCatalogEntry {
                    release,
                    installed_runtime_names,
                }
            })
            .collect())
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

    let mut channels_by_version = BTreeMap::<String, Vec<String>>::new();
    for (tag, version) in manifest.dist_tags {
        let Some(channel) = map_openclaw_dist_tag(&tag) else {
            continue;
        };
        channels_by_version
            .entry(version)
            .or_default()
            .push(channel.to_string());
    }
    for channels in channels_by_version.values_mut() {
        channels.sort_by_key(|channel| openclaw_channel_priority(channel));
        channels.dedup();
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

        let published_at = manifest.time.get(version_key.as_str()).and_then(|value| {
            OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
        });

        let channels = channels_by_version
            .get(version)
            .cloned()
            .unwrap_or_default();
        releases.push(OpenClawRelease {
            version: version.to_string(),
            channel: channels.first().cloned(),
            channels,
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

pub fn normalize_openclaw_channel_selector(channel: &str) -> Result<String, String> {
    let channel = channel.trim();
    if channel.is_empty() {
        return Err("OpenClaw release channel is required".to_string());
    }

    Ok(match channel {
        "latest" => "stable".to_string(),
        value => value.to_string(),
    })
}

fn openclaw_channel_priority(channel: &str) -> usize {
    match channel {
        "stable" => 0,
        "beta" => 1,
        "dev" => 2,
        _ => 3,
    }
}
