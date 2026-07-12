use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::store::{clean_path, display_path};

pub(crate) fn detect_openclaw_checkout(path: &Path) -> Option<PathBuf> {
    let package_json = path.join("package.json");
    let scripts_dir = path.join("scripts");
    if !package_json.exists() || !scripts_dir.join("run-node.mjs").exists() {
        return None;
    }

    let contents = fs::read_to_string(package_json).ok()?;
    let package: Value = serde_json::from_str(&contents).ok()?;
    if package.get("name").and_then(Value::as_str) == Some("openclaw") {
        Some(clean_path(path))
    } else {
        None
    }
}

pub(crate) fn discover_openclaw_checkout(cwd: &Path) -> Option<PathBuf> {
    for ancestor in cwd.ancestors().take(8) {
        if let Some(checkout) = detect_openclaw_checkout(ancestor) {
            return Some(checkout);
        }

        let sibling = ancestor.join("openclaw");
        if let Some(checkout) = detect_openclaw_checkout(&sibling) {
            return Some(checkout);
        }
    }

    None
}

pub(crate) fn default_worktree_root(repo_root: &Path, env_name: &str) -> PathBuf {
    clean_path(&repo_root.join(".worktrees").join(env_name))
}

pub(crate) fn ensure_openclaw_worktree(
    repo_root: &Path,
    env_name: &str,
) -> Result<PathBuf, String> {
    let repo_root = detect_openclaw_checkout(repo_root)
        .ok_or_else(|| format!("OpenClaw checkout not found at {}", display_path(repo_root)))?;
    let worktree_root = default_worktree_root(&repo_root, env_name);
    let registered = registered_worktree_paths(&repo_root)?;
    let worktree_registered = contains_worktree_path(&registered, &worktree_root);

    if worktree_registered {
        if !worktree_root.exists() {
            remove_registered_worktree(&repo_root, &worktree_root)?;
        } else if is_existing_openclaw_worktree(&worktree_root) {
            return Ok(worktree_root);
        } else {
            return Err(format!(
                "registered worktree is not a valid OpenClaw checkout: {}",
                display_path(&worktree_root)
            ));
        }
    }

    if worktree_root.exists() {
        return Err(format!(
            "worktree path already exists but is not registered to this OpenClaw checkout: {}",
            display_path(&worktree_root)
        ));
    }

    if let Some(parent) = worktree_root.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .args(["worktree", "add", "--detach"])
        .arg(&worktree_root)
        .output()
        .map_err(|error| format!("failed to run git worktree add: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("git worktree add failed: {detail}"));
    }

    let registered = registered_worktree_paths(&repo_root)?;
    if !contains_worktree_path(&registered, &worktree_root)
        || !is_existing_openclaw_worktree(&worktree_root)
    {
        return Err(format!(
            "created worktree is not a valid OpenClaw checkout: {}",
            display_path(&worktree_root)
        ));
    }

    Ok(worktree_root)
}

pub(crate) fn remove_openclaw_worktree(
    repo_root: &Path,
    worktree_root: &Path,
) -> Result<(), String> {
    let registered = registered_worktree_paths(repo_root)?;
    if !contains_worktree_path(&registered, worktree_root) {
        if worktree_root.exists() {
            return Err(format!(
                "refusing to remove worktree path not registered to this OpenClaw checkout: {}",
                display_path(worktree_root)
            ));
        }
        return Ok(());
    }

    remove_registered_worktree(repo_root, worktree_root)
}

fn remove_registered_worktree(repo_root: &Path, worktree_root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove"])
        .arg(worktree_root)
        .output()
        .map_err(|error| format!("failed to run git worktree remove: {error}"))?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() { stderr } else { stdout };
    Err(format!("git worktree remove failed: {detail}"))
}

fn registered_worktree_paths(repo_root: &Path) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(|error| format!("failed to run git worktree list: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        return Err(format!("git worktree list failed: {detail}"));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "git worktree list returned a non-UTF-8 path".to_string())?;
    Ok(stdout
        .split('\0')
        .filter_map(|field| field.strip_prefix("worktree "))
        .map(PathBuf::from)
        .collect())
}

fn contains_worktree_path(registered: &[PathBuf], expected: &Path) -> bool {
    let expected = normalize_worktree_path(expected);
    registered
        .iter()
        .any(|path| normalize_worktree_path(path) == expected)
}

fn normalize_worktree_path(path: &Path) -> PathBuf {
    if let Ok(path) = fs::canonicalize(path) {
        return path;
    }

    let mut ancestor = path;
    let mut missing = Vec::<OsString>::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            return clean_path(path);
        };
        missing.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            return clean_path(path);
        };
        ancestor = parent;
    }

    let mut normalized = fs::canonicalize(ancestor).unwrap_or_else(|_| clean_path(ancestor));
    for component in missing.into_iter().rev() {
        normalized.push(component);
    }
    clean_path(&normalized)
}

fn is_existing_openclaw_worktree(path: &Path) -> bool {
    path.exists()
        && path.join(".git").exists()
        && detect_openclaw_checkout(path).is_some()
        && Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--is-inside-work-tree"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
}
