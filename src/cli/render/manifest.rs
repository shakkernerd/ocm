use std::collections::BTreeMap;

use serde::Serialize;

use crate::manifest::OcmManifest;

use super::{RenderProfile, format_key_value_lines};

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestPathSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ManifestShowSummary {
    pub found: bool,
    pub path: Option<String>,
    pub search_root: String,
    pub manifest: Option<OcmManifest>,
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

pub fn manifest_show(summary: &ManifestShowSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return manifest_show_pretty(summary);
    }
    manifest_show_raw(summary)
}

fn manifest_show_pretty(summary: &ManifestShowSummary) -> Vec<String> {
    let Some(manifest) = summary.manifest.as_ref() else {
        return vec![
            "Manifest".to_string(),
            String::new(),
            format!("No ocm.yaml found from {}", summary.search_root),
        ];
    };

    let runtime = manifest
        .runtime
        .as_ref()
        .and_then(|runtime| {
            runtime
                .name
                .as_deref()
                .or(runtime.version.as_deref())
                .or(runtime.channel.as_deref())
        })
        .unwrap_or("none");
    let launcher = manifest
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.name.as_deref())
        .unwrap_or("none");
    let service_install = manifest
        .service
        .as_ref()
        .and_then(|service| service.install)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "none".to_string());

    vec![
        "Manifest".to_string(),
        String::new(),
        format!("Path: {}", summary.path.as_deref().unwrap_or("none")),
        format!("Schema: {}", manifest.schema),
        format!("Env: {}", manifest.env.name),
        format!("Runtime: {runtime}"),
        format!("Launcher: {launcher}"),
        format!("Service install: {service_install}"),
    ]
}

fn manifest_show_raw(summary: &ManifestShowSummary) -> Vec<String> {
    let mut lines = BTreeMap::new();
    lines.insert("found".to_string(), summary.found.to_string());
    lines.insert(
        "path".to_string(),
        summary.path.clone().unwrap_or_else(|| "none".to_string()),
    );
    lines.insert("searchRoot".to_string(), summary.search_root.clone());
    if let Some(manifest) = summary.manifest.as_ref() {
        lines.insert("schema".to_string(), manifest.schema.clone());
        lines.insert("env".to_string(), manifest.env.name.clone());
        lines.insert(
            "runtime".to_string(),
            manifest
                .runtime
                .as_ref()
                .and_then(|runtime| {
                    runtime
                        .name
                        .as_deref()
                        .or(runtime.version.as_deref())
                        .or(runtime.channel.as_deref())
                })
                .unwrap_or("none")
                .to_string(),
        );
        lines.insert(
            "launcher".to_string(),
            manifest
                .launcher
                .as_ref()
                .and_then(|launcher| launcher.name.as_deref())
                .unwrap_or("none")
                .to_string(),
        );
        lines.insert(
            "serviceInstall".to_string(),
            manifest
                .service
                .as_ref()
                .and_then(|service| service.install)
                .map(|value| value.to_string())
                .unwrap_or_else(|| "none".to_string()),
        );
    }
    format_key_value_lines(lines)
}
