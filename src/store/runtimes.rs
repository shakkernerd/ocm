use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::infra::download::{
    artifact_file_name_from_url, download_to_file, file_sha256, normalize_sha256,
    verify_file_integrity, verify_file_sha256,
};
use crate::runtime::releases::{
    load_official_openclaw_releases, load_release_manifest, normalize_openclaw_channel_selector,
    official_openclaw_releases_url, select_official_openclaw_release_by_channel,
    select_official_openclaw_release_by_version, select_release, OpenClawRelease,
};
use crate::runtime::{
    AddRuntimeOptions, InstallRuntimeFromOfficialReleaseOptions, InstallRuntimeFromReleaseOptions,
    InstallRuntimeFromUrlOptions, InstallRuntimeOptions, RuntimeMeta, RuntimeReleaseSelectorKind,
    RuntimeSourceKind,
};

use super::common::{ensure_dir, load_json_files, path_exists, read_json, write_json};
use super::layout::{
    clean_path, display_path, resolve_absolute_path, runtime_install_files_dir,
    runtime_install_root, runtime_meta_path, validate_name,
};
use super::now_utc;

const INTERNAL_NPM_BIN_ENV: &str = "OCM_INTERNAL_NPM_BIN";

fn trim_description(description: Option<String>) -> Option<String> {
    description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn configured_npm_bin(env: &BTreeMap<String, String>) -> &str {
    env.get(INTERNAL_NPM_BIN_ENV)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("npm")
}

fn installed_openclaw_binary_path(install_files: &Path) -> PathBuf {
    install_files.join("node_modules/openclaw/openclaw.mjs")
}

fn summarize_command_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    for bytes in [stderr, stdout] {
        let text = String::from_utf8_lossy(bytes);
        if let Some(line) = text.lines().find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        }) {
            return Some(line.to_string());
        }
    }
    None
}

fn install_openclaw_package_with_npm(
    archive_path: &Path,
    install_files: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let npm_bin = configured_npm_bin(env);
    let output = Command::new(npm_bin)
        .arg("install")
        .arg("--prefix")
        .arg(install_files)
        .arg("--omit=dev")
        .arg("--no-save")
        .arg("--package-lock=false")
        .arg(archive_path)
        .env("npm_config_fund", "false")
        .env("npm_config_audit", "false")
        .env("npm_config_update_notifier", "false")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            format!(
                "failed to run {npm_bin} while installing the OpenClaw package: {error}"
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let detail = summarize_command_output(&output.stdout, &output.stderr)
        .unwrap_or_else(|| format!("{npm_bin} exited with code {}", output.status.code().unwrap_or(1)));
    Err(format!(
        "failed to install OpenClaw package dependencies with {npm_bin}: {detail}"
    ))
}

fn build_installed_runtime_meta(
    name: String,
    binary_path: &Path,
    install_root: &Path,
    source_path: Option<&Path>,
    source_url: Option<String>,
    source_manifest_url: Option<String>,
    source_sha256: Option<String>,
    release_version: Option<String>,
    release_channel: Option<String>,
    release_selector_kind: Option<RuntimeReleaseSelectorKind>,
    release_selector_value: Option<String>,
    description: Option<String>,
) -> RuntimeMeta {
    let created_at = now_utc();
    RuntimeMeta {
        kind: "ocm-runtime".to_string(),
        name,
        binary_path: display_path(binary_path),
        source_kind: RuntimeSourceKind::Installed,
        source_path: source_path.map(display_path),
        source_url,
        source_manifest_url,
        source_sha256,
        release_version,
        release_channel,
        release_selector_kind,
        release_selector_value,
        install_root: Some(display_path(install_root)),
        description,
        created_at,
        updated_at: created_at,
    }
}

fn copy_installed_runtime_binary(source_path: &Path, binary_path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(source_path).map_err(|error| error.to_string())?;
    fs::copy(source_path, binary_path).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        let permissions = metadata.permissions();
        fs::set_permissions(binary_path, permissions).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn install_runtime_at_path(
    name: String,
    meta_path: PathBuf,
    install_root: PathBuf,
    install_files: PathBuf,
    file_name: &Path,
    source_path: Option<&Path>,
    source_url: Option<String>,
    source_manifest_url: Option<String>,
    source_sha256: Option<String>,
    release_version: Option<String>,
    release_channel: Option<String>,
    release_selector_kind: Option<RuntimeReleaseSelectorKind>,
    release_selector_value: Option<String>,
    description: Option<String>,
) -> Result<RuntimeMeta, String> {
    if path_exists(&install_root) {
        return Err(format!(
            "runtime install root already exists: {}",
            display_path(&install_root)
        ));
    }

    let result = (|| {
        ensure_dir(&install_files)?;
        let binary_path = install_files.join(file_name);
        match (source_path, source_url.as_deref()) {
            (Some(source_path), _) => copy_installed_runtime_binary(source_path, &binary_path)?,
            (None, Some(source_url)) => {
                download_to_file(source_url, &binary_path)?;
                if let Some(source_sha256) = source_sha256.as_deref() {
                    verify_file_sha256(&binary_path, source_sha256)?;
                }
                #[cfg(unix)]
                {
                    let mut permissions = fs::metadata(&binary_path)
                        .map_err(|error| error.to_string())?
                        .permissions();
                    permissions.set_mode(0o755);
                    fs::set_permissions(&binary_path, permissions)
                        .map_err(|error| error.to_string())?;
                }
            }
            (None, None) => return Err("runtime install requires a source path or URL".to_string()),
        }

        let meta = build_installed_runtime_meta(
            name,
            &binary_path,
            &install_root,
            source_path,
            source_url,
            source_manifest_url,
            source_sha256,
            release_version,
            release_channel,
            release_selector_kind,
            release_selector_value,
            description,
        );
        write_json(&meta_path, &meta)?;
        Ok(meta)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_dir_all(&install_root);
    }

    result
}

fn install_runtime_from_openclaw_package(
    name: String,
    meta_path: PathBuf,
    install_root: PathBuf,
    install_files: PathBuf,
    tarball_url: String,
    source_manifest_url: String,
    source_integrity: Option<String>,
    release_version: String,
    release_channel: Option<String>,
    release_selector_kind: Option<RuntimeReleaseSelectorKind>,
    release_selector_value: Option<String>,
    description: Option<String>,
    env: &BTreeMap<String, String>,
) -> Result<RuntimeMeta, String> {
    if path_exists(&install_root) {
        return Err(format!(
            "runtime install root already exists: {}",
            display_path(&install_root)
        ));
    }

    let result = (|| {
        ensure_dir(&install_files)?;
        let archive_name = artifact_file_name_from_url(&tarball_url)?;
        let archive_path = install_files.join(&archive_name);
        download_to_file(&tarball_url, &archive_path)?;
        if let Some(source_integrity) = source_integrity.as_deref() {
            verify_file_integrity(&archive_path, source_integrity)?;
        }

        install_openclaw_package_with_npm(&archive_path, &install_files, env)?;
        let _ = fs::remove_file(&archive_path);

        let binary_path = installed_openclaw_binary_path(&install_files);
        if !path_exists(&binary_path) {
            return Err(format!(
                "OpenClaw release \"{release_version}\" is missing node_modules/openclaw/openclaw.mjs after installation"
            ));
        }
        #[cfg(unix)]
        {
            let mut permissions = fs::metadata(&binary_path)
                .map_err(|error| error.to_string())?
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&binary_path, permissions).map_err(|error| error.to_string())?;
        }
        let binary_sha256 = file_sha256(&binary_path)?;

        let meta = build_installed_runtime_meta(
            name,
            &binary_path,
            &install_root,
            None,
            Some(tarball_url),
            Some(source_manifest_url),
            Some(binary_sha256),
            Some(release_version),
            release_channel,
            release_selector_kind,
            release_selector_value,
            description,
        );
        write_json(&meta_path, &meta)?;
        Ok(meta)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_dir_all(&install_root);
    }

    result
}

fn prepare_runtime_meta_path(
    name: &str,
    replace_existing: bool,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    let meta_path = runtime_meta_path(name, env, cwd)?;
    if path_exists(&meta_path) {
        if !replace_existing {
            return Err(format!("runtime \"{name}\" already exists"));
        }
        remove_runtime(name, env, cwd)?;
    }
    Ok(meta_path)
}

pub fn list_runtimes(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<RuntimeMeta>, String> {
    let stores = super::ensure_store(env, cwd)?;
    let files = load_json_files(&stores.runtimes_dir)?;
    let mut out: Vec<RuntimeMeta> = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

pub fn get_runtime(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let safe_name = validate_name(name, "Runtime name")?;
    let path = runtime_meta_path(&safe_name, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!("runtime \"{safe_name}\" does not exist"));
    }
    read_json(&path)
}

pub fn get_runtime_verified(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    verify_runtime_binary(get_runtime(name, env, cwd)?)
}

pub fn add_runtime(
    options: AddRuntimeOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = runtime_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("runtime \"{name}\" already exists"));
    }

    let raw_path = options.path.trim();
    if raw_path.is_empty() {
        return Err("runtime path is required".to_string());
    }

    let binary_path = resolve_absolute_path(raw_path, env, cwd)?;
    if !path_exists(&binary_path) {
        return Err(format!(
            "runtime path does not exist: {}",
            display_path(&binary_path)
        ));
    }

    let metadata = fs::metadata(&binary_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!(
            "runtime path must be a file: {}",
            display_path(&binary_path)
        ));
    }

    let description = trim_description(options.description);

    let created_at = now_utc();
    let meta = RuntimeMeta {
        kind: "ocm-runtime".to_string(),
        name,
        binary_path: display_path(&binary_path),
        source_kind: RuntimeSourceKind::Registered,
        source_path: Some(display_path(&binary_path)),
        source_url: None,
        source_manifest_url: None,
        source_sha256: None,
        release_version: None,
        release_channel: None,
        release_selector_kind: None,
        release_selector_value: None,
        install_root: None,
        description,
        created_at,
        updated_at: created_at,
    };
    write_json(&meta_path, &meta)?;
    Ok(meta)
}

pub fn remove_runtime(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let meta = get_runtime(name, env, cwd)?;
    let path = runtime_meta_path(&meta.name, env, cwd)?;
    if let Some(install_root) = meta.install_root.as_deref() {
        let expected_root = runtime_install_root(&meta.name, env, cwd)?;
        if clean_path(Path::new(install_root)) == expected_root && path_exists(&expected_root) {
            fs::remove_dir_all(&expected_root).map_err(|error| error.to_string())?;
        }
    }
    fs::remove_file(path).map_err(|error| error.to_string())?;
    Ok(meta)
}

pub fn install_runtime(
    options: InstallRuntimeOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = prepare_runtime_meta_path(&name, options.force, env, cwd)?;

    let raw_path = options.path.trim();
    if raw_path.is_empty() {
        return Err("runtime path is required".to_string());
    }

    let source_path = resolve_absolute_path(raw_path, env, cwd)?;
    if !path_exists(&source_path) {
        return Err(format!(
            "runtime path does not exist: {}",
            display_path(&source_path)
        ));
    }

    let metadata = fs::metadata(&source_path).map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(format!(
            "runtime path must be a file: {}",
            display_path(&source_path)
        ));
    }

    let file_name = source_path.file_name().ok_or_else(|| {
        format!(
            "runtime path must include a file name: {}",
            display_path(&source_path)
        )
    })?;
    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    install_runtime_at_path(
        name,
        meta_path,
        install_root,
        install_files,
        Path::new(file_name),
        Some(&source_path),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        trim_description(options.description),
    )
}

pub fn install_runtime_from_url(
    options: InstallRuntimeFromUrlOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = prepare_runtime_meta_path(&name, options.force, env, cwd)?;

    let file_name = artifact_file_name_from_url(&options.url)?;
    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    install_runtime_at_path(
        name,
        meta_path,
        install_root,
        install_files,
        Path::new(&file_name),
        None,
        Some(options.url),
        None,
        None,
        None,
        None,
        None,
        None,
        trim_description(options.description),
    )
}

pub fn install_runtime_from_release(
    options: InstallRuntimeFromReleaseOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = prepare_runtime_meta_path(&name, options.force, env, cwd)?;

    let manifest = load_release_manifest(&options.manifest_url)?;
    let (release_selector_kind, release_selector_value) =
        match (options.version.as_deref(), options.channel.as_deref()) {
            (Some(version), None) => (
                Some(RuntimeReleaseSelectorKind::Version),
                Some(version.trim().to_string()),
            ),
            (None, Some(channel)) => (
                Some(RuntimeReleaseSelectorKind::Channel),
                Some(channel.trim().to_string()),
            ),
            _ => (None, None),
        };
    let release = select_release(
        &manifest,
        options.version.as_deref(),
        options.channel.as_deref(),
    )?;
    let description =
        trim_description(options.description).or_else(|| trim_description(release.description));

    let file_name = artifact_file_name_from_url(&release.url)?;
    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    install_runtime_at_path(
        name,
        meta_path,
        install_root,
        install_files,
        Path::new(&file_name),
        None,
        Some(release.url),
        Some(options.manifest_url),
        release.sha256,
        Some(release.version),
        release.channel,
        release_selector_kind,
        release_selector_value,
        description,
    )
}

pub fn install_runtime_from_official_openclaw_release(
    options: InstallRuntimeFromOfficialReleaseOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let channel = options
        .channel
        .as_deref()
        .map(normalize_openclaw_channel_selector)
        .transpose()?;

    let releases_url = official_openclaw_releases_url(env);
    let releases = load_official_openclaw_releases(&releases_url)?;
    let (release_selector_kind, release_selector_value) =
        match (options.version.as_deref(), channel.as_deref()) {
            (Some(version), None) => (
                Some(RuntimeReleaseSelectorKind::Version),
                Some(version.trim().to_string()),
            ),
            (None, Some(channel)) => (
                Some(RuntimeReleaseSelectorKind::Channel),
                Some(channel.trim().to_string()),
            ),
            _ => (None, None),
        };
    let release = match (options.version.as_deref(), channel.as_deref()) {
        (Some(version), None) => select_official_openclaw_release_by_version(&releases, version)?,
        (None, Some(channel)) => select_official_openclaw_release_by_channel(&releases, channel)?,
        (Some(_), Some(_)) => {
            return Err("runtime install accepts only one of --version or --channel".to_string());
        }
        (None, None) => {
            return Err("runtime install requires --version or --channel".to_string());
        }
    };
    let description = trim_description(options.description)
        .or_else(|| Some(format!("Official OpenClaw release {}", release.version)));

    install_runtime_from_selected_official_openclaw_release(
        name,
        options.force,
        releases_url,
        release,
        release_selector_kind,
        release_selector_value,
        description,
        env,
        cwd,
    )
}

pub fn install_runtime_from_selected_official_openclaw_release(
    name: String,
    force: bool,
    releases_url: String,
    release: OpenClawRelease,
    release_selector_kind: Option<RuntimeReleaseSelectorKind>,
    release_selector_value: Option<String>,
    description: Option<String>,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let meta_path = prepare_runtime_meta_path(&name, force, env, cwd)?;
    let description =
        trim_description(description).or_else(|| Some(format!("Official OpenClaw release {}", release.version)));

    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    install_runtime_from_openclaw_package(
        name,
        meta_path,
        install_root,
        install_files,
        release.tarball_url,
        releases_url,
        release.integrity,
        release.version,
        release.channel,
        release_selector_kind,
        release_selector_value,
        description,
        env,
    )
}

pub fn runtime_integrity_issue(meta: &RuntimeMeta) -> Option<String> {
    let binary_path = Path::new(&meta.binary_path);
    if !path_exists(binary_path) {
        return Some(format!(
            "binary path does not exist: {}",
            display_path(binary_path)
        ));
    }

    let metadata = match fs::metadata(binary_path) {
        Ok(metadata) => metadata,
        Err(error) => return Some(error.to_string()),
    };
    if !metadata.is_file() {
        return Some(format!(
            "binary path is not a file: {}",
            display_path(binary_path)
        ));
    }

    let Some(expected_sha256) = meta.source_sha256.as_deref() else {
        return None;
    };
    let expected_sha256 = match normalize_sha256(expected_sha256) {
        Ok(value) => value,
        Err(error) => return Some(error),
    };
    let actual_sha256 = match file_sha256(binary_path) {
        Ok(value) => value,
        Err(error) => return Some(error),
    };
    if actual_sha256 != expected_sha256 {
        return Some(format!(
            "sha256 mismatch: expected {expected_sha256}, got {actual_sha256}"
        ));
    }

    None
}

pub fn verify_runtime_binary(meta: RuntimeMeta) -> Result<RuntimeMeta, String> {
    if let Some(issue) = runtime_integrity_issue(&meta) {
        return Err(format!("runtime \"{}\" {issue}", meta.name));
    }

    Ok(meta)
}
