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

pub const ENV_ARCHIVE_MANIFEST_PATH: &str = "meta/env.json";
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
pub struct EnvArchiveManifest {
    pub kind: String,
    pub format_version: u32,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub env: ArchivedEnvMeta,
}

pub struct ExtractedEnvArchive<T> {
    pub manifest: T,
    pub root_dir: PathBuf,
}

pub fn write_env_archive<T: Serialize>(
    manifest: &T,
    source_root: &Path,
    output_path: &Path,
) -> Result<(), String> {
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let file = File::create(output_path).map_err(|error| error.to_string())?;
    let mut builder = Builder::new(file);
    let mut manifest_raw =
        serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    manifest_raw.push('\n');
    let manifest_bytes = manifest_raw.into_bytes();
    let mut header = Header::new_gnu();
    header.set_size(manifest_bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(
            &mut header,
            ENV_ARCHIVE_MANIFEST_PATH,
            Cursor::new(manifest_bytes),
        )
        .map_err(|error| error.to_string())?;
    builder
        .append_dir_all(ENV_ARCHIVE_ROOT_DIR, source_root)
        .map_err(|error| error.to_string())?;
    builder.finish().map_err(|error| error.to_string())
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

    let manifest_path = staging_dir.join(ENV_ARCHIVE_MANIFEST_PATH);
    let root_dir = staging_dir.join(ENV_ARCHIVE_ROOT_DIR);
    if !manifest_path.exists() {
        return Err("archive is missing meta/env.json".to_string());
    }
    if !root_dir.exists() {
        return Err("archive is missing root/".to_string());
    }

    let manifest_raw = fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?;
    let manifest = serde_json::from_str(&manifest_raw).map_err(|error| error.to_string())?;
    Ok(ExtractedEnvArchive { manifest, root_dir })
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

    use super::{ArchivedEnvMeta, EnvArchiveManifest, extract_env_archive, write_env_archive};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_path(label: &str) -> PathBuf {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir()
            .join("ocm-archive-tests")
            .join(format!("{label}-{}-{id}", std::process::id()))
    }

    #[test]
    fn env_archives_round_trip_manifest_and_root_contents() {
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
        let manifest = EnvArchiveManifest {
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

        write_env_archive(&manifest, &source_root, &archive_path).unwrap();
        let extracted =
            extract_env_archive::<EnvArchiveManifest>(&archive_path, &extract_dir).unwrap();

        assert_eq!(extracted.manifest, manifest);
        assert_eq!(
            fs::read_to_string(extracted.root_dir.join(".openclaw/workspace/notes.txt")).unwrap(),
            "hello archive"
        );

        let _ = fs::remove_dir_all(source_root);
        let _ = fs::remove_dir_all(extract_dir);
        let _ = fs::remove_file(archive_path);
    }
}
