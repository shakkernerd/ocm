use crate::types::{RuntimeRelease, RuntimeReleaseManifest};

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
