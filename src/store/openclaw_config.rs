use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::env::EnvMeta;

use super::common::path_exists;
use super::layout::{EnvPaths, clean_path, derive_env_paths, display_path};

#[derive(Clone, Debug)]
pub(crate) struct OpenClawConfigAudit {
    pub status: String,
    pub issues: Vec<String>,
    pub repair_source_root: Option<PathBuf>,
    pub repair_workspace: bool,
    pub repair_gateway_port: bool,
}

pub(crate) fn audit_openclaw_config(meta: &EnvMeta, known_envs: &[EnvMeta]) -> OpenClawConfigAudit {
    let paths = derive_env_paths(Path::new(&meta.root));
    if !path_exists(&paths.config_path) {
        return OpenClawConfigAudit {
            status: "absent".to_string(),
            issues: Vec::new(),
            repair_source_root: None,
            repair_workspace: false,
            repair_gateway_port: false,
        };
    }

    let raw = match fs::read_to_string(&paths.config_path) {
        Ok(raw) => raw,
        Err(error) => {
            return OpenClawConfigAudit {
                status: "invalid".to_string(),
                issues: vec![format!(
                    "OpenClaw config is unreadable: {} ({error})",
                    display_path(&paths.config_path)
                )],
                repair_source_root: None,
                repair_workspace: false,
                repair_gateway_port: false,
            };
        }
    };

    let value: Value = match serde_json::from_str(&raw) {
        Ok(value) => value,
        Err(error) => {
            return OpenClawConfigAudit {
                status: "invalid".to_string(),
                issues: vec![format!(
                    "OpenClaw config is invalid JSON: {} ({error})",
                    display_path(&paths.config_path)
                )],
                repair_source_root: None,
                repair_workspace: false,
                repair_gateway_port: false,
            };
        }
    };

    audit_openclaw_config_value(meta, known_envs, &paths, &value)
}

pub(crate) fn repair_openclaw_config(
    meta: &EnvMeta,
    known_envs: &[EnvMeta],
) -> Result<bool, String> {
    let paths = derive_env_paths(Path::new(&meta.root));
    let Some(mut value) = read_config_value(&paths.config_path)? else {
        return Ok(false);
    };

    let audit = audit_openclaw_config_value(meta, known_envs, &paths, &value);
    if audit.status != "drifted" {
        return Ok(false);
    }

    let mut changed = false;
    if let Some(source_root) = audit.repair_source_root.as_deref() {
        changed |= rewrite_env_root_paths(&mut value, source_root, &paths.root);
    }
    if audit.repair_workspace && workspace_field_needs_repair(&value, &paths.root) {
        changed |= rewrite_workspace_field(&mut value, &paths.workspace_dir);
    }
    if audit.repair_gateway_port {
        changed |= rewrite_gateway_port(&mut value, meta.gateway_port.unwrap_or_default());
    }

    if !changed {
        return Ok(false);
    }

    write_config_value(&paths.config_path, &value)?;
    Ok(true)
}

pub(crate) fn rewrite_openclaw_config_for_target(
    target_paths: &EnvPaths,
    source_root: Option<&Path>,
    gateway_port: Option<u32>,
) -> Result<(), String> {
    let Some(mut value) = read_config_value(&target_paths.config_path)? else {
        return Ok(());
    };

    let mut changed = false;
    if let Some(source_root) = source_root {
        changed |= rewrite_env_root_paths(&mut value, source_root, &target_paths.root);
    }
    changed |= rewrite_workspace_field_if_env_scoped(
        &mut value,
        &target_paths.root,
        &target_paths.workspace_dir,
    );
    if let Some(gateway_port) = gateway_port {
        changed |= rewrite_gateway_port(&mut value, gateway_port);
    }

    if !changed {
        return Ok(());
    }

    write_config_value(&target_paths.config_path, &value)
}

fn audit_openclaw_config_value(
    meta: &EnvMeta,
    known_envs: &[EnvMeta],
    paths: &EnvPaths,
    value: &Value,
) -> OpenClawConfigAudit {
    let mut issues = Vec::new();
    let foreign_roots = collect_foreign_env_roots(meta, known_envs, value);
    let inferred_roots = collect_inferred_foreign_env_roots(&paths.root, value);
    let known_roots = foreign_roots
        .values()
        .map(|(root, _)| root.clone())
        .collect::<BTreeSet<_>>();
    for (env_name, (root, count)) in &foreign_roots {
        push_issue(
            &mut issues,
            format!(
                "OpenClaw config contains {count} path(s) under env \"{env_name}\" root: {}",
                display_path(root)
            ),
        );
    }
    for (root, count) in &inferred_roots {
        if known_roots.contains(root) {
            continue;
        }
        push_issue(
            &mut issues,
            format!(
                "OpenClaw config contains {count} env-scoped path(s) outside the current env root: {}",
                display_path(root)
            ),
        );
    }

    let mut repair_workspace = false;
    if let Some(workspace) = read_workspace_field(value) {
        let workspace_path = Path::new(workspace);
        if workspace_path.is_absolute()
            && !workspace_path.starts_with(&paths.root)
            && looks_env_scoped_workspace(workspace_path)
        {
            repair_workspace = true;
            if matching_foreign_env_root(meta, known_envs, workspace_path).is_none() {
                push_issue(
                    &mut issues,
                    format!(
                        "OpenClaw config workspace points outside env root: {}",
                        display_path(workspace_path)
                    ),
                );
            }
        }
    }

    let mut repair_gateway_port = false;
    if let Some(expected_port) = meta.gateway_port
        && let Some(actual_port) = read_gateway_port(value)
        && actual_port != expected_port
    {
        repair_gateway_port = true;
        push_issue(
            &mut issues,
            format!(
                "OpenClaw config gateway port {actual_port} does not match env metadata {expected_port}"
            ),
        );
    }

    let mut repair_roots = known_roots;
    repair_roots.extend(inferred_roots.keys().cloned());
    let repair_source_root = if repair_roots.len() == 1 {
        repair_roots.into_iter().next()
    } else {
        None
    };

    let status = if issues.is_empty() { "ok" } else { "drifted" }.to_string();
    OpenClawConfigAudit {
        status,
        issues,
        repair_source_root,
        repair_workspace,
        repair_gateway_port,
    }
}

fn read_config_value(config_path: &Path) -> Result<Option<Value>, String> {
    if !path_exists(config_path) {
        return Ok(None);
    }

    let raw = fs::read_to_string(config_path).map_err(|error| error.to_string())?;
    let value = serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    Ok(Some(value))
}

fn write_config_value(config_path: &Path, value: &Value) -> Result<(), String> {
    let mut rewritten = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    rewritten.push('\n');
    fs::write(config_path, rewritten).map_err(|error| error.to_string())
}

fn collect_foreign_env_roots(
    current: &EnvMeta,
    known_envs: &[EnvMeta],
    value: &Value,
) -> BTreeMap<String, (PathBuf, usize)> {
    let mut refs = BTreeMap::<String, (PathBuf, usize)>::new();
    collect_foreign_env_roots_inner(current, known_envs, value, &mut refs);
    refs
}

fn collect_inferred_foreign_env_roots(
    current_root: &Path,
    value: &Value,
) -> BTreeMap<PathBuf, usize> {
    let mut refs = BTreeMap::<PathBuf, usize>::new();
    collect_inferred_foreign_env_roots_inner(current_root, value, &mut refs);
    refs
}

fn collect_foreign_env_roots_inner(
    current: &EnvMeta,
    known_envs: &[EnvMeta],
    value: &Value,
    refs: &mut BTreeMap<String, (PathBuf, usize)>,
) {
    match value {
        Value::String(raw) => {
            let path = Path::new(raw);
            let Some((env_name, env_root)) = matching_foreign_env_root(current, known_envs, path)
            else {
                return;
            };
            let entry = refs
                .entry(env_name)
                .or_insert_with(|| (env_root.clone(), 0));
            entry.1 += 1;
        }
        Value::Array(values) => {
            for nested in values {
                collect_foreign_env_roots_inner(current, known_envs, nested, refs);
            }
        }
        Value::Object(values) => {
            for nested in values.values() {
                collect_foreign_env_roots_inner(current, known_envs, nested, refs);
            }
        }
        _ => {}
    }
}

fn collect_inferred_foreign_env_roots_inner(
    current_root: &Path,
    value: &Value,
    refs: &mut BTreeMap<PathBuf, usize>,
) {
    match value {
        Value::String(raw) => {
            let path = Path::new(raw);
            let Some(env_root) = inferred_foreign_env_root(current_root, path) else {
                return;
            };
            *refs.entry(env_root).or_insert(0) += 1;
        }
        Value::Array(values) => {
            for nested in values {
                collect_inferred_foreign_env_roots_inner(current_root, nested, refs);
            }
        }
        Value::Object(values) => {
            for nested in values.values() {
                collect_inferred_foreign_env_roots_inner(current_root, nested, refs);
            }
        }
        _ => {}
    }
}

fn matching_foreign_env_root(
    current: &EnvMeta,
    known_envs: &[EnvMeta],
    path: &Path,
) -> Option<(String, PathBuf)> {
    if !path.is_absolute() {
        return None;
    }

    for env in known_envs {
        if env.name == current.name {
            continue;
        }
        let env_root = clean_path(Path::new(&env.root));
        if path.starts_with(&env_root) {
            return Some((env.name.clone(), env_root));
        }
    }
    None
}

fn inferred_foreign_env_root(current_root: &Path, path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }

    let path = clean_path(path);
    if path.starts_with(current_root) {
        return None;
    }

    let mut prefix = PathBuf::new();
    for component in path.components() {
        if component.as_os_str() == ".openclaw" {
            if prefix.as_os_str().is_empty() {
                return None;
            }
            return Some(clean_path(&prefix));
        }
        prefix.push(component.as_os_str());
    }

    None
}

fn read_workspace_field(value: &Value) -> Option<&str> {
    value
        .get("agents")?
        .get("defaults")?
        .get("workspace")?
        .as_str()
}

fn read_gateway_port(value: &Value) -> Option<u32> {
    let port = value.get("gateway")?.get("port")?.as_u64()?;
    if (1..=u16::MAX as u64).contains(&port) {
        Some(port as u32)
    } else {
        None
    }
}

fn rewrite_env_root_paths(value: &mut Value, source_root: &Path, target_root: &Path) -> bool {
    match value {
        Value::String(raw) => rewrite_env_root_string(raw, source_root, target_root),
        Value::Array(values) => {
            let mut changed = false;
            for nested in values {
                changed |= rewrite_env_root_paths(nested, source_root, target_root);
            }
            changed
        }
        Value::Object(values) => {
            let mut changed = false;
            for nested in values.values_mut() {
                changed |= rewrite_env_root_paths(nested, source_root, target_root);
            }
            changed
        }
        _ => false,
    }
}

fn rewrite_env_root_string(raw: &mut String, source_root: &Path, target_root: &Path) -> bool {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return false;
    }

    let Ok(suffix) = path.strip_prefix(source_root) else {
        return false;
    };

    *raw = display_path(&clean_path(&target_root.join(suffix)));
    true
}

fn rewrite_workspace_field_if_env_scoped(
    value: &mut Value,
    target_root: &Path,
    target_workspace_dir: &Path,
) -> bool {
    let Some(workspace) = workspace_value_mut(value) else {
        return false;
    };
    let workspace_path = Path::new(workspace);
    if !workspace_path.is_absolute() || workspace_path.starts_with(target_root) {
        return false;
    }
    if !looks_env_scoped_workspace(workspace_path) {
        return false;
    }

    *workspace = display_path(target_workspace_dir);
    true
}

fn rewrite_workspace_field(value: &mut Value, target_workspace_dir: &Path) -> bool {
    let Some(workspace) = workspace_value_mut(value) else {
        return false;
    };
    let target = display_path(target_workspace_dir);
    if workspace == &target {
        return false;
    }
    *workspace = target;
    true
}

fn workspace_field_needs_repair(value: &Value, target_root: &Path) -> bool {
    let Some(workspace) = read_workspace_field(value) else {
        return false;
    };
    let workspace_path = Path::new(workspace);
    workspace_path.is_absolute()
        && !workspace_path.starts_with(target_root)
        && looks_env_scoped_workspace(workspace_path)
}

fn workspace_value_mut(value: &mut Value) -> Option<&mut String> {
    let agents = value.get_mut("agents")?.as_object_mut()?;
    let defaults = agents.get_mut("defaults")?.as_object_mut()?;
    match defaults.get_mut("workspace")? {
        Value::String(raw) => Some(raw),
        _ => None,
    }
}

fn rewrite_gateway_port(value: &mut Value, gateway_port: u32) -> bool {
    let Some(root) = value.as_object_mut() else {
        return false;
    };
    let Some(gateway) = root.get_mut("gateway").and_then(Value::as_object_mut) else {
        return false;
    };
    if !matches!(gateway.get("port"), Some(Value::Number(_))) {
        return false;
    }
    if gateway.get("port").and_then(Value::as_u64) == Some(gateway_port as u64) {
        return false;
    }

    gateway.insert("port".to_string(), Value::from(gateway_port));
    true
}

fn looks_env_scoped_workspace(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|window| matches!(window, [".openclaw", "workspace"]))
}

fn push_issue(issues: &mut Vec<String>, issue: String) {
    if !issues.iter().any(|current| current == &issue) {
        issues.push(issue);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::OffsetDateTime;

    use super::*;

    fn meta(name: &str, root: &str, gateway_port: Option<u32>) -> EnvMeta {
        EnvMeta {
            kind: "ocm-env".to_string(),
            name: name.to_string(),
            root: root.to_string(),
            gateway_port,
            service_enabled: true,
            service_running: true,
            default_runtime: None,
            default_launcher: None,
            protected: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_used_at: None,
        }
    }

    #[test]
    fn inferred_foreign_env_root_uses_the_openclaw_home_prefix() {
        let current_root = Path::new("/tmp/ocm/envs/target");
        let inferred = inferred_foreign_env_root(
            current_root,
            Path::new("/tmp/ocm/envs/source/.openclaw/workspace/notes.txt"),
        );
        assert_eq!(inferred, Some(PathBuf::from("/tmp/ocm/envs/source")));
    }

    #[test]
    fn audit_can_repair_one_inferred_foreign_env_root_without_metadata() {
        let current = meta("target", "/tmp/ocm/envs/target", Some(19790));
        let paths = derive_env_paths(Path::new(&current.root));
        let value = json!({
            "agents": {
                "defaults": {
                    "workspace": "/tmp/ocm/envs/source/.openclaw/workspace"
                }
            },
            "memory": {
                "logPath": "/tmp/ocm/envs/source/.openclaw/logs/gateway.log"
            },
            "gateway": {
                "port": 19789
            }
        });

        let audit = audit_openclaw_config_value(&current, &[], &paths, &value);
        assert_eq!(audit.status, "drifted");
        assert_eq!(
            audit.repair_source_root,
            Some(PathBuf::from("/tmp/ocm/envs/source"))
        );
        assert!(audit.repair_workspace);
        assert!(audit.repair_gateway_port);
        assert!(audit.issues.iter().any(|issue| issue.contains(
            "OpenClaw config contains 2 env-scoped path(s) outside the current env root"
        )));
    }
}
