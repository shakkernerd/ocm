use std::fs::{self, File};
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tar::{Archive, Builder, Header};
use time::OffsetDateTime;
use zip::ZipArchive;

use crate::env::{default_service_enabled, default_service_running};

pub const ENV_ARCHIVE_METADATA_PATH: &str = "meta/env.json";
pub const ENV_ARCHIVE_ROOT_DIR: &str = "root";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedEnvMeta {
    pub name: String,
    #[serde(default)]
    pub source_root: Option<String>,
    pub gateway_port: Option<u32>,
    #[serde(default = "default_service_enabled")]
    pub service_enabled: bool,
    #[serde(default = "default_service_running")]
    pub service_running: bool,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvArchiveMetadata {
    pub kind: String,
    pub format_version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub env: ArchivedEnvMeta,
}

pub struct ExtractedEnvArchive<T> {
    pub metadata: T,
    pub root_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvArchiveEntryKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Clone, Copy)]
pub struct EnvArchiveOptions {
    pub should_skip_path: fn(&Path, EnvArchiveEntryKind) -> bool,
}

impl Default for EnvArchiveOptions {
    fn default() -> Self {
        Self {
            should_skip_path: include_env_archive_path,
        }
    }
}

fn include_env_archive_path(_relative_path: &Path, _kind: EnvArchiveEntryKind) -> bool {
    false
}

pub fn write_env_archive<T: Serialize>(
    metadata: &T,
    source_root: &Path,
    output_path: &Path,
) -> Result<(), String> {
    write_env_archive_with_options(
        metadata,
        source_root,
        output_path,
        EnvArchiveOptions::default(),
    )
}

pub fn write_env_archive_with_options<T: Serialize>(
    metadata: &T,
    source_root: &Path,
    output_path: &Path,
    options: EnvArchiveOptions,
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let file = File::create(output_path).map_err(|error| error.to_string())?;
    let mut builder = Builder::new(file);
    builder.follow_symlinks(false);
    let mut metadata_raw =
        serde_json::to_string_pretty(metadata).map_err(|error| error.to_string())?;
    metadata_raw.push('\n');
    let metadata_bytes = metadata_raw.into_bytes();
    let mut header = Header::new_gnu();
    header.set_size(metadata_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(
            &mut header,
            ENV_ARCHIVE_METADATA_PATH,
            Cursor::new(metadata_bytes),
        )
        .map_err(|error| error.to_string())?;
    append_env_root(&mut builder, source_root, options)?;
    builder.finish().map_err(|error| error.to_string())
}

fn append_env_root(
    builder: &mut Builder<File>,
    source_root: &Path,
    options: EnvArchiveOptions,
) -> Result<(), String> {
    builder
        .append_dir(ENV_ARCHIVE_ROOT_DIR, source_root)
        .map_err(|error| format!("failed to archive {}: {error}", source_root.display()))?;

    let mut stack = sorted_child_paths(source_root)?;
    while let Some(path) = stack.pop() {
        let relative_path = path.strip_prefix(source_root).map_err(|error| {
            format!(
                "failed to resolve archive path for {}: {error}",
                path.display()
            )
        })?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!("failed to inspect {} for archive: {error}", path.display())
        })?;
        let entry_kind = env_archive_entry_kind(&metadata);
        if (options.should_skip_path)(relative_path, entry_kind) {
            continue;
        }

        let archive_path = Path::new(ENV_ARCHIVE_ROOT_DIR).join(relative_path);
        if metadata.is_dir() {
            builder
                .append_dir(&archive_path, &path)
                .map_err(|error| format!("failed to archive {}: {error}", path.display()))?;
            let mut children = sorted_child_paths(&path)?;
            stack.append(&mut children);
            continue;
        }

        builder
            .append_path_with_name(&path, &archive_path)
            .map_err(|error| format!("failed to archive {}: {error}", path.display()))?;
    }

    Ok(())
}

fn sorted_child_paths(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut children = fs::read_dir(path)
        .map_err(|error| {
            format!(
                "failed to read directory {} for archive: {error}",
                path.display()
            )
        })?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    children.reverse();
    Ok(children)
}

fn env_archive_entry_kind(metadata: &fs::Metadata) -> EnvArchiveEntryKind {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        EnvArchiveEntryKind::Directory
    } else if file_type.is_file() {
        EnvArchiveEntryKind::File
    } else if file_type.is_symlink() {
        EnvArchiveEntryKind::Symlink
    } else {
        EnvArchiveEntryKind::Other
    }
}

pub fn extract_env_archive<T: DeserializeOwned>(
    archive_path: &Path,
    staging_dir: &Path,
) -> Result<ExtractedEnvArchive<T>, String> {
    fs::create_dir_all(staging_dir).map_err(|error| error.to_string())?;

    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = Archive::new(file);
    archive
        .unpack(staging_dir)
        .map_err(|error| error.to_string())?;

    let metadata_path = staging_dir.join(ENV_ARCHIVE_METADATA_PATH);
    let root_dir = staging_dir.join(ENV_ARCHIVE_ROOT_DIR);
    if !metadata_path.exists() {
        return Err("archive is missing meta/env.json".to_string());
    }
    if !root_dir.exists() {
        return Err("archive is missing root/".to_string());
    }

    let metadata_raw = fs::read_to_string(&metadata_path).map_err(|error| error.to_string())?;
    let metadata = serde_json::from_str(&metadata_raw).map_err(|error| error.to_string())?;
    Ok(ExtractedEnvArchive { metadata, root_dir })
}

pub fn extract_tar_gz(archive_path: &Path, destination_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(destination_dir).map_err(|error| error.to_string())?;

    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let decoder = GzDecoder::new(file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination_dir)
        .map_err(|error| error.to_string())
}

pub fn extract_zip(archive_path: &Path, destination_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(destination_dir).map_err(|error| error.to_string())?;

    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(relative_path) = entry.enclosed_name() else {
            continue;
        };
        let output_path = destination_dir.join(relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(|error| error.to_string())?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let mut output = File::create(&output_path).map_err(|error| error.to_string())?;
        io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = output
                .metadata()
                .map_err(|error| error.to_string())?
                .permissions();
            permissions.set_mode(mode);
            fs::set_permissions(&output_path, permissions).map_err(|error| error.to_string())?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{ArchivedEnvMeta, EnvArchiveMetadata, extract_env_archive, write_env_archive};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_path(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join("ocm-archive-tests")
            .join(format!("{label}-{}-{id}", std::process::id()))
    }

    #[test]
    fn env_archives_round_trip_metadata_and_root_contents() {
        let source_root = temp_path("source-root");
        let archive_path = temp_path("archives").join("demo.ocm-env.tar");
        let extract_dir = temp_path("extract");
        fs::create_dir_all(source_root.join(".openclaw/workspace")).unwrap();
        fs::write(
            source_root.join(".openclaw/workspace/notes.txt"),
            "hello archive",
        )
        .unwrap();

        let exported_at = time::OffsetDateTime::from_unix_timestamp(1_774_497_600).unwrap();
        let metadata = EnvArchiveMetadata {
            kind: "ocm-env-archive".to_string(),
            format_version: 1,
            exported_at,
            env: ArchivedEnvMeta {
                name: "demo".to_string(),
                source_root: Some(source_root.display().to_string()),
                gateway_port: Some(19789),
                service_enabled: true,
                service_running: true,
                default_runtime: Some("stable".to_string()),
                default_launcher: Some("shell".to_string()),
                protected: true,
                created_at: exported_at,
                updated_at: exported_at,
                last_used_at: None,
            },
        };

        write_env_archive(&metadata, &source_root, &archive_path).unwrap();
        let extracted =
            extract_env_archive::<EnvArchiveMetadata>(&archive_path, &extract_dir).unwrap();

        assert_eq!(extracted.metadata, metadata);
        assert_eq!(
            fs::read_to_string(extracted.root_dir.join(".openclaw/workspace/notes.txt")).unwrap(),
            "hello archive"
        );

        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(extract_dir);
        let _ = fs::remove_file(archive_path);
    }
}
