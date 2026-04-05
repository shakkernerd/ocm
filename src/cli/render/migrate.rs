use std::collections::BTreeMap;

use crate::migrate::MigrationSourceSummary;

use super::{RenderProfile, format_key_value_lines};

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
