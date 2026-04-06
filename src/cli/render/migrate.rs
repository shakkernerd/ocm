use std::collections::BTreeMap;

use crate::migrate::{MigrationImportSummary, MigrationPlanSummary, MigrationSourceSummary};

use super::{RenderProfile, format_key_value_lines};
use crate::cli::render::env;

pub fn migration_source(summary: &MigrationSourceSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        return vec![
            "Migration source".to_string(),
            String::new(),
            format!("Source home: {}", summary.source_home),
            format!("Exists: {}", summary.exists),
            format!("Config path: {}", summary.config_path),
            format!("Config exists: {}", summary.config_exists),
            format!("Workspace dir: {}", summary.workspace_dir),
            format!("Workspace exists: {}", summary.workspace_exists),
        ];
    }

    let mut lines = BTreeMap::new();
    lines.insert("sourceHome".to_string(), summary.source_home.clone());
    lines.insert("exists".to_string(), summary.exists.to_string());
    lines.insert("configPath".to_string(), summary.config_path.clone());
    lines.insert(
        "configExists".to_string(),
        summary.config_exists.to_string(),
    );
    lines.insert("workspaceDir".to_string(), summary.workspace_dir.clone());
    lines.insert(
        "workspaceExists".to_string(),
        summary.workspace_exists.to_string(),
    );
    format_key_value_lines(lines)
}

pub fn migration_plan(summary: &MigrationPlanSummary, profile: RenderProfile) -> Vec<String> {
    if profile.pretty {
        let mut lines = vec![
            "Migration plan".to_string(),
            String::new(),
            format!("Source home: {}", summary.source.source_home),
            format!("Source exists: {}", summary.source.exists),
            format!("Target env: {}", summary.env_name),
            format!("Target exists: {}", summary.env_exists),
            format!("Target root: {}", summary.target_root),
        ];
        if let Some(path) = summary.manifest_path.as_deref() {
            lines.push(format!("Manifest path: {path}"));
        }
        if let Some(preview) = summary.manifest_preview.as_deref() {
            lines.push("Manifest preview:".to_string());
            lines.extend(preview.lines().map(|line| format!("  {line}")));
        }
        return lines;
    }

    let mut lines = BTreeMap::new();
    lines.insert("sourceHome".to_string(), summary.source.source_home.clone());
    lines.insert(
        "sourceExists".to_string(),
        summary.source.exists.to_string(),
    );
    lines.insert("env".to_string(), summary.env_name.clone());
    lines.insert("envExists".to_string(), summary.env_exists.to_string());
    lines.insert("targetRoot".to_string(), summary.target_root.clone());
    if let Some(path) = summary.manifest_path.as_deref() {
        lines.insert("manifestPath".to_string(), path.to_string());
    }
    if let Some(preview) = summary.manifest_preview.as_deref() {
        lines.insert("manifestPreview".to_string(), preview.to_string());
    }
    format_key_value_lines(lines)
}

pub fn migration_import(
    summary: &MigrationImportSummary,
    command_example: &str,
    profile: RenderProfile,
) -> Vec<String> {
    let mut lines = env::env_imported(&summary.import, command_example, profile);
    if profile.pretty {
        if let Some(path) = summary.manifest_path.as_deref() {
            lines.push(String::new());
            lines.push(format!("Manifest: {path}"));
        }
        return lines;
    }

    if let Some(path) = summary.manifest_path.as_deref() {
        lines.push(format!("  manifest: {path}"));
    }
    lines
}
