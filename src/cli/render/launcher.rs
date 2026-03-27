use std::collections::BTreeMap;

use crate::launcher::LauncherMeta;

use super::{format_key_value_lines, format_rfc3339};

pub fn launcher_added(meta: &LauncherMeta) -> Vec<String> {
    let mut lines = vec![
        format!("Added launcher {}", meta.name),
        format!("  command: {}", meta.command),
    ];
    if let Some(cwd) = meta.cwd.as_deref() {
        lines.push(format!("  cwd: {cwd}"));
    }
    lines
}

pub fn launcher_list(launchers: &[LauncherMeta]) -> Vec<String> {
    if launchers.is_empty() {
        return vec!["No launchers.".to_string()];
    }
    let mut lines = Vec::with_capacity(launchers.len());
    for meta in launchers {
        let mut bits = vec![meta.name.clone(), meta.command.clone()];
        if let Some(cwd) = meta.cwd.as_deref() {
            bits.push(format!("cwd={cwd}"));
        }
        lines.push(bits.join("  "));
    }
    lines
}

pub fn launcher_show(meta: &LauncherMeta) -> Result<Vec<String>, String> {
    let mut lines = BTreeMap::new();
    lines.insert("kind".to_string(), meta.kind.clone());
    lines.insert("name".to_string(), meta.name.clone());
    lines.insert("command".to_string(), meta.command.clone());
    lines.insert("createdAt".to_string(), format_rfc3339(meta.created_at)?);
    lines.insert("updatedAt".to_string(), format_rfc3339(meta.updated_at)?);
    if let Some(cwd) = meta.cwd.as_deref() {
        lines.insert("cwd".to_string(), cwd.to_string());
    }
    if let Some(description) = meta.description.as_deref() {
        lines.insert("description".to_string(), description.to_string());
    }
    Ok(format_key_value_lines(lines))
}

pub fn launcher_removed(name: &str) -> Vec<String> {
    vec![format!("Removed launcher {name}")]
}
