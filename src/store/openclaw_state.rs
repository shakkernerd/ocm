use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::env::EnvMeta;

use super::common::path_exists;
use super::layout::{EnvPaths, clean_path, derive_env_paths, display_path};

#[derive(Clone, Debug)]
pub(crate) struct OpenClawStateAudit {
    pub issues: Vec<String>,
    pub repair_runtime_state: bool,
}

pub(crate) fn audit_openclaw_state(meta: &EnvMeta, known_envs: &[EnvMeta]) -> OpenClawStateAudit {
    let paths = derive_env_paths(Path::new(&meta.root));
    if !path_exists(&paths.state_dir) {
        return OpenClawStateAudit {
            issues: Vec::new(),
            repair_runtime_state: false,
        };
    }

    let mut known_refs = BTreeMap::<String, (PathBuf, usize)>::new();
    let mut inferred_refs = BTreeMap::<PathBuf, usize>::new();
    collect_runtime_state_path_refs(
        &paths,
        meta,
        known_envs,
        &mut known_refs,
        &mut inferred_refs,
    );

    let mut issues = Vec::new();
    let known_roots = known_refs
        .values()
        .map(|(root, _)| root.clone())
        .collect::<BTreeSet<_>>();
    for (env_name, (root, count)) in &known_refs {
        push_issue(
            &mut issues,
            format!(
                "OpenClaw runtime state contains {count} copied path reference(s) under env \"{env_name}\" root: {}",
                display_path(root)
            ),
        );
    }
    for (root, count) in &inferred_refs {
        if known_roots.contains(root) {
            continue;
        }
        push_issue(
            &mut issues,
            format!(
                "OpenClaw runtime state contains {count} env-scoped path reference(s) outside the current env root: {}",
                display_path(root)
            ),
        );
    }

    OpenClawStateAudit {
        repair_runtime_state: !issues.is_empty(),
        issues,
    }
}

pub(crate) fn repair_openclaw_runtime_state(meta: &EnvMeta) -> Result<bool, String> {
    let paths = derive_env_paths(Path::new(&meta.root));
    clear_nonportable_runtime_state(&paths)
}

pub(crate) fn prepare_migrated_runtime_state(
    paths: &EnvPaths,
    source_state_root: &Path,
) -> Result<bool, String> {
    if !path_exists(&paths.state_dir) {
        return Ok(false);
    }

    let mut changed = false;
    changed |= rewrite_runtime_state_root_refs(
        &paths.state_dir,
        &paths.config_path,
        &paths.workspace_dir,
        source_state_root,
        &paths.state_dir,
    )?;
    changed |=
        clear_volatile_runtime_state(&paths.state_dir, &paths.config_path, &paths.workspace_dir)?;
    Ok(changed)
}

pub(crate) fn clear_nonportable_runtime_state(paths: &EnvPaths) -> Result<bool, String> {
    if !path_exists(&paths.state_dir) {
        return Ok(false);
    }

    let mut changed = false;
    let entries = fs::read_dir(&paths.state_dir).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        if name == OsStr::new("openclaw.json") || name == OsStr::new("workspace") {
            continue;
        }
        if name == OsStr::new("agents") {
            if prune_agent_runtime_state(&path)? {
                changed = true;
            }
            continue;
        }

        remove_path(&path)?;
        changed = true;
    }

    Ok(changed)
}

fn rewrite_runtime_state_root_refs(
    root: &Path,
    config_path: &Path,
    workspace_dir: &Path,
    source_state_root: &Path,
    target_state_root: &Path,
) -> Result<bool, String> {
    let mut changed = false;
    let source_root = display_path(source_state_root);
    let target_root = display_path(target_state_root);
    rewrite_runtime_state_root_refs_inner(
        root,
        config_path,
        workspace_dir,
        &source_root,
        &target_root,
        &mut changed,
    )?;
    Ok(changed)
}

fn rewrite_runtime_state_root_refs_inner(
    root: &Path,
    config_path: &Path,
    workspace_dir: &Path,
    source_root: &str,
    target_root: &str,
    changed: &mut bool,
) -> Result<(), String> {
    if !path_exists(root) {
        return Ok(());
    }

    let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path == config_path || path.starts_with(workspace_dir) {
            continue;
        }

        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.is_dir() {
            rewrite_runtime_state_root_refs_inner(
                &path,
                config_path,
                workspace_dir,
                source_root,
                target_root,
                changed,
            )?;
            continue;
        }
        if !metadata.is_file() {
            continue;
        }

        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let rewritten = raw.replace(source_root, target_root);
        if rewritten == raw {
            continue;
        }
        fs::write(&path, rewritten).map_err(|error| error.to_string())?;
        *changed = true;
    }

    Ok(())
}

fn clear_volatile_runtime_state(
    root: &Path,
    config_path: &Path,
    workspace_dir: &Path,
) -> Result<bool, String> {
    if !path_exists(root) {
        return Ok(false);
    }

    let mut changed = false;
    let entries = fs::read_dir(root).map_err(|error| error.to_string())?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path == config_path || path.starts_with(workspace_dir) {
            continue;
        }

        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        let file_name = entry.file_name();
        if should_remove_volatile_runtime_path(&file_name, metadata.is_dir()) {
            remove_path(&path)?;
            changed = true;
            continue;
        }

        if metadata.is_dir() {
            changed |= clear_volatile_runtime_state(&path, config_path, workspace_dir)?;
        }
    }

    Ok(changed)
}

fn prune_agent_runtime_state(agents_root: &Path) -> Result<bool, String> {
    if !path_exists(agents_root) {
        return Ok(false);
    }

    let mut changed = false;
    let agent_entries = fs::read_dir(agents_root).map_err(|error| error.to_string())?;
    for entry in agent_entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let agent_root = entry.path();
        let metadata = fs::symlink_metadata(&agent_root).map_err(|error| error.to_string())?;
        if !metadata.is_dir() {
            remove_path(&agent_root)?;
            changed = true;
            continue;
        }

        let child_entries = fs::read_dir(&agent_root).map_err(|error| error.to_string())?;
        for child in child_entries {
            let child = child.map_err(|error| error.to_string())?;
            let child_path = child.path();
            if child.file_name() == OsStr::new("agent") {
                continue;
            }
            remove_path(&child_path)?;
            changed = true;
        }

        let mut remaining = fs::read_dir(&agent_root).map_err(|error| error.to_string())?;
        if remaining.next().is_none() {
            remove_path(&agent_root)?;
            changed = true;
        }
    }

    let mut remaining = fs::read_dir(agents_root).map_err(|error| error.to_string())?;
    if remaining.next().is_none() {
        remove_path(agents_root)?;
        changed = true;
    }

    Ok(changed)
}

fn should_remove_volatile_runtime_path(name: &OsStr, is_dir: bool) -> bool {
    if is_dir {
        return matches!(
            name.to_str(),
            Some("run") | Some("tmp") | Some("temp") | Some("locks")
        );
    }

    matches!(
        Path::new(name).extension().and_then(OsStr::to_str),
        Some("pid") | Some("lock") | Some("sock") | Some("socket")
    ) || matches!(
        name.to_str(),
        Some("pid") | Some("lock") | Some("sock") | Some("socket")
    )
}

fn collect_runtime_state_path_refs(
    paths: &EnvPaths,
    current: &EnvMeta,
    known_envs: &[EnvMeta],
    known_refs: &mut BTreeMap<String, (PathBuf, usize)>,
    inferred_refs: &mut BTreeMap<PathBuf, usize>,
) {
    visit_runtime_state_files(
        &paths.state_dir,
        &paths.config_path,
        &paths.workspace_dir,
        &mut |path| {
            let Ok(raw) = fs::read_to_string(path) else {
                return;
            };

            for token in candidate_path_tokens(&raw) {
                let candidate = Path::new(token);
                if let Some((env_name, env_root)) =
                    matching_foreign_env_root(current, known_envs, candidate)
                {
                    let entry = known_refs
                        .entry(env_name)
                        .or_insert_with(|| (env_root.clone(), 0));
                    entry.1 += 1;
                    continue;
                }
                if let Some(env_root) = inferred_foreign_env_root(&paths.root, candidate) {
                    *inferred_refs.entry(env_root).or_insert(0) += 1;
                }
            }
        },
    );
}

fn visit_runtime_state_files(
    root: &Path,
    config_path: &Path,
    workspace_dir: &Path,
    on_file: &mut dyn FnMut(&Path),
) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path == config_path || path.starts_with(workspace_dir) {
            continue;
        }

        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if metadata.is_dir() {
            visit_runtime_state_files(&path, config_path, workspace_dir, on_file);
        } else if metadata.is_file() {
            on_file(&path);
        }
    }
}

fn candidate_path_tokens(raw: &str) -> impl Iterator<Item = &str> {
    raw.split(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>'
            )
    })
    .filter(|token| !token.is_empty())
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

fn remove_path(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.is_dir() {
        fs::remove_dir_all(path).map_err(|error| error.to_string())
    } else {
        fs::remove_file(path).map_err(|error| error.to_string())
    }
}

fn push_issue(issues: &mut Vec<String>, issue: String) {
    if !issues.iter().any(|current| current == &issue) {
        issues.push(issue);
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use time::OffsetDateTime;

    use super::*;

    fn meta(name: &str, root: &str) -> EnvMeta {
        EnvMeta {
            kind: "ocm-env".to_string(),
            name: name.to_string(),
            root: root.to_string(),
            gateway_port: None,
            service_enabled: true,
            default_runtime: None,
            default_launcher: None,
            protected: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_used_at: None,
        }
    }

    #[test]
    fn audit_detects_foreign_env_refs_in_runtime_state() {
        let temp =
            std::env::temp_dir().join(format!("ocm-openclaw-state-audit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let source_root = temp.join("source");
        let target_root = temp.join("target");
        fs::create_dir_all(target_root.join(".openclaw/agents/main/sessions")).unwrap();
        fs::write(
            target_root.join(".openclaw/agents/main/sessions/main.jsonl"),
            format!(
                "{{\"cwd\":\"{}\"}}\n",
                source_root.join(".openclaw/workspace").display()
            ),
        )
        .unwrap();

        let current = meta("target", &display_path(&target_root));
        let known_envs = vec![meta("source", &display_path(&source_root)), current.clone()];
        let audit = audit_openclaw_state(&current, &known_envs);
        assert!(audit.repair_runtime_state);
        assert!(audit.issues.iter().any(|issue| issue.contains(
            "OpenClaw runtime state contains 1 copied path reference(s) under env \"source\" root"
        )));

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn clear_nonportable_runtime_state_preserves_config_workspace_and_agent_auth() {
        let temp =
            std::env::temp_dir().join(format!("ocm-openclaw-state-clear-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let paths = derive_env_paths(&temp);
        fs::create_dir_all(paths.workspace_dir.join("notes")).unwrap();
        fs::write(&paths.config_path, "{}\n").unwrap();
        fs::create_dir_all(paths.state_dir.join("agents/main/agent")).unwrap();
        fs::create_dir_all(paths.state_dir.join("agents/main/sessions")).unwrap();
        fs::write(
            paths.state_dir.join("agents/main/agent/auth-profiles.json"),
            "{}\n",
        )
        .unwrap();
        fs::write(paths.state_dir.join("logs.txt"), "log\n").unwrap();
        fs::write(paths.workspace_dir.join("notes/todo.txt"), "keep\n").unwrap();

        let changed = clear_nonportable_runtime_state(&paths).unwrap();
        assert!(changed);
        assert!(paths.config_path.exists());
        assert!(paths.workspace_dir.join("notes/todo.txt").exists());
        assert!(
            paths
                .state_dir
                .join("agents/main/agent/auth-profiles.json")
                .exists()
        );
        assert!(!paths.state_dir.join("agents/main/sessions").exists());
        assert!(!paths.state_dir.join("logs.txt").exists());

        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn prepare_migrated_runtime_state_preserves_history_and_rewrites_root_refs() {
        let temp =
            std::env::temp_dir().join(format!("ocm-openclaw-state-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        let source_state_root = temp.join("legacy-home/.openclaw");
        let paths = derive_env_paths(temp.join("target"));
        fs::create_dir_all(paths.workspace_dir.join("notes")).unwrap();
        fs::create_dir_all(paths.state_dir.join("agents/main/agent")).unwrap();
        fs::create_dir_all(paths.state_dir.join("agents/main/sessions")).unwrap();
        fs::create_dir_all(paths.state_dir.join("logs")).unwrap();
        fs::create_dir_all(paths.state_dir.join("run")).unwrap();
        fs::write(&paths.config_path, "{}\n").unwrap();
        fs::write(paths.workspace_dir.join("notes/todo.txt"), "keep\n").unwrap();
        fs::write(
            paths.state_dir.join("agents/main/agent/auth-profiles.json"),
            "{}\n",
        )
        .unwrap();
        fs::write(
            paths.state_dir.join("agents/main/sessions/main.jsonl"),
            format!(
                "{{\"cwd\":\"{}\",\"log\":\"{}\"}}\n",
                source_state_root.join("workspace").display(),
                source_state_root.join("logs/gateway.log").display()
            ),
        )
        .unwrap();
        fs::write(
            paths.state_dir.join("logs/gateway.log"),
            format!("source={}\n", source_state_root.join("workspace").display()),
        )
        .unwrap();
        fs::write(
            paths.state_dir.join("openclaw.json.bak"),
            format!("backup={}\n", source_state_root.display()),
        )
        .unwrap();
        fs::write(paths.state_dir.join("gateway.pid"), "4242\n").unwrap();
        fs::write(paths.state_dir.join("run/live.sock"), "sock\n").unwrap();

        let changed = prepare_migrated_runtime_state(&paths, &source_state_root).unwrap();
        assert!(changed);
        assert!(paths.workspace_dir.join("notes/todo.txt").exists());
        assert!(
            paths
                .state_dir
                .join("agents/main/sessions/main.jsonl")
                .exists()
        );
        assert!(paths.state_dir.join("logs/gateway.log").exists());
        assert!(paths.state_dir.join("openclaw.json.bak").exists());
        assert!(!paths.state_dir.join("gateway.pid").exists());
        assert!(!paths.state_dir.join("run").exists());

        let session_raw =
            fs::read_to_string(paths.state_dir.join("agents/main/sessions/main.jsonl")).unwrap();
        let logs_raw = fs::read_to_string(paths.state_dir.join("logs/gateway.log")).unwrap();
        let backup_raw = fs::read_to_string(paths.state_dir.join("openclaw.json.bak")).unwrap();
        assert!(session_raw.contains(&display_path(&paths.state_dir.join("workspace"))));
        assert!(session_raw.contains(&display_path(&paths.state_dir.join("logs/gateway.log"))));
        assert!(!session_raw.contains(&display_path(&source_state_root)));
        assert!(logs_raw.contains(&display_path(&paths.state_dir.join("workspace"))));
        assert!(!logs_raw.contains(&display_path(&source_state_root)));
        assert!(backup_raw.contains(&display_path(&paths.state_dir)));
        assert!(!backup_raw.contains(&display_path(&source_state_root)));

        let _ = fs::remove_dir_all(&temp);
    }
}
