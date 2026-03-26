use super::RuntimeService;
use crate::infra::download::fetch_json;
use crate::types::{RuntimeRelease, RuntimeReleaseManifest};

pub fn load_release_manifest(url: &str) -> Result<RuntimeReleaseManifest, String> {
    let manifest: RuntimeReleaseManifest = fetch_json(url)?;
    validate_release_manifest(manifest)
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
