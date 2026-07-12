use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;

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
        } else if is_existing_openclaw_worktree(&repo_root, &worktree_root) {
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
        || !is_existing_openclaw_worktree(&repo_root, &worktree_root)
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

    parse_registered_worktree_paths(&output.stdout)
}

fn parse_registered_worktree_paths(output: &[u8]) -> Result<Vec<PathBuf>, String> {
    output
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .map(|path| git_path_from_bytes(path).map(PathBuf::from))
        .collect()
}

fn contains_worktree_path(registered: &[PathBuf], expected: &Path) -> bool {
    let expected = normalize_worktree_path(expected);
    registered
        .iter()
        .any(|path| normalize_worktree_path(path) == expected)
}

fn normalize_worktree_path(path: &Path) -> PathBuf {
    let Some(parent) = path.parent() else {
        return clean_path(path);
    };
    let Some(name) = path.file_name() else {
        return clean_path(path);
    };

    let mut ancestor = parent;
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
    normalized.push(name);
    clean_path(&normalized)
}

fn is_existing_openclaw_worktree(repo_root: &Path, path: &Path) -> bool {
    path.exists()
        && path.join(".git").exists()
        && detect_openclaw_checkout(path).is_some()
        && git_common_dir(repo_root)
            .zip(git_common_dir(path))
            .is_some_and(|(repo, worktree)| repo == worktree)
}

fn git_common_dir(path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let common_dir = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    let common_dir = common_dir.strip_suffix(b"\r").unwrap_or(common_dir);
    let common_dir = PathBuf::from(git_path_from_bytes(common_dir).ok()?);
    fs::canonicalize(&common_dir)
        .ok()
        .or_else(|| Some(clean_path(&common_dir)))
}

#[cfg(unix)]
fn git_path_from_bytes(path: &[u8]) -> Result<OsString, String> {
    Ok(OsString::from_vec(path.to_vec()))
}

#[cfg(not(unix))]
fn git_path_from_bytes(path: &[u8]) -> Result<OsString, String> {
    String::from_utf8(path.to_vec())
        .map(OsString::from)
        .map_err(|_| "git returned a non-UTF-8 path".to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::ffi::OsStrExt;

    use super::parse_registered_worktree_paths;

    #[test]
    fn worktree_porcelain_parser_preserves_non_utf8_paths() {
        let paths = parse_registered_worktree_paths(
            b"worktree /tmp/openclaw\0HEAD abc\0\0worktree /tmp/other\xff\0HEAD def\0\0",
        )
        .unwrap();

        assert_eq!(paths.len(), 2);
        assert_eq!(paths[1].as_os_str().as_bytes(), b"/tmp/other\xff");
    }
}
