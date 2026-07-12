use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;

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
    env_name: String,
    lock_file: File,
}

impl SourceWatchLease {
    #[cfg(unix)]
    pub(crate) fn configure_child(&self, command: &mut Command) {
        let lock_fd = self.lock_file.as_raw_fd();
        command.process_group(0);
        // The watcher inherits the lease so an OCM crash cannot release
        // exclusivity while the source gateway remains alive.
        unsafe {
            command.pre_exec(move || {
                let flags = libc::fcntl(lock_fd, libc::F_GETFD);
                if flags == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::fcntl(lock_fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    #[cfg(not(unix))]
    pub(crate) fn configure_child(&self, _command: &mut Command) {}
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
        if let Ok(meta) = read_json::<SourceWatchOverride>(&override_path)
            && !is_leased_source_watch(&meta)
            && is_valid_source_watch_metadata(&meta, &env_name)
            && is_legacy_source_watch_process(&meta)
        {
            return Err(format!(
                "source watch for env \"{env_name}\" is already active with legacy pid {}",
                meta.watch_pid
            ));
        }
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
        Ok(SourceWatchLease {
            env_name,
            lock_file,
        })
    }

    pub fn create_source_watch_override(
        &self,
        options: CreateSourceWatchOverrideOptions,
    ) -> Result<SourceWatchOverride, String> {
        self.write_source_watch_override(options, false)
    }

    pub(crate) fn create_source_watch_override_with_lease(
        &self,
        options: CreateSourceWatchOverrideOptions,
        lease: &SourceWatchLease,
    ) -> Result<SourceWatchOverride, String> {
        if options.env_name != lease.env_name {
            return Err(format!(
                "source watch lease for env \"{}\" cannot create an override for env \"{}\"",
                lease.env_name, options.env_name
            ));
        }
        self.write_source_watch_override(options, true)
    }

    fn write_source_watch_override(
        &self,
        options: CreateSourceWatchOverrideOptions,
        leased: bool,
    ) -> Result<SourceWatchOverride, String> {
        let env_name = validate_name(&options.env_name, "Environment name")?;
        let path = source_watch_override_path(&env_name, self.env, self.cwd)?;
        if let Some(parent) = path.parent() {
            ensure_dir(parent)?;
        }
        let token_prefix = if leased { "lease-" } else { "" };
        let token = format!(
            "{token_prefix}{}-{}",
            options.watch_pid,
            now_utc().unix_timestamp_nanos()
        );
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
        if !is_leased_source_watch(&meta)
            && is_valid_source_watch_metadata(&meta, &env_name)
            && is_legacy_source_watch_process(&meta)
        {
            return Ok(Some(meta));
        }
        if !lock_path.exists() {
            remove_file_if_present(&path)?;
            return Ok(None);
        }

        let lock_file = open_source_watch_lock(&lock_path)?;
        match FileExt::try_lock_exclusive(&lock_file) {
            Ok(()) => {
                remove_file_if_present(&path)?;
                FileExt::unlock(&lock_file).map_err(|error| {
                    format!(
                        "failed unlocking stale source watch lock {}: {error}",
                        display_path(&lock_path)
                    )
                })?;
                Ok(None)
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if is_valid_source_watch_metadata(&meta, &env_name) {
                    Ok(Some(meta))
                } else {
                    Ok(None)
                }
            }
            Err(error) => Err(format!(
                "failed checking source watch lock {}: {error}",
                display_path(&lock_path)
            )),
        }
    }
}

fn is_leased_source_watch(meta: &SourceWatchOverride) -> bool {
    meta.token.starts_with("lease-")
}

fn is_valid_source_watch_metadata(meta: &SourceWatchOverride, env_name: &str) -> bool {
    meta.kind == SOURCE_WATCH_OVERRIDE_KIND
        && meta.env_name == env_name
        && !meta.repo_root.trim().is_empty()
        && !meta.token.trim().is_empty()
        && meta.watch_pid > 0
        && Path::new(&meta.repo_root).join("openclaw.mjs").is_file()
        && Path::new(&meta.repo_root).join("extensions").is_dir()
        && is_process_alive(meta.watch_pid)
}

fn open_source_watch_lock(lock_path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(lock_path)
        .map_err(|error| {
            format!(
                "failed opening source watch lock {}: {error}",
                display_path(lock_path)
            )
        })
}

fn is_legacy_source_watch_process(meta: &SourceWatchOverride) -> bool {
    let command_matches = process_command_line(meta.watch_pid)
        .is_some_and(|command| legacy_source_watch_command_matches(&command));
    if !command_matches {
        return false;
    }
    legacy_source_watch_cwd_matches(meta)
}

fn legacy_source_watch_command_matches(command: &str) -> bool {
    let command = command.replace('\\', "/");
    command.contains("node")
        && command.contains("scripts/watch-node.mjs")
        && command.contains("gateway")
        && command.contains("run")
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

#[cfg(unix)]
fn process_command_line(pid: u32) -> Option<String> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "linux")]
fn legacy_source_watch_cwd_matches(meta: &SourceWatchOverride) -> bool {
    fs::read_link(format!("/proc/{}/cwd", meta.watch_pid))
        .ok()
        .is_some_and(|cwd| same_path(&cwd, Path::new(&meta.repo_root)))
}

#[cfg(all(unix, not(target_os = "linux")))]
fn legacy_source_watch_cwd_matches(meta: &SourceWatchOverride) -> bool {
    let output = Command::new("lsof")
        .args([
            "-b",
            "-w",
            "-a",
            "-p",
            &meta.watch_pid.to_string(),
            "-d",
            "cwd",
            "-Fn",
        ])
        .output();
    let Ok(output) = output else {
        return false;
    };
    output.status.success()
        && String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix('n'))
            .is_some_and(|cwd| same_path(Path::new(cwd), Path::new(&meta.repo_root)))
}

#[cfg(windows)]
fn legacy_source_watch_cwd_matches(_meta: &SourceWatchOverride) -> bool {
    true
}

#[cfg(unix)]
fn same_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
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

#[cfg(windows)]
fn process_command_line(pid: u32) -> Option<String> {
    let script = format!("(Get-CimInstance Win32_Process -Filter 'ProcessId = {pid}').CommandLine");
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
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

    #[test]
    fn legacy_source_watch_identity_requires_the_watch_command() {
        assert!(legacy_source_watch_command_matches(
            "node scripts/watch-node.mjs gateway run --port 18789"
        ));
        assert!(!legacy_source_watch_command_matches(
            "cargo test source_watch"
        ));
    }
}
