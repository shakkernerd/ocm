use std::collections::BTreeMap;

use crate::infra::terminal::{Cell, Tone, paint, render_table, terminal_width};
use crate::runtime::OpenClawReleaseCatalogEntry;

use super::{RenderProfile, format_key_value_lines, format_rfc3339};

pub fn release_list(
    releases: &[OpenClawReleaseCatalogEntry],
    profile: RenderProfile,
) -> Vec<String> {
    release_list_with_width(releases, profile, terminal_width())
}

fn release_list_with_width(
    releases: &[OpenClawReleaseCatalogEntry],
    profile: RenderProfile,
    width: Option<usize>,
) -> Vec<String> {
    if releases.is_empty() {
        return vec![paint(
            "No published OpenClaw releases.",
            Tone::Muted,
            profile.color,
        )];
    }
    if !profile.pretty {
        return release_list_raw(releases);
    }

    let show_full = width.map(|value| value >= 110).unwrap_or(true);
    let rows = releases
        .iter()
        .map(|release| {
            let published_at = release
                .release
                .published_at
                .map(|value| format_rfc3339(value))
                .transpose()
                .unwrap_or_else(|_| Some("—".to_string()))
                .unwrap_or_else(|| "—".to_string());
            let installed = if release.installed_runtime_names.is_empty() {
                "—".to_string()
            } else {
                release.installed_runtime_names.join(", ")
            };
            if show_full {
                vec![
                    Cell::accent(release.release.version.clone()),
                    optional_cell(release.release.channel.as_deref()),
                    Cell::muted(published_at),
                    Cell::plain(installed),
                    Cell::muted(release.release.tarball_url.clone()),
                ]
            } else {
                vec![
                    Cell::accent(release.release.version.clone()),
                    optional_cell(release.release.channel.as_deref()),
                    Cell::muted(published_at),
                    Cell::plain(installed),
                ]
            }
        })
        .collect::<Vec<_>>();

    let mut lines = render_table(
        if show_full {
            &["Version", "Channel", "Published", "Installed", "Tarball"]
        } else {
            &["Version", "Channel", "Published", "Installed"]
        },
        &rows,
        profile.color,
    );
    if !show_full {
        lines.push(String::new());
        lines.push(paint(
            "Use release show <version>, --raw, or --json for tarball details.",
            Tone::Muted,
            profile.color,
        ));
    }
    lines
}

fn release_list_raw(releases: &[OpenClawReleaseCatalogEntry]) -> Vec<String> {
    let mut lines = Vec::with_capacity(releases.len());
    for release in releases {
        let mut bits = vec![
            release.release.version.clone(),
            release.release.tarball_url.clone(),
        ];
        if let Some(channel) = release.release.channel.as_deref() {
            bits.push(format!("channel={channel}"));
        }
        if let Some(published_at) = release.release.published_at {
            if let Ok(published_at) = format_rfc3339(published_at) {
                bits.push(format!("publishedAt={published_at}"));
            }
        }
        if let Some(shasum) = release.release.shasum.as_deref() {
            bits.push(format!("shasum={shasum}"));
        }
        if let Some(integrity) = release.release.integrity.as_deref() {
            bits.push(format!("integrity={integrity}"));
        }
        if !release.installed_runtime_names.is_empty() {
            bits.push(format!(
                "installed={}",
                release.installed_runtime_names.join(",")
            ));
        }
        lines.push(bits.join("  "));
    }
    lines
}

fn optional_cell(value: Option<&str>) -> Cell {
    value.map(Cell::plain).unwrap_or_else(|| Cell::muted("—"))
}

pub fn release_show(release: &OpenClawReleaseCatalogEntry) -> Result<Vec<String>, String> {
    let mut lines = BTreeMap::new();
    lines.insert("version".to_string(), release.release.version.clone());
    if let Some(channel) = release.release.channel.as_deref() {
        lines.insert("channel".to_string(), channel.to_string());
    }
    lines.insert(
        "tarballUrl".to_string(),
        release.release.tarball_url.clone(),
    );
    if let Some(published_at) = release.release.published_at {
        lines.insert("publishedAt".to_string(), format_rfc3339(published_at)?);
    }
    if let Some(shasum) = release.release.shasum.as_deref() {
        lines.insert("shasum".to_string(), shasum.to_string());
    }
    if let Some(integrity) = release.release.integrity.as_deref() {
        lines.insert("integrity".to_string(), integrity.to_string());
    }
    if release.installed_runtime_names.is_empty() {
        lines.insert("installedRuntimes".to_string(), "—".to_string());
    } else {
        lines.insert(
            "installedRuntimes".to_string(),
            release.installed_runtime_names.join(", "),
        );
    }
    Ok(format_key_value_lines(lines))
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::{RenderProfile, release_list_with_width};
    use crate::runtime::{OpenClawRelease, OpenClawReleaseCatalogEntry};

    #[test]
    fn release_list_pretty_compacts_on_narrow_terminals() {
        let lines = release_list_with_width(
            &[sample_catalog_entry()],
            RenderProfile::pretty(false),
            Some(80),
        );

        assert!(lines[1].contains("Version"));
        assert!(lines[1].contains("Installed"));
        assert!(!lines[1].contains("Tarball"));
        assert_eq!(
            lines.last().unwrap(),
            "Use release show <version>, --raw, or --json for tarball details."
        );
    }

    fn sample_release() -> OpenClawRelease {
        OpenClawRelease {
            version: "2026.3.24".to_string(),
            channel: Some("stable".to_string()),
            tarball_url: "https://registry.npmjs.org/openclaw/-/openclaw-2026.3.24.tgz".to_string(),
            shasum: Some("abc123".to_string()),
            integrity: Some("sha512-demo".to_string()),
            published_at: Some(OffsetDateTime::UNIX_EPOCH),
        }
    }

    fn sample_catalog_entry() -> OpenClawReleaseCatalogEntry {
        OpenClawReleaseCatalogEntry {
            release: sample_release(),
            installed_runtime_names: vec!["stable".to_string()],
        }
    }
}
