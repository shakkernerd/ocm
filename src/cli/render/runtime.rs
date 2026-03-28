use std::collections::BTreeMap;

use crate::infra::terminal::{Cell, Tone, paint, render_table, terminal_width};
use crate::runtime::{
    RuntimeBinarySummary, RuntimeMeta, RuntimeRelease, RuntimeUpdateBatchSummary,
    RuntimeVerifySummary,
};

use super::{RenderProfile, format_key_value_lines, format_rfc3339};

pub fn runtime_added(meta: &RuntimeMeta) -> Vec<String> {
    vec![
        format!("Added runtime {}", meta.name),
        format!("  binary path: {}", meta.binary_path),
    ]
}

pub fn runtime_list(runtimes: &[RuntimeMeta], profile: RenderProfile) -> Vec<String> {
    runtime_list_with_width(runtimes, profile, terminal_width())
}

fn runtime_list_with_width(
    runtimes: &[RuntimeMeta],
    profile: RenderProfile,
    width: Option<usize>,
) -> Vec<String> {
    if runtimes.is_empty() {
        return vec!["No runtimes.".to_string()];
    }
    if !profile.pretty {
        return runtime_list_raw(runtimes);
    }

    let show_full = width.map(|width| width >= 110).unwrap_or(true);
    let rows = runtimes
        .iter()
        .map(|meta| {
            if show_full {
                vec![
                    Cell::accent(meta.name.clone()),
                    Cell::plain(meta.source_kind.as_str()),
                    optional_cell(meta.release_version.as_deref()),
                    optional_cell(meta.release_channel.as_deref()),
                    Cell::muted(meta.binary_path.clone()),
                ]
            } else {
                vec![
                    Cell::accent(meta.name.clone()),
                    Cell::plain(meta.source_kind.as_str()),
                    Cell::muted(runtime_target(meta)),
                ]
            }
        })
        .collect::<Vec<_>>();
    let mut lines = render_table(
        if show_full {
            &["Name", "Source", "Release", "Channel", "Binary"]
        } else {
            &["Name", "Source", "Target"]
        },
        &rows,
        profile.color,
    );
    if !show_full {
        lines.push(String::new());
        lines.push(paint(
            "Use --raw for full runtime path and release details.",
            Tone::Muted,
            profile.color,
        ));
    }
    lines
}

fn runtime_list_raw(runtimes: &[RuntimeMeta]) -> Vec<String> {
    let mut lines = Vec::with_capacity(runtimes.len());
    for meta in runtimes {
        let mut bits = vec![
            meta.name.clone(),
            meta.binary_path.clone(),
            format!("source={}", meta.source_kind.as_str()),
        ];
        if let Some(release_version) = meta.release_version.as_deref() {
            bits.push(format!("release={release_version}"));
        }
        if let Some(release_channel) = meta.release_channel.as_deref() {
            bits.push(format!("channel={release_channel}"));
        }
        lines.push(bits.join("  "));
    }
    lines
}

fn optional_cell(value: Option<&str>) -> Cell {
    value.map(Cell::plain).unwrap_or_else(|| Cell::muted("—"))
}

fn runtime_target(meta: &RuntimeMeta) -> String {
    match (
        meta.release_version.as_deref(),
        meta.release_channel.as_deref(),
    ) {
        (Some(version), Some(channel)) => format!("{version} ({channel})"),
        (Some(version), None) => version.to_string(),
        (None, Some(channel)) => format!("channel:{channel}"),
        (None, None) => meta.binary_path.clone(),
    }
}

pub fn runtime_show(meta: &RuntimeMeta) -> Result<Vec<String>, String> {
    let mut lines = BTreeMap::new();
    lines.insert("kind".to_string(), meta.kind.clone());
    lines.insert("name".to_string(), meta.name.clone());
    lines.insert("binaryPath".to_string(), meta.binary_path.clone());
    lines.insert(
        "sourceKind".to_string(),
        meta.source_kind.as_str().to_string(),
    );
    lines.insert("createdAt".to_string(), format_rfc3339(meta.created_at)?);
    lines.insert("updatedAt".to_string(), format_rfc3339(meta.updated_at)?);
    if let Some(description) = meta.description.as_deref() {
        lines.insert("description".to_string(), description.to_string());
    }
    if let Some(source_path) = meta.source_path.as_deref() {
        lines.insert("sourcePath".to_string(), source_path.to_string());
    }
    if let Some(source_url) = meta.source_url.as_deref() {
        lines.insert("sourceUrl".to_string(), source_url.to_string());
    }
    if let Some(source_manifest_url) = meta.source_manifest_url.as_deref() {
        lines.insert(
            "sourceManifestUrl".to_string(),
            source_manifest_url.to_string(),
        );
    }
    if let Some(source_sha256) = meta.source_sha256.as_deref() {
        lines.insert("sourceSha256".to_string(), source_sha256.to_string());
    }
    if let Some(release_version) = meta.release_version.as_deref() {
        lines.insert("releaseVersion".to_string(), release_version.to_string());
    }
    if let Some(release_channel) = meta.release_channel.as_deref() {
        lines.insert("releaseChannel".to_string(), release_channel.to_string());
    }
    if let Some(release_selector_kind) = meta.release_selector_kind.as_ref() {
        lines.insert(
            "releaseSelectorKind".to_string(),
            release_selector_kind.as_str().to_string(),
        );
    }
    if let Some(release_selector_value) = meta.release_selector_value.as_deref() {
        lines.insert(
            "releaseSelectorValue".to_string(),
            release_selector_value.to_string(),
        );
    }
    if let Some(install_root) = meta.install_root.as_deref() {
        lines.insert("installRoot".to_string(), install_root.to_string());
    }
    Ok(format_key_value_lines(lines))
}

pub fn runtime_which(summary: &RuntimeBinarySummary) -> Vec<String> {
    vec![summary.binary_path.clone()]
}

pub fn runtime_removed(name: &str) -> Vec<String> {
    vec![format!("Removed runtime {name}")]
}

pub fn runtime_installed(meta: &RuntimeMeta) -> Vec<String> {
    let mut lines = vec![
        format!("Installed runtime {}", meta.name),
        format!("  binary path: {}", meta.binary_path),
    ];
    if let Some(install_root) = meta.install_root.as_deref() {
        lines.push(format!("  install root: {install_root}"));
    }
    lines
}

pub fn runtime_releases(releases: &[RuntimeRelease]) -> Vec<String> {
    if releases.is_empty() {
        return vec!["No runtime releases.".to_string()];
    }
    let mut lines = Vec::with_capacity(releases.len());
    for release in releases {
        let mut bits = vec![release.version.clone(), release.url.clone()];
        if let Some(channel) = release.channel.as_deref() {
            bits.push(format!("channel={channel}"));
        }
        if let Some(sha256) = release.sha256.as_deref() {
            bits.push(format!("sha256={sha256}"));
        }
        lines.push(bits.join("  "));
    }
    lines
}

pub fn runtime_update_batch(batch: &RuntimeUpdateBatchSummary) -> Vec<String> {
    if batch.results.is_empty() {
        return vec!["No runtimes.".to_string()];
    }
    let mut lines = vec![format!(
        "Runtime update summary: total={} updated={} skipped={} failed={}",
        batch.count, batch.updated, batch.skipped, batch.failed
    )];
    for summary in &batch.results {
        let mut bits = vec![
            summary.name.clone(),
            format!("outcome={}", summary.outcome),
            format!("source={}", summary.source_kind),
        ];
        if let Some(binary_path) = summary.binary_path.as_deref() {
            bits.push(binary_path.to_string());
        }
        if let Some(release_version) = summary.release_version.as_deref() {
            bits.push(format!("release={release_version}"));
        }
        if let Some(release_channel) = summary.release_channel.as_deref() {
            bits.push(format!("channel={release_channel}"));
        }
        if let Some(issue) = summary.issue.as_deref() {
            bits.push(format!("issue={issue}"));
        }
        lines.push(bits.join("  "));
    }
    lines
}

pub fn runtime_updated(meta: &RuntimeMeta) -> Vec<String> {
    let mut lines = vec![
        format!("Updated runtime {}", meta.name),
        format!("  binary path: {}", meta.binary_path),
    ];
    if let Some(install_root) = meta.install_root.as_deref() {
        lines.push(format!("  install root: {install_root}"));
    }
    lines
}

pub fn runtime_verify_all(summaries: &[RuntimeVerifySummary]) -> Vec<String> {
    if summaries.is_empty() {
        return vec!["No runtimes.".to_string()];
    }
    let mut lines = Vec::with_capacity(summaries.len());
    for summary in summaries {
        let mut bits = vec![
            summary.name.clone(),
            summary.binary_path.clone(),
            format!("source={}", summary.source_kind),
            format!("healthy={}", summary.healthy),
        ];
        if let Some(issue) = summary.issue.as_deref() {
            bits.push(format!("issue={issue}"));
        }
        lines.push(bits.join("  "));
    }
    lines
}

pub fn runtime_verify(summary: &RuntimeVerifySummary) -> Vec<String> {
    let mut lines = vec![
        format!("name: {}", summary.name),
        format!("binaryPath: {}", summary.binary_path),
        format!("sourceKind: {}", summary.source_kind),
        format!("healthy: {}", summary.healthy),
    ];
    if let Some(source_path) = summary.source_path.as_deref() {
        lines.push(format!("sourcePath: {source_path}"));
    }
    if let Some(source_url) = summary.source_url.as_deref() {
        lines.push(format!("sourceUrl: {source_url}"));
    }
    if let Some(source_manifest_url) = summary.source_manifest_url.as_deref() {
        lines.push(format!("sourceManifestUrl: {source_manifest_url}"));
    }
    if let Some(source_sha256) = summary.source_sha256.as_deref() {
        lines.push(format!("sourceSha256: {source_sha256}"));
    }
    if let Some(release_version) = summary.release_version.as_deref() {
        lines.push(format!("releaseVersion: {release_version}"));
    }
    if let Some(release_channel) = summary.release_channel.as_deref() {
        lines.push(format!("releaseChannel: {release_channel}"));
    }
    if let Some(install_root) = summary.install_root.as_deref() {
        lines.push(format!("installRoot: {install_root}"));
    }
    if let Some(issue) = summary.issue.as_deref() {
        lines.push(format!("issue: {issue}"));
    }
    lines
}

#[cfg(test)]
mod tests {
    use time::OffsetDateTime;

    use super::{RenderProfile, runtime_list_with_width};
    use crate::runtime::{RuntimeMeta, RuntimeSourceKind};

    #[test]
    fn runtime_list_pretty_compacts_on_narrow_terminals() {
        let lines =
            runtime_list_with_width(&[sample_runtime()], RenderProfile::pretty(false), Some(80));

        assert!(lines[1].contains("Target"));
        assert!(!lines[1].contains("Channel"));
        assert!(!lines[1].contains("Binary"));
        assert!(lines.iter().any(|line| line.contains("2026.3.24 (stable)")));
        assert_eq!(
            lines.last().unwrap(),
            "Use --raw for full runtime path and release details."
        );
    }

    #[test]
    fn runtime_list_pretty_keeps_all_columns_on_wide_terminals() {
        let lines =
            runtime_list_with_width(&[sample_runtime()], RenderProfile::pretty(false), Some(140));

        assert!(lines[1].contains("Channel"));
        assert!(lines[1].contains("Binary"));
        assert!(lines.iter().any(|line| line.contains("/tmp/openclaw")));
    }

    fn sample_runtime() -> RuntimeMeta {
        RuntimeMeta {
            kind: "ocm-runtime".to_string(),
            name: "stable".to_string(),
            binary_path: "/tmp/openclaw".to_string(),
            source_kind: RuntimeSourceKind::Installed,
            source_path: None,
            source_url: None,
            source_manifest_url: None,
            source_sha256: None,
            release_version: Some("2026.3.24".to_string()),
            release_channel: Some("stable".to_string()),
            release_selector_kind: None,
            release_selector_value: None,
            install_root: None,
            description: None,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        }
    }
}
