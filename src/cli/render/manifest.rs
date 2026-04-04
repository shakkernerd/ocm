use std::collections::BTreeMap;

use serde::Serialize;

use super::{RenderProfile, format_key_value_lines};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestPathSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
}

pub fn manifest_path(summary: &ManifestPathSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_path_pretty(summary);
    }
    manifest_path_raw(summary)
}

fn manifest_path_pretty(summary: &ManifestPathSummary) -> Vec<String> {
    if let Some(path) = summary.path.as_deref() {
        vec!["Manifest path".to_string(), String::new(), path.to_string()]
    } else {
        vec![
            "Manifest path".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ]
    }
}

fn manifest_path_raw(summary: &ManifestPathSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    format_key_value_lines(lines)
}
