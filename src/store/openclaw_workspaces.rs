use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::common::path_exists;
use super::layout::{EnvPaths, clean_path, display_path};

const DEFAULT_AGENT_ID: &str = "main";
const MAX_INCLUDE_DEPTH: usize = 10;
const MAX_INCLUDE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_INCLUDE_PATH_LENGTH: usize = 4096;

#[derive(Clone, Debug)]
pub(crate) struct EffectiveOpenClawConfig {
    pub(crate) value: Value,
    pub(crate) include_paths: BTreeSet<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OpenClawWorkspaceInventory {
    workspace_roots: BTreeSet<PathBuf>,
    config_include_paths: BTreeSet<PathBuf>,
}

impl OpenClawWorkspaceInventory {
    pub(crate) fn contains(&self, path: &Path) -> bool {
        let path = clean_path(path);
        self.workspace_roots
            .iter()
            .any(|root| path == *root || path.starts_with(root))
            || self.config_include_paths.contains(&path)
    }

    pub(crate) fn has_descendant(&self, path: &Path) -> bool {
        let path = clean_path(path);
        self.workspace_roots
            .iter()
            .chain(self.config_include_paths.iter())
            .any(|root| root.starts_with(&path))
    }

    pub(crate) fn archive_relative_roots(
        &self,
        archive_root: &Path,
    ) -> Result<BTreeSet<PathBuf>, String> {
        let archive_root = clean_path(archive_root);
        let canonical_archive_root = canonicalize_path_allow_missing(&archive_root)?;
        let mut relative_roots = BTreeSet::new();

        for preserved_path in self
            .workspace_roots
            .iter()
            .chain(self.config_include_paths.iter())
        {
            let canonical_preserved_path = canonicalize_path_allow_missing(preserved_path)?;
            if canonical_preserved_path == canonical_archive_root
                || !canonical_preserved_path.starts_with(&canonical_archive_root)
            {
                return Err(format!(
                    "cannot safely preserve configured OpenClaw workspace or config include that resolves outside the environment root: {} (resolved: {}; environment root resolves to: {})",
                    display_path(preserved_path),
                    display_path(&canonical_preserved_path),
                    display_path(&canonical_archive_root)
                ));
            }

            let canonical_relative = canonical_preserved_path
                .strip_prefix(&canonical_archive_root)
                .map_err(|error| error.to_string())?;
            if let Ok(relative) = preserved_path.strip_prefix(&archive_root) {
                if relative != canonical_relative {
                    return Err(format!(
                        "cannot safely preserve configured OpenClaw workspace or config include through a symlink: {}",
                        display_path(preserved_path)
                    ));
                }
                relative_roots.insert(relative.to_path_buf());
            }
            relative_roots.insert(canonical_relative.to_path_buf());
        }

        Ok(relative_roots)
    }
}

pub(crate) fn resolve_env_openclaw_workspaces(
    paths: &EnvPaths,
) -> Result<OpenClawWorkspaceInventory, String> {
    let mut inventory =
        resolve_openclaw_workspaces(&paths.config_path, &paths.state_dir, &paths.openclaw_home)?;
    inventory
        .workspace_roots
        .insert(paths.workspace_dir.clone());
    Ok(inventory)
}

pub(crate) fn resolve_plain_openclaw_workspaces(
    state_dir: &Path,
) -> Result<OpenClawWorkspaceInventory, String> {
    let state_dir = canonicalize_path_allow_missing(state_dir)?;
    let openclaw_home = state_dir.parent().unwrap_or(&state_dir);
    let mut inventory =
        resolve_openclaw_workspaces(&state_dir.join("openclaw.json"), &state_dir, openclaw_home)?;
    inventory
        .workspace_roots
        .insert(state_dir.join("workspace"));
    Ok(inventory)
}

pub(crate) fn load_effective_openclaw_config(
    config_path: &Path,
) -> Result<Option<EffectiveOpenClawConfig>, String> {
    if !path_exists(config_path) {
        return Ok(None);
    }

    let raw = fs::read_to_string(config_path).map_err(|error| {
        format!(
            "failed to read OpenClaw config {}: {error}",
            display_path(config_path)
        )
    })?;
    let value = parse_json5(&raw, config_path, "OpenClaw config")?;
    let config_root = config_path.parent().ok_or_else(|| {
        format!(
            "OpenClaw config path has no parent: {}",
            config_path.display()
        )
    })?;
    let mut processor = IncludeProcessor::new(config_root)?;
    let value = processor.process(value, config_path, 0, &mut vec![clean_path(config_path)])?;

    Ok(Some(EffectiveOpenClawConfig {
        value,
        include_paths: processor.include_paths,
    }))
}

fn resolve_openclaw_workspaces(
    config_path: &Path,
    state_dir: &Path,
    openclaw_home: &Path,
) -> Result<OpenClawWorkspaceInventory, String> {
    let resolved = load_effective_openclaw_config(config_path)?;
    let config = resolved
        .as_ref()
        .map(|resolved| resolved.value.clone())
        .unwrap_or_else(|| Value::Object(Map::new()));
    let config_include_paths = resolved
        .map(|resolved| resolved.include_paths)
        .unwrap_or_default();
    let entries = agent_entries(&config);
    let default_agent_id = resolve_default_agent_id(&entries);
    let default_workspace = config
        .pointer("/agents/defaults/workspace")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut agent_ids = entries
        .iter()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str))
        .map(normalize_agent_id)
        .collect::<BTreeSet<_>>();
    agent_ids.insert(default_agent_id.clone());

    let mut workspace_roots = BTreeSet::new();
    for agent_id in agent_ids {
        let explicit = entries
            .iter()
            .find(|entry| {
                entry
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| normalize_agent_id(id) == agent_id)
            })
            .and_then(|entry| entry.get("workspace"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let raw = if let Some(explicit) = explicit {
            explicit.to_string()
        } else if agent_id == default_agent_id {
            default_workspace
                .map(str::to_string)
                .unwrap_or_else(|| display_path(&state_dir.join("workspace")))
        } else if let Some(default_workspace) = default_workspace {
            display_path(&resolve_workspace_path(default_workspace, openclaw_home)?.join(&agent_id))
        } else {
            display_path(&state_dir.join(format!("workspace-{agent_id}")))
        };
        let workspace = resolve_workspace_path(&raw, openclaw_home)?;
        if workspace == clean_path(state_dir) {
            return Err(format!(
                "configured OpenClaw workspace cannot be the state directory itself: {}",
                display_path(&workspace)
            ));
        }
        workspace_roots.insert(workspace);
    }
    Ok(OpenClawWorkspaceInventory {
        workspace_roots,
        config_include_paths,
    })
}

fn agent_entries(config: &Value) -> Vec<&Map<String, Value>> {
    config
        .pointer("/agents/list")
        .and_then(Value::as_array)
        .map(|entries| entries.iter().filter_map(Value::as_object).collect())
        .unwrap_or_default()
}

fn resolve_default_agent_id(entries: &[&Map<String, Value>]) -> String {
    let selected = entries
        .iter()
        .find(|entry| entry.get("default").and_then(Value::as_bool) == Some(true))
        .or_else(|| entries.first());
    selected
        .and_then(|entry| entry.get("id"))
        .and_then(Value::as_str)
        .map(normalize_agent_id)
        .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string())
}

fn normalize_agent_id(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return DEFAULT_AGENT_ID.to_string();
    }
    let lowercase = trimmed.to_lowercase();
    let valid = trimmed.len() <= 64
        && trimmed
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric())
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-');
    if valid {
        return lowercase;
    }

    let mut normalized = String::new();
    let mut replacing = false;
    for ch in lowercase.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            normalized.push(ch);
            replacing = false;
        } else if !replacing {
            normalized.push('-');
            replacing = true;
        }
    }
    let normalized = normalized
        .trim_matches('-')
        .chars()
        .take(64)
        .collect::<String>();
    if normalized.is_empty() {
        DEFAULT_AGENT_ID.to_string()
    } else {
        normalized
    }
}

fn resolve_workspace_path(raw: &str, openclaw_home: &Path) -> Result<PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.contains('\0') {
        return Err("OpenClaw workspace path must not contain null bytes".to_string());
    }
    let expanded = match trimmed {
        "~" => openclaw_home.to_path_buf(),
        _ if trimmed.starts_with("~/") || trimmed.starts_with("~\\") => {
            openclaw_home.join(&trimmed[2..])
        }
        _ => PathBuf::from(trimmed),
    };
    if !expanded.is_absolute() {
        return Err(format!(
            "cannot safely preserve relative OpenClaw workspace path \"{trimmed}\"; configure an absolute path or a path under ~"
        ));
    }
    Ok(clean_path(&expanded))
}

struct IncludeProcessor {
    root_dir: PathBuf,
    canonical_root_dir: PathBuf,
    include_paths: BTreeSet<PathBuf>,
}

impl IncludeProcessor {
    fn new(root_dir: &Path) -> Result<Self, String> {
        let root_dir = clean_path(root_dir);
        let canonical_root_dir = canonicalize_path_allow_missing(&root_dir)?;
        Ok(Self {
            root_dir,
            canonical_root_dir,
            include_paths: BTreeSet::new(),
        })
    }

    fn process(
        &mut self,
        value: Value,
        containing_file: &Path,
        depth: usize,
        stack: &mut Vec<PathBuf>,
    ) -> Result<Value, String> {
        match value {
            Value::Array(values) => values
                .into_iter()
                .map(|value| self.process(value, containing_file, depth, stack))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(mut object) => {
                let Some(include) = object.remove("$include") else {
                    let mut resolved = Map::new();
                    for (key, value) in object {
                        resolved.insert(key, self.process(value, containing_file, depth, stack)?);
                    }
                    return Ok(Value::Object(resolved));
                };

                let included = self.resolve_include(include, containing_file, depth, stack)?;
                if object.is_empty() {
                    return Ok(included);
                }
                if !included.is_object() {
                    return Err(
                        "OpenClaw $include sibling keys require included content to be an object"
                            .to_string(),
                    );
                }

                let mut siblings = Map::new();
                for (key, value) in object {
                    siblings.insert(key, self.process(value, containing_file, depth, stack)?);
                }
                Ok(deep_merge(included, Value::Object(siblings)))
            }
            value => Ok(value),
        }
    }

    fn resolve_include(
        &mut self,
        include: Value,
        containing_file: &Path,
        depth: usize,
        stack: &mut Vec<PathBuf>,
    ) -> Result<Value, String> {
        match include {
            Value::String(path) => self.load_include(&path, containing_file, depth, stack),
            Value::Array(paths) => {
                let mut merged = Value::Object(Map::new());
                for path in paths {
                    let path = path.as_str().ok_or_else(|| {
                        "OpenClaw $include arrays must contain only paths".to_string()
                    })?;
                    merged = deep_merge(
                        merged,
                        self.load_include(path, containing_file, depth, stack)?,
                    );
                }
                Ok(merged)
            }
            _ => Err("OpenClaw $include must be a path or an array of paths".to_string()),
        }
    }

    fn load_include(
        &mut self,
        include_path: &str,
        containing_file: &Path,
        depth: usize,
        stack: &mut Vec<PathBuf>,
    ) -> Result<Value, String> {
        if depth >= MAX_INCLUDE_DEPTH {
            return Err(format!(
                "maximum OpenClaw $include depth ({MAX_INCLUDE_DEPTH}) exceeded at {include_path}"
            ));
        }
        let resolved = self.resolve_include_path(include_path, containing_file)?;
        if stack.contains(&resolved) {
            let chain = stack
                .iter()
                .chain(std::iter::once(&resolved))
                .map(|path| display_path(path))
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(format!("circular OpenClaw $include detected: {chain}"));
        }

        let metadata = fs::metadata(&resolved).map_err(|error| {
            format!(
                "failed to inspect OpenClaw include {}: {error}",
                display_path(&resolved)
            )
        })?;
        if metadata.len() > MAX_INCLUDE_FILE_BYTES {
            return Err(format!(
                "OpenClaw include exceeds {} bytes: {}",
                MAX_INCLUDE_FILE_BYTES,
                display_path(&resolved)
            ));
        }
        let raw = fs::read_to_string(&resolved).map_err(|error| {
            format!(
                "failed to read OpenClaw include {}: {error}",
                display_path(&resolved)
            )
        })?;
        let parsed = parse_json5(&raw, &resolved, "OpenClaw include")?;
        self.include_paths.insert(resolved.clone());
        stack.push(resolved.clone());
        let result = self.process(parsed, &resolved, depth + 1, stack);
        stack.pop();
        result
    }

    fn resolve_include_path(
        &self,
        include_path: &str,
        containing_file: &Path,
    ) -> Result<PathBuf, String> {
        if include_path.contains('\0') {
            return Err("OpenClaw $include path must not contain null bytes".to_string());
        }
        if include_path.len() >= MAX_INCLUDE_PATH_LENGTH {
            return Err(format!(
                "OpenClaw $include path exceeds {MAX_INCLUDE_PATH_LENGTH} characters"
            ));
        }
        let include_path = Path::new(include_path);
        let resolved = if include_path.is_absolute() {
            clean_path(include_path)
        } else {
            clean_path(
                &containing_file
                    .parent()
                    .unwrap_or(&self.root_dir)
                    .join(include_path),
            )
        };
        if display_path(&resolved).len() >= MAX_INCLUDE_PATH_LENGTH {
            return Err(format!(
                "resolved OpenClaw $include path exceeds {MAX_INCLUDE_PATH_LENGTH} characters"
            ));
        }
        if !resolved.starts_with(&self.root_dir) {
            return Err(format!(
                "OpenClaw $include path escapes the config directory: {}",
                display_path(&resolved)
            ));
        }
        let canonical = canonicalize_path_allow_missing(&resolved)?;
        if !canonical.starts_with(&self.canonical_root_dir) {
            return Err(format!(
                "OpenClaw $include path resolves outside the config directory: {}",
                display_path(&resolved)
            ));
        }
        Ok(resolved)
    }
}

fn parse_json5(raw: &str, path: &Path, label: &str) -> Result<Value, String> {
    json5::from_str(raw)
        .map_err(|error| format!("failed to parse {label} {}: {error}", display_path(path)))
}

fn deep_merge(base: Value, override_value: Value) -> Value {
    match (base, override_value) {
        (Value::Array(mut base), Value::Array(override_values)) => {
            base.extend(override_values);
            Value::Array(base)
        }
        (Value::Object(mut base), Value::Object(override_values)) => {
            for (key, value) in override_values {
                let merged = base
                    .remove(&key)
                    .map(|current| deep_merge(current, value.clone()))
                    .unwrap_or(value);
                base.insert(key, merged);
            }
            Value::Object(base)
        }
        (_, override_value) => override_value,
    }
}

fn canonicalize_path_allow_missing(path: &Path) -> Result<PathBuf, String> {
    let mut existing = path;
    let mut missing = Vec::<OsString>::new();
    loop {
        match fs::canonicalize(existing) {
            Ok(mut resolved) => {
                for component in missing.iter().rev() {
                    resolved.push(component);
                }
                return Ok(clean_path(&resolved));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let name = existing
                    .file_name()
                    .ok_or_else(|| format!("failed to resolve path: {}", display_path(path)))?;
                missing.push(name.to_os_string());
                existing = existing
                    .parent()
                    .ok_or_else(|| format!("failed to resolve path: {}", display_path(path)))?;
            }
            Err(error) => {
                return Err(format!(
                    "failed to resolve path {}: {error}",
                    display_path(path)
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn test_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "ocm-openclaw-workspaces-{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn effective_config_resolves_json5_nested_includes_and_merges_arrays() {
        let root = test_root("includes");
        let state_dir = root.join(".openclaw");
        fs::create_dir_all(state_dir.join("config")).unwrap();
        fs::write(
            state_dir.join("openclaw.json"),
            "{ $include: ['./config/base.json5', './config/agents.json5'], agents: { list: [{ id: 'local' }] } }",
        )
        .unwrap();
        fs::write(
            state_dir.join("config/base.json5"),
            "{ agents: { defaults: { workspace: '~/teams' } } }",
        )
        .unwrap();
        fs::write(
            state_dir.join("config/agents.json5"),
            "{ $include: './nested.json5', agents: { list: [{ id: 'ops' }] } }",
        )
        .unwrap();
        fs::write(
            state_dir.join("config/nested.json5"),
            "{ agents: { list: [{ id: 'main', default: true }] } }",
        )
        .unwrap();

        let resolved = load_effective_openclaw_config(&state_dir.join("openclaw.json"))
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved.value.pointer("/agents/defaults/workspace"),
            Some(&Value::String("~/teams".to_string()))
        );
        let ids = resolved
            .value
            .pointer("/agents/list")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["main", "ops", "local"]);
        assert_eq!(resolved.include_paths.len(), 3);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_inventory_matches_openclaw_agent_resolution() {
        let root = test_root("inventory");
        let paths = super::super::layout::derive_env_paths(&root);
        fs::create_dir_all(&paths.state_dir).unwrap();
        fs::write(
            &paths.config_path,
            r#"{
              "agents": {
                "defaults": { "workspace": "~/teams" },
                "list": [
                  { "id": "Primary", "default": true },
                  { "id": "Ops Team" },
                  { "id": "Custom", "workspace": "~/.openclaw/team/custom" }
                ]
              }
            }"#,
        )
        .unwrap();

        let inventory = resolve_env_openclaw_workspaces(&paths).unwrap();
        assert_eq!(
            inventory.workspace_roots,
            BTreeSet::from([
                root.join(".openclaw/workspace"),
                root.join("teams"),
                root.join("teams/ops-team"),
                root.join(".openclaw/team/custom"),
            ])
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_inventory_rejects_external_and_symlink_escaped_paths() {
        let root = test_root("external");
        let paths = super::super::layout::derive_env_paths(&root);
        fs::create_dir_all(&paths.state_dir).unwrap();
        let external = test_root("external-target");
        fs::create_dir_all(&external).unwrap();
        fs::write(
            &paths.config_path,
            format!(
                "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}",
                external.display()
            ),
        )
        .unwrap();
        let inventory = resolve_env_openclaw_workspaces(&paths).unwrap();
        assert!(
            inventory.archive_relative_roots(&paths.root).is_err(),
            "external workspace must be rejected"
        );

        #[cfg(unix)]
        {
            let linked = paths.state_dir.join("linked");
            std::os::unix::fs::symlink(&external, &linked).unwrap();
            fs::write(
                &paths.config_path,
                format!(
                    "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}",
                    linked.display()
                ),
            )
            .unwrap();
            let inventory = resolve_env_openclaw_workspaces(&paths).unwrap();
            assert!(
                inventory.archive_relative_roots(&paths.root).is_err(),
                "workspace symlink escaping the env root must be rejected"
            );
        }

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(external);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_inventory_rejects_internal_workspace_symlinks() {
        let root = test_root("internal-symlink");
        let paths = super::super::layout::derive_env_paths(&root);
        let target = paths.state_dir.join("team/ops");
        let linked = paths.state_dir.join("linked");
        fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, &linked).unwrap();
        fs::write(
            &paths.config_path,
            format!(
                "{{\"agents\":{{\"defaults\":{{\"workspace\":\"{}\"}}}}}}",
                linked.display()
            ),
        )
        .unwrap();

        let inventory = resolve_env_openclaw_workspaces(&paths).unwrap();
        assert!(inventory.contains(&linked));
        let error = inventory.archive_relative_roots(&paths.root).unwrap_err();
        assert!(error.contains("through a symlink"), "{error}");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_inventory_rejects_relative_paths() {
        let root = test_root("relative");
        let paths = super::super::layout::derive_env_paths(&root);
        fs::create_dir_all(&paths.state_dir).unwrap();
        fs::write(
            &paths.config_path,
            r#"{"agents":{"defaults":{"workspace":"relative/workspace"}}}"#,
        )
        .unwrap();

        let error = resolve_env_openclaw_workspaces(&paths).unwrap_err();
        assert!(
            error.contains("relative OpenClaw workspace path"),
            "{error}"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_inventory_does_not_infer_prefix_lookalikes() {
        let root = test_root("lookalikes");
        let paths = super::super::layout::derive_env_paths(&root);
        fs::create_dir_all(&paths.state_dir).unwrap();
        fs::write(&paths.config_path, "{}").unwrap();

        let inventory = resolve_env_openclaw_workspaces(&paths).unwrap();
        assert!(inventory.contains(&paths.workspace_dir));
        assert!(!inventory.contains(&paths.state_dir.join("workspace-attestations")));
        assert!(!inventory.contains(&paths.state_dir.join("workspace-cache")));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn effective_config_rejects_circular_includes() {
        let root = test_root("circular");
        let state_dir = root.join(".openclaw");
        fs::create_dir_all(&state_dir).unwrap();
        fs::write(
            state_dir.join("openclaw.json"),
            r#"{"$include":"./a.json5"}"#,
        )
        .unwrap();
        fs::write(
            state_dir.join("a.json5"),
            r#"{"$include":"./openclaw.json"}"#,
        )
        .unwrap();

        let error = load_effective_openclaw_config(&state_dir.join("openclaw.json")).unwrap_err();
        assert!(error.contains("circular OpenClaw $include"), "{error}");

        let _ = fs::remove_dir_all(root);
    }
}
