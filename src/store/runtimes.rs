use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::host::verify_official_openclaw_runtime_host;
use crate::infra::download::{
    artifact_file_name_from_url, download_to_file, file_sha256, normalize_sha256,
    verify_file_integrity, verify_file_sha256,
};
use crate::managed_node::managed_runtime_install_command;
use crate::runtime::releases::{
    OpenClawRelease, load_official_openclaw_releases, load_release_manifest,
    normalize_openclaw_channel_selector, official_openclaw_releases_url,
    select_official_openclaw_release_by_channel, select_official_openclaw_release_by_version,
    select_release,
};
use crate::runtime::{
    AddRuntimeOptions, InstallRuntimeFromOfficialReleaseOptions, InstallRuntimeFromReleaseOptions,
    InstallRuntimeFromUrlOptions, InstallRuntimeOptions, RuntimeMeta, RuntimeReleaseSelectorKind,
    RuntimeSourceKind, is_official_openclaw_package_runtime, is_openclaw_package_runtime,
};

use super::common::{
    copy_dir_recursive, ensure_dir, load_json_files, path_exists, read_json, write_json,
};
use super::layout::{
    clean_path, display_path, resolve_absolute_path, runtime_install_files_dir,
    runtime_install_root, runtime_meta_path, validate_name,
};
use super::now_utc;

fn trim_description(description: Option<String>) -> Option<String> {
    description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn installed_openclaw_binary_path(install_files: &Path) -> PathBuf {
    install_files.join("node_modules/openclaw/openclaw.mjs")
}

fn installed_openclaw_package_root(install_files: &Path) -> PathBuf {
    install_files.join("node_modules/openclaw")
}

fn openclaw_package_root_from_binary(binary_path: &Path) -> Option<PathBuf> {
    binary_path
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| *value == "openclaw.mjs")?;
    let package_root = binary_path.parent()?.to_path_buf();
    if package_root.file_name().and_then(|value| value.to_str()) != Some("openclaw") {
        return None;
    }
    if package_root
        .parent()
        .and_then(|value| value.file_name())
        .and_then(|value| value.to_str())
        != Some("node_modules")
    {
        return None;
    }
    Some(package_root)
}

fn symlink_or_copy_dir(source: &Path, target: &Path) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        ensure_dir(parent)?;
    }
    #[cfg(unix)]
    {
        match std::os::unix::fs::symlink(source, target) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(_) => {}
        }
    }
    #[cfg(windows)]
    {
        match std::os::windows::fs::symlink_dir(source, target) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(_) => {}
        }
    }
    copy_dir_recursive(source, target)
}

fn expose_openclaw_package_runtime_dependencies(install_files: &Path) -> Result<(), String> {
    let package_root = installed_openclaw_package_root(install_files);
    if !package_root.join("package.json").exists() {
        return Ok(());
    }

    let prefix_node_modules = install_files.join("node_modules");
    let package_node_modules = package_root.join("node_modules");
    let entries = match fs::read_dir(&prefix_node_modules) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let source_dir = entry.path();
        let package_name = entry.file_name().to_string_lossy().to_string();
        if package_name == "openclaw" || package_name == ".bin" || package_name.starts_with('.') {
            continue;
        }
        if package_name.starts_with('@') {
            if !source_dir.is_dir() {
                continue;
            }
            for scoped_entry in fs::read_dir(&source_dir).map_err(|error| error.to_string())? {
                let scoped_entry = scoped_entry.map_err(|error| error.to_string())?;
                let scoped_source_dir = scoped_entry.path();
                if !scoped_source_dir.join("package.json").exists() {
                    continue;
                }
                let scoped_name = scoped_entry.file_name().to_string_lossy().to_string();
                let target_dir = package_node_modules.join(&package_name).join(scoped_name);
                if !target_dir.exists() {
                    symlink_or_copy_dir(&scoped_source_dir, &target_dir)?;
                }
            }
            continue;
        }
        if !source_dir.join("package.json").exists() {
            continue;
        }
        let target_dir = package_node_modules.join(package_name);
        if !target_dir.exists() {
            symlink_or_copy_dir(&source_dir, &target_dir)?;
        }
    }
    Ok(())
}

fn openclaw_package_runtime_dependency_layout_issue(package_root: &Path) -> Option<String> {
    if !package_root.join("package.json").exists() {
        return None;
    }
    let Some(prefix_node_modules) = package_root.parent() else {
        return Some(format!(
            "OpenClaw package runtime has no node_modules parent: {}",
            display_path(package_root)
        ));
    };
    let package_node_modules = package_root.join("node_modules");
    let entries = match fs::read_dir(prefix_node_modules) {
        Ok(entries) => entries,
        Err(error) => {
            return Some(format!(
                "failed to inspect OpenClaw package runtime dependencies at {}: {error}",
                display_path(prefix_node_modules)
            ));
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                return Some(format!(
                    "failed to inspect OpenClaw package runtime dependency entry: {error}"
                ));
            }
        };
        let source_dir = entry.path();
        let package_name = entry.file_name().to_string_lossy().to_string();
        if package_name == "openclaw" || package_name == ".bin" || package_name.starts_with('.') {
            continue;
        }
        if package_name.starts_with('@') {
            if !source_dir.is_dir() {
                continue;
            }
            let scoped_entries = match fs::read_dir(&source_dir) {
                Ok(entries) => entries,
                Err(error) => {
                    return Some(format!(
                        "failed to inspect OpenClaw package runtime scoped dependencies at {}: {error}",
                        display_path(&source_dir)
                    ));
                }
            };
            for scoped_entry in scoped_entries {
                let scoped_entry = match scoped_entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        return Some(format!(
                            "failed to inspect OpenClaw package runtime scoped dependency entry: {error}"
                        ));
                    }
                };
                let scoped_source_dir = scoped_entry.path();
                if !scoped_source_dir.join("package.json").exists() {
                    continue;
                }
                let scoped_name = scoped_entry.file_name().to_string_lossy().to_string();
                let target_dir = package_node_modules.join(&package_name).join(scoped_name);
                if !target_dir.join("package.json").exists() {
                    return Some(format!(
                        "OpenClaw package runtime dependency layout is missing {}",
                        display_path(&target_dir.join("package.json"))
                    ));
                }
            }
            continue;
        }
        if !source_dir.join("package.json").exists() {
            continue;
        }
        let target_dir = package_node_modules.join(package_name);
        if !target_dir.join("package.json").exists() {
            return Some(format!(
                "OpenClaw package runtime dependency layout is missing {}",
                display_path(&target_dir.join("package.json"))
            ));
        }
    }
    None
}

#[derive(Clone, Debug, Default)]
struct RuntimeSourceDetails {
    path: Option<PathBuf>,
    url: Option<String>,
    manifest_url: Option<String>,
    sha256: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RuntimeReleaseDetails {
    version: Option<String>,
    channel: Option<String>,
    selector_kind: Option<RuntimeReleaseSelectorKind>,
    selector_value: Option<String>,
}

impl RuntimeReleaseDetails {
    pub(crate) fn with_selector(
        selector_kind: Option<RuntimeReleaseSelectorKind>,
        selector_value: Option<String>,
    ) -> Self {
        Self {
            selector_kind,
            selector_value,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug)]
struct RuntimeInstallTarget {
    name: String,
    meta_path: PathBuf,
    install_root: PathBuf,
    install_files: PathBuf,
}

#[derive(Clone, Copy)]
pub(crate) struct InstallContext<'a> {
    pub env: &'a BTreeMap<String, String>,
    pub cwd: &'a Path,
}

#[derive(Clone, Debug)]
pub struct BuildLocalRuntimeOptions {
    pub name: String,
    pub repo: String,
    pub description: Option<String>,
    pub force: bool,
}

fn summarize_command_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let mut fallback = Vec::new();
    for bytes in [stderr, stdout] {
        let text = String::from_utf8_lossy(bytes);
        let meaningful = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| !line.starts_with("npm notice"))
            .filter(|line| !line.starts_with("npm warn"))
            .take(12)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if !meaningful.is_empty() {
            return Some(meaningful.join("\n"));
        }
        fallback.extend(
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .take(3)
                .map(ToOwned::to_owned),
        );
    }
    (!fallback.is_empty()).then(|| fallback.join("\n"))
}

fn npm_program(env: &BTreeMap<String, String>) -> String {
    env.get("OCM_INTERNAL_NPM_BIN")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("npm")
        .to_string()
}

fn install_openclaw_package_with_npm(
    archive_path: &Path,
    install_files: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<(), String> {
    let host_ready = verify_official_openclaw_runtime_host(env).is_ok();
    let install_command = if host_ready {
        crate::managed_node::CommandSpec {
            program: npm_program(env),
            args: Vec::new(),
        }
    } else {
        managed_runtime_install_command(env, cwd)?
    };

    let output = Command::new(&install_command.program)
        .args(&install_command.args)
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
                "failed to run {} while installing the OpenClaw package: {error}",
                install_command.program
            )
        })?;

    if output.status.success() {
        return Ok(());
    }

    let detail = summarize_command_output(&output.stdout, &output.stderr).unwrap_or_else(|| {
        format!(
            "{} exited with code {}",
            install_command.program,
            output.status.code().unwrap_or(1)
        )
    });
    Err(format!(
        "failed to install OpenClaw package dependencies with {}: {detail}",
        install_command.program
    ))
}

fn load_openclaw_repo_version(repo_path: &Path) -> Result<String, String> {
    let package_json_path = repo_path.join("package.json");
    let raw = fs::read_to_string(&package_json_path).map_err(|error| {
        format!(
            "failed to read OpenClaw package.json at {}: {error}",
            display_path(&package_json_path)
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        format!(
            "failed to parse OpenClaw package.json at {}: {error}",
            display_path(&package_json_path)
        )
    })?;

    let package_name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    if package_name != "openclaw" {
        return Err(format!(
            "local runtime build requires an OpenClaw repo package named \"openclaw\"; found \"{package_name}\""
        ));
    }

    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "OpenClaw package.json is missing a non-empty version".to_string())
}

fn git_short_commit(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("rev-parse")
        .arg("--short=7")
        .arg("HEAD")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn default_local_build_description(version: &str, commit: Option<&str>) -> String {
    match commit {
        Some(commit) => format!("Local OpenClaw build {version} ({commit})"),
        None => format!("Local OpenClaw build {version}"),
    }
}

fn pack_local_openclaw_repo(
    repo_path: &Path,
    pack_dir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    ensure_dir(pack_dir)?;
    let npm = npm_program(env);
    let output = Command::new(&npm)
        .arg("pack")
        .arg("--pack-destination")
        .arg(pack_dir)
        .env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0")
        .env("npm_config_fund", "false")
        .env("npm_config_audit", "false")
        .env("npm_config_update_notifier", "false")
        .current_dir(repo_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            format!(
                "failed to run npm pack for local OpenClaw build in {}: {error}",
                display_path(repo_path)
            )
        })?;

    if !output.status.success() {
        let detail =
            summarize_command_output(&output.stdout, &output.stderr).unwrap_or_else(|| {
                format!(
                    "npm pack exited with code {}",
                    output.status.code().unwrap_or(1)
                )
            });
        return Err(format!("failed to pack local OpenClaw build: {detail}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().rev() {
        let trimmed = line.trim();
        if trimmed.ends_with(".tgz") {
            let candidate = pack_dir.join(trimmed);
            if path_exists(&candidate) {
                return Ok(candidate);
            }
        }
    }

    let mut archives = fs::read_dir(pack_dir)
        .map_err(|error| error.to_string())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("tgz"))
        .collect::<Vec<_>>();
    archives.sort();
    match archives.len() {
        1 => Ok(archives.remove(0)),
        0 => Err("npm pack did not produce an OpenClaw package archive".to_string()),
        _ => Err(format!(
            "npm pack produced multiple package archives in {}; expected one",
            display_path(pack_dir)
        )),
    }
}

fn build_installed_runtime_meta(
    target: &RuntimeInstallTarget,
    binary_path: &Path,
    source: &RuntimeSourceDetails,
    release: &RuntimeReleaseDetails,
    description: Option<String>,
) -> RuntimeMeta {
    let created_at = now_utc();
    RuntimeMeta {
        kind: "ocm-runtime".to_string(),
        name: target.name.clone(),
        binary_path: display_path(binary_path),
        source_kind: RuntimeSourceKind::Installed,
        source_path: source.path.as_deref().map(display_path),
        source_url: source.url.clone(),
        source_manifest_url: source.manifest_url.clone(),
        source_sha256: source.sha256.clone(),
        release_version: release.version.clone(),
        release_channel: release.channel.clone(),
        release_selector_kind: release.selector_kind.clone(),
        release_selector_value: release.selector_value.clone(),
        install_root: Some(display_path(&target.install_root)),
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
    target: RuntimeInstallTarget,
    file_name: &Path,
    source: RuntimeSourceDetails,
    release: RuntimeReleaseDetails,
    description: Option<String>,
) -> Result<RuntimeMeta, String> {
    if path_exists(&target.install_root) {
        return Err(format!(
            "runtime install root already exists: {}",
            display_path(&target.install_root)
        ));
    }

    let result = (|| {
        ensure_dir(&target.install_files)?;
        let binary_path = target.install_files.join(file_name);
        match (source.path.as_deref(), source.url.as_deref()) {
            (Some(source_path), _) => copy_installed_runtime_binary(source_path, &binary_path)?,
            (None, Some(source_url)) => {
                download_to_file(source_url, &binary_path)?;
                if let Some(source_sha256) = source.sha256.as_deref() {
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

        let meta =
            build_installed_runtime_meta(&target, &binary_path, &source, &release, description);
        write_json(&target.meta_path, &meta)?;
        Ok(meta)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&target.meta_path);
        let _ = fs::remove_dir_all(&target.install_root);
    }

    result
}

fn prepare_runtime_install_target(
    name: String,
    replace_existing: bool,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeInstallTarget, String> {
    let meta_path = prepare_runtime_meta_path(&name, replace_existing, env, cwd)?;
    let install_root = runtime_install_root(&name, env, cwd)?;
    let install_files = runtime_install_files_dir(&name, env, cwd)?;
    Ok(RuntimeInstallTarget {
        name,
        meta_path,
        install_root,
        install_files,
    })
}

fn install_runtime_from_openclaw_package(
    target: RuntimeInstallTarget,
    source: RuntimeSourceDetails,
    source_integrity: Option<String>,
    release: RuntimeReleaseDetails,
    description: Option<String>,
    context: InstallContext<'_>,
) -> Result<RuntimeMeta, String> {
    if path_exists(&target.install_root) {
        return Err(format!(
            "runtime install root already exists: {}",
            display_path(&target.install_root)
        ));
    }
    let result = (|| {
        ensure_dir(&target.install_files)?;
        let tarball_url = source.url.as_deref().ok_or_else(|| {
            "official OpenClaw runtime install requires a tarball URL".to_string()
        })?;
        let archive_name = artifact_file_name_from_url(tarball_url)?;
        let archive_path = target.install_files.join(&archive_name);
        download_to_file(tarball_url, &archive_path)?;
        if let Some(source_integrity) = source_integrity.as_deref() {
            verify_file_integrity(&archive_path, source_integrity)?;
        }

        let meta = install_runtime_from_openclaw_package_archive(
            &target,
            &archive_path,
            source,
            release,
            description,
            context,
        );
        let _ = fs::remove_file(&archive_path);
        meta
    })();

    if result.is_err() {
        let _ = fs::remove_file(&target.meta_path);
        let _ = fs::remove_dir_all(&target.install_root);
    }

    result
}

fn install_runtime_from_openclaw_package_archive(
    target: &RuntimeInstallTarget,
    archive_path: &Path,
    source: RuntimeSourceDetails,
    release: RuntimeReleaseDetails,
    description: Option<String>,
    context: InstallContext<'_>,
) -> Result<RuntimeMeta, String> {
    install_openclaw_package_with_npm(
        archive_path,
        &target.install_files,
        context.cwd,
        context.env,
    )?;
    expose_openclaw_package_runtime_dependencies(&target.install_files)?;

    let binary_path = installed_openclaw_binary_path(&target.install_files);
    if !path_exists(&binary_path) {
        let release_version = release.version.as_deref().unwrap_or("unknown");
        return Err(format!(
            "OpenClaw package \"{release_version}\" is missing node_modules/openclaw/openclaw.mjs after installation"
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

    let mut source = source;
    source.sha256 = Some(binary_sha256);
    let meta = build_installed_runtime_meta(target, &binary_path, &source, &release, description);
    write_json(&target.meta_path, &meta)?;
    Ok(meta)
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
    verify_runtime_binary(get_runtime(name, env, cwd)?, env)
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
    let target = prepare_runtime_install_target(name, options.force, env, cwd)?;

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

    let file_name = PathBuf::from(source_path.file_name().ok_or_else(|| {
        format!(
            "runtime path must include a file name: {}",
            display_path(&source_path)
        )
    })?);
    install_runtime_at_path(
        target,
        &file_name,
        RuntimeSourceDetails {
            path: Some(source_path),
            ..RuntimeSourceDetails::default()
        },
        RuntimeReleaseDetails::default(),
        trim_description(options.description),
    )
}

pub fn install_runtime_from_url(
    options: InstallRuntimeFromUrlOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let target = prepare_runtime_install_target(name, options.force, env, cwd)?;

    let file_name = artifact_file_name_from_url(&options.url)?;
    install_runtime_at_path(
        target,
        Path::new(&file_name),
        RuntimeSourceDetails {
            url: Some(options.url),
            ..RuntimeSourceDetails::default()
        },
        RuntimeReleaseDetails::default(),
        trim_description(options.description),
    )
}

pub(crate) fn install_runtime_from_local_openclaw_build(
    options: BuildLocalRuntimeOptions,
    context: InstallContext<'_>,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let meta_path = runtime_meta_path(&name, context.env, context.cwd)?;
    if path_exists(&meta_path) && !options.force {
        return Err(format!("runtime \"{name}\" already exists"));
    }

    let raw_repo = options.repo.trim();
    if raw_repo.is_empty() {
        return Err("OpenClaw repo path is required".to_string());
    }
    let repo_path = resolve_absolute_path(raw_repo, context.env, context.cwd)?;
    let metadata = fs::metadata(&repo_path).map_err(|error| {
        format!(
            "OpenClaw repo path does not exist: {} ({error})",
            display_path(&repo_path)
        )
    })?;
    if !metadata.is_dir() {
        return Err(format!(
            "OpenClaw repo path must be a directory: {}",
            display_path(&repo_path)
        ));
    }

    let repo_path = fs::canonicalize(&repo_path).map_err(|error| error.to_string())?;
    let version = load_openclaw_repo_version(&repo_path)?;
    let commit = git_short_commit(&repo_path);
    let stores = super::ensure_store(context.env, context.cwd)?;
    let pack_dir = stores.runtimes_dir.join(format!(
        ".{name}.pack-{}-{}",
        std::process::id(),
        now_utc().unix_timestamp_nanos()
    ));
    let _ = fs::remove_dir_all(&pack_dir);

    let result = (|| {
        let archive_path = pack_local_openclaw_repo(&repo_path, &pack_dir, context.env)?;
        let target = prepare_runtime_install_target(name, options.force, context.env, context.cwd)?;
        if path_exists(&target.install_root) {
            return Err(format!(
                "runtime install root already exists: {}",
                display_path(&target.install_root)
            ));
        }
        ensure_dir(&target.install_files)?;
        let description = trim_description(options.description)
            .or_else(|| Some(default_local_build_description(&version, commit.as_deref())));
        let meta = install_runtime_from_openclaw_package_archive(
            &target,
            &archive_path,
            RuntimeSourceDetails {
                path: Some(repo_path),
                ..RuntimeSourceDetails::default()
            },
            RuntimeReleaseDetails {
                version: Some(version),
                ..RuntimeReleaseDetails::default()
            },
            description,
            context,
        );
        if meta.is_err() {
            let _ = fs::remove_file(&target.meta_path);
            let _ = fs::remove_dir_all(&target.install_root);
        }
        meta
    })();

    let _ = fs::remove_dir_all(&pack_dir);
    result
}

pub fn install_runtime_from_release(
    options: InstallRuntimeFromReleaseOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<RuntimeMeta, String> {
    let name = validate_name(&options.name, "Runtime name")?;
    let target = prepare_runtime_install_target(name, options.force, env, cwd)?;

    let manifest = load_release_manifest(&options.manifest_url)?;
    let release = select_release(
        &manifest,
        options.version.as_deref(),
        options.channel.as_deref(),
    )?;
    let (selector_kind, selector_value) =
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
    let release_details = RuntimeReleaseDetails {
        version: Some(release.version.clone()),
        channel: release.channel.clone(),
        selector_kind,
        selector_value,
    };
    let description =
        trim_description(options.description).or_else(|| trim_description(release.description));

    let file_name = artifact_file_name_from_url(&release.url)?;
    install_runtime_at_path(
        target,
        Path::new(&file_name),
        RuntimeSourceDetails {
            url: Some(release.url),
            manifest_url: Some(options.manifest_url),
            sha256: release.sha256,
            ..RuntimeSourceDetails::default()
        },
        release_details,
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
        RuntimeReleaseDetails {
            selector_kind: release_selector_kind,
            selector_value: release_selector_value,
            ..RuntimeReleaseDetails::default()
        },
        description,
        InstallContext { env, cwd },
    )
}

pub(crate) fn install_runtime_from_selected_official_openclaw_release(
    name: String,
    force: bool,
    releases_url: String,
    release: OpenClawRelease,
    release_details: RuntimeReleaseDetails,
    description: Option<String>,
    context: InstallContext<'_>,
) -> Result<RuntimeMeta, String> {
    let target = prepare_runtime_install_target(name, force, context.env, context.cwd)?;
    let description = trim_description(description)
        .or_else(|| Some(format!("Official OpenClaw release {}", release.version)));
    let source_integrity = release.integrity.clone();
    let source = RuntimeSourceDetails {
        url: Some(release.tarball_url),
        manifest_url: Some(releases_url),
        ..RuntimeSourceDetails::default()
    };
    let release = RuntimeReleaseDetails {
        version: Some(release.version),
        channel: release.channel,
        selector_kind: release_details.selector_kind,
        selector_value: release_details.selector_value,
    };
    install_runtime_from_openclaw_package(
        target,
        source,
        source_integrity,
        release,
        description,
        context,
    )
}

pub fn runtime_integrity_issue(
    meta: &RuntimeMeta,
    env: &BTreeMap<String, String>,
) -> Option<String> {
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

    let expected_sha256 = meta.source_sha256.as_deref()?;
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

    if is_official_openclaw_package_runtime(meta, env) || is_openclaw_package_runtime(meta) {
        if let Some(package_root) = openclaw_package_root_from_binary(binary_path) {
            return openclaw_package_runtime_dependency_layout_issue(&package_root);
        }
    }

    None
}

pub fn verify_runtime_binary(
    meta: RuntimeMeta,
    env: &BTreeMap<String, String>,
) -> Result<RuntimeMeta, String> {
    if let Some(issue) = runtime_integrity_issue(&meta, env) {
        return Err(format!("runtime \"{}\" {issue}", meta.name));
    }

    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::summarize_command_output;

    #[test]
    fn command_summary_prefers_errors_over_npm_warnings() {
        let stderr = br#"
npm warn deprecated node-domexception@1.0.0: Use your platform's native DOMException instead
npm error code 1
npm error command failed
npm error Error [ERR_MODULE_NOT_FOUND]: Cannot find module './missing.mjs'
"#;

        let summary = summarize_command_output(b"", stderr).unwrap();

        assert!(summary.contains("npm error code 1"));
        assert!(summary.contains("ERR_MODULE_NOT_FOUND"));
        assert!(!summary.contains("deprecated node-domexception"));
    }

    #[test]
    fn command_summary_falls_back_to_warnings_when_no_errors_exist() {
        let stderr = br#"
npm warn deprecated node-domexception@1.0.0: Use your platform's native DOMException instead
"#;

        let summary = summarize_command_output(b"", stderr).unwrap();

        assert!(summary.contains("deprecated node-domexception"));
    }
}
