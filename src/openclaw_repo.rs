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

    if is_existing_openclaw_worktree(&worktree_root) {
        return Ok(worktree_root);
    }

    if worktree_root.exists() {
        return Err(format!(
            "worktree path already exists and is not a valid OpenClaw worktree: {}",
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

    if !is_existing_openclaw_worktree(&worktree_root) {
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
    if !worktree_root.exists() {
        return Ok(());
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["worktree", "remove", "--force"])
        .arg(worktree_root)
        .output()
        .map_err(|error| format!("failed to run git worktree remove: {error}"))?;
    if output.status.success() {
        return Ok(());
    }

    fs::remove_dir_all(worktree_root).map_err(|error| {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            error.to_string()
        } else {
            format!("{stderr}; fallback remove failed: {error}")
        }
    })
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
