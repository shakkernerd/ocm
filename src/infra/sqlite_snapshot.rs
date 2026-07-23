use std::fs::{self, File};
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, OpenFlags};

const SQLITE_BACKUP_PAGES_PER_STEP: i32 = 256;
const SQLITE_BACKUP_RETRY_DELAY: Duration = Duration::from_millis(10);
const SQLITE_BACKUP_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn create_sqlite_snapshot(source: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        return Err(format!(
            "SQLite snapshot target already exists: {}",
            target.display()
        ));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let result = create_sqlite_snapshot_inner(source, target);
    if result.is_err() {
        let _ = fs::remove_file(target);
    }
    result
}

fn create_sqlite_snapshot_inner(source: &Path, target: &Path) -> Result<(), String> {
    let source_db = Connection::open_with_flags(
        source,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| {
        format!(
            "failed to open SQLite database {} for snapshot: {error}",
            source.display()
        )
    })?;
    source_db
        .busy_timeout(SQLITE_BACKUP_TIMEOUT)
        .map_err(|error| {
            format!(
                "failed to configure SQLite snapshot timeout for {}: {error}",
                source.display()
            )
        })?;

    let mut target_db = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| {
        format!(
            "failed to create SQLite snapshot {}: {error}",
            target.display()
        )
    })?;
    target_db
        .busy_timeout(SQLITE_BACKUP_TIMEOUT)
        .map_err(|error| {
            format!(
                "failed to configure SQLite snapshot target {}: {error}",
                target.display()
            )
        })?;

    let backup = Backup::new(&source_db, &mut target_db).map_err(|error| {
        format!(
            "failed to initialize SQLite snapshot for {}: {error}",
            source.display()
        )
    })?;
    let deadline = Instant::now() + SQLITE_BACKUP_TIMEOUT;
    loop {
        let status = backup.step(SQLITE_BACKUP_PAGES_PER_STEP).map_err(|error| {
            format!(
                "failed to copy SQLite database {}: {error}",
                source.display()
            )
        })?;
        match status {
            StepResult::Done => break,
            StepResult::More => {}
            StepResult::Busy | StepResult::Locked => sleep(SQLITE_BACKUP_RETRY_DELAY),
            _ => {
                return Err(format!(
                    "SQLite backup returned an unsupported status for {}",
                    source.display()
                ));
            }
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "timed out snapshotting SQLite database {}",
                source.display()
            ));
        }
    }
    drop(backup);

    let mut quick_check = target_db
        .prepare("PRAGMA quick_check")
        .map_err(|error| format!("failed to verify SQLite snapshot: {error}"))?;
    let rows = quick_check
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("failed to verify SQLite snapshot: {error}"))?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|error| format!("failed to verify SQLite snapshot: {error}"))?);
    }
    if results.as_slice() != ["ok"] {
        return Err(format!(
            "SQLite snapshot integrity check failed for {}: {}",
            source.display(),
            results.join("; ")
        ));
    }
    drop(quick_check);
    target_db
        .close()
        .map_err(|(_, error)| format!("failed to close SQLite snapshot: {error}"))?;
    source_db
        .close()
        .map_err(|(_, error)| format!("failed to close SQLite source: {error}"))?;

    File::open(target)
        .and_then(|file| file.sync_all())
        .map_err(|error| {
            format!(
                "failed to sync SQLite snapshot {}: {error}",
                target.display()
            )
        })
}
