use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use fs2::FileExt;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::EnvironmentService;
use crate::store::{
    display_path, ensure_dir, now_utc, read_json, source_watch_override_path, validate_name,
    write_json,
};

const SOURCE_WATCH_OVERRIDE_KIND: &str = "ocm-source-watch-override";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceWatchOverride {
    pub kind: String,
    pub env_name: String,
    pub repo_root: String,
    pub watch_pid: u32,
    pub token: String,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
pub struct CreateSourceWatchOverrideOptions {
    pub env_name: String,
    pub repo_root: PathBuf,
    pub watch_pid: u32,
}

#[derive(Debug)]
pub(crate) struct SourceWatchLease {
    lock_file: File,
}

impl Drop for SourceWatchLease {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.lock_file);
    }
}

impl SourceWatchOverride {
    pub fn openclaw_entry_path(&self) -> PathBuf {
        Path::new(&self.repo_root).join("openclaw.mjs")
    }

    pub fn extensions_dir(&self) -> PathBuf {
        Path::new(&self.repo_root).join("extensions")
    }

    pub fn command_label(&self) -> String {
        format!("node {}", display_path(&self.openclaw_entry_path()))
    }
}

impl<'a> EnvironmentService<'a> {
    pub(crate) fn acquire_source_watch_lease(
        &self,
        env_name: &str,
    ) -> Result<SourceWatchLease, String> {
        let env_name = validate_name(env_name, "Environment name")?;
        let override_path = source_watch_override_path(&env_name, self.env, self.cwd)?;
        let lock_path = override_path.with_extension("lock");
        if let Some(parent) = lock_path.parent() {
            ensure_dir(parent)?;
        }
        let mut lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(&lock_path)
            .map_err(|error| {
                format!(
                    "failed opening source watch lock {}: {error}",
                    display_path(&lock_path)
                )
            })?;
        match FileExt::try_lock_exclusive(&lock_file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                return Err(format!(
                    "source watch for env \"{env_name}\" is already active or starting"
                ));
            }
            Err(error) => {
                return Err(format!(
                    "failed locking source watch for env \"{env_name}\": {error}"
                ));
            }
        }

        // Owning the OS lock proves any surviving metadata belongs to a dead
        // lease, even if its child PID has since been reused by another process.
        remove_file_if_present(&override_path)?;

        lock_file
            .set_len(0)
            .and_then(|()| writeln!(lock_file, "{}", std::process::id()))
            .map_err(|error| {
                format!(
                    "failed recording source watch lock {}: {error}",
                    display_path(&lock_path)
                )
            })?;
        Ok(SourceWatchLease { lock_file })
    }

    pub fn create_source_watch_override(
        &self,
        options: CreateSourceWatchOverrideOptions,
    ) -> Result<SourceWatchOverride, String> {
        let env_name = validate_name(&options.env_name, "Environment name")?;
        let path = source_watch_override_path(&env_name, self.env, self.cwd)?;
        if let Some(parent) = path.parent() {
            ensure_dir(parent)?;
        }
        let token = format!("{}-{}", options.watch_pid, now_utc().unix_timestamp_nanos());
        let meta = SourceWatchOverride {
            kind: SOURCE_WATCH_OVERRIDE_KIND.to_string(),
            env_name,
            repo_root: display_path(&options.repo_root),
            watch_pid: options.watch_pid,
            token,
            started_at: now_utc(),
        };
        write_json(&path, &meta)?;
        Ok(meta)
    }

    pub fn clear_source_watch_override(&self, env_name: &str, token: &str) -> Result<bool, String> {
        let env_name = validate_name(env_name, "Environment name")?;
        let path = source_watch_override_path(&env_name, self.env, self.cwd)?;
        if !path.exists() {
            return Ok(false);
        }
        let existing = match read_json::<SourceWatchOverride>(&path) {
            Ok(existing) => existing,
            Err(_) => {
                remove_file_if_present(&path)?;
                return Ok(true);
            }
        };
        if existing.token != token {
            return Ok(false);
        }
        remove_file_if_present(&path)?;
        Ok(true)
    }

    pub fn active_source_watch_override(
        &self,
        env_name: &str,
    ) -> Result<Option<SourceWatchOverride>, String> {
        let env_name = validate_name(env_name, "Environment name")?;
        let path = source_watch_override_path(&env_name, self.env, self.cwd)?;
        let lock_path = path.with_extension("lock");
        if !path.exists() {
            return Ok(None);
        }
        let meta = match read_json::<SourceWatchOverride>(&path) {
            Ok(meta) => meta,
            Err(_) => {
                remove_file_if_present(&path)?;
                return Ok(None);
            }
        };
        if !is_valid_source_watch_override(&meta, &env_name, &lock_path)? {
            remove_file_if_present(&path)?;
            return Ok(None);
        }
        Ok(Some(meta))
    }
}

fn is_valid_source_watch_override(
    meta: &SourceWatchOverride,
    env_name: &str,
    lock_path: &Path,
) -> Result<bool, String> {
    let metadata_valid = meta.kind == SOURCE_WATCH_OVERRIDE_KIND
        && meta.env_name == env_name
        && !meta.repo_root.trim().is_empty()
        && !meta.token.trim().is_empty()
        && meta.watch_pid > 0
        && Path::new(&meta.repo_root).join("openclaw.mjs").is_file()
        && Path::new(&meta.repo_root).join("extensions").is_dir()
        && is_process_alive(meta.watch_pid);
    if !metadata_valid {
        return Ok(false);
    }
    source_watch_lease_is_active(lock_path)
}

fn source_watch_lease_is_active(lock_path: &Path) -> Result<bool, String> {
    let lock_file = match OpenOptions::new().read(true).write(true).open(lock_path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "failed opening source watch lock {}: {error}",
                display_path(lock_path)
            ));
        }
    };
    match FileExt::try_lock_exclusive(&lock_file) {
        Ok(()) => {
            FileExt::unlock(&lock_file).map_err(|error| {
                format!(
                    "failed unlocking stale source watch lock {}: {error}",
                    display_path(lock_path)
                )
            })?;
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(true),
        Err(error) => Err(format!(
            "failed checking source watch lock {}: {error}",
            display_path(lock_path)
        )),
    }
}

fn remove_file_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed removing {}: {error}", display_path(path))),
    }
}

#[cfg(unix)]
fn is_process_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_process_alive(pid: u32) -> bool {
    let filter = format!("PID eq {pid}");
    Command::new("tasklist")
        .args(["/FI", &filter])
        .output()
        .map(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_watch_override_labels_the_built_entry() {
        let meta = SourceWatchOverride {
            kind: SOURCE_WATCH_OVERRIDE_KIND.to_string(),
            env_name: "demo".to_string(),
            repo_root: "/repo/openclaw".to_string(),
            watch_pid: 123,
            token: "123-token".to_string(),
            started_at: OffsetDateTime::UNIX_EPOCH,
        };

        assert_eq!(
            meta.openclaw_entry_path(),
            PathBuf::from("/repo/openclaw/openclaw.mjs")
        );
        assert_eq!(
            meta.extensions_dir(),
            PathBuf::from("/repo/openclaw/extensions")
        );
        assert_eq!(meta.command_label(), "node /repo/openclaw/openclaw.mjs");
    }
}
