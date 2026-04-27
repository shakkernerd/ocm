use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::infra::archive::{extract_tar_gz, extract_zip};
use crate::infra::download::download_to_file;
use crate::store::{display_path, resolve_store_paths};

pub const OPENCLAW_MIN_NODE_VERSION: &str = "22.14.0";

const INTERNAL_MANAGED_NODE_ARCHIVE_URL_ENV: &str = "OCM_INTERNAL_MANAGED_NODE_ARCHIVE_URL";
const NODE_DIST_BASE_URL: &str = "https://nodejs.org/dist";

#[derive(Clone, Debug)]
pub(crate) struct ManagedNodeToolchain {
    pub(crate) node_bin: PathBuf,
    pub(crate) npm_cli: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct ManagedNodeDistribution {
    version: String,
    asset_name: String,
    root_dir_name: String,
    archive_kind: ManagedNodeArchiveKind,
    node_relative_path: &'static str,
    npm_cli_relative_path: &'static str,
    platform_label: &'static str,
}

#[derive(Clone, Copy, Debug)]
enum ManagedNodeArchiveKind {
    TarGz,
    Zip,
}

#[derive(Clone, Debug)]
pub(crate) struct CommandSpec {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

pub(crate) fn managed_node_fallback_supported() -> bool {
    managed_node_distribution().is_ok()
}

pub(crate) fn managed_node_fallback_detail() -> Result<String, String> {
    let distribution = managed_node_distribution()?;
    Ok(format!(
        "OCM can install a private Node.js {} toolchain for official releases on {}",
        distribution.version, distribution.platform_label
    ))
}

pub(crate) fn load_existing_managed_node_toolchain(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Option<ManagedNodeToolchain>, String> {
    let distribution = match managed_node_distribution() {
        Ok(distribution) => distribution,
        Err(_) => return Ok(None),
    };
    let root = managed_node_root(&distribution, env, cwd)?;
    Ok(verify_managed_node_toolchain(&root, &distribution)
        .map(|(node_bin, npm_cli)| ManagedNodeToolchain { node_bin, npm_cli }))
}

pub(crate) fn ensure_managed_node_toolchain(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<ManagedNodeToolchain, String> {
    let distribution = managed_node_distribution()?;
    let root = managed_node_root(&distribution, env, cwd)?;
    if let Some((node_bin, npm_cli)) = verify_managed_node_toolchain(&root, &distribution) {
        return Ok(ManagedNodeToolchain { node_bin, npm_cli });
    }

    if root.exists() {
        fs::remove_dir_all(&root).map_err(|error| {
            format!(
                "failed to clear the incomplete managed Node.js toolchain at {}: {error}",
                display_path(&root)
            )
        })?;
    }

    let parent = root.parent().ok_or_else(|| {
        format!(
            "managed Node.js root has no parent: {}",
            display_path(&root)
        )
    })?;
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let stage_root = parent.join(format!(
        ".node-toolchain-{}-{}",
        std::process::id(),
        time::OffsetDateTime::now_utc().unix_timestamp_nanos()
    ));
    if stage_root.exists() {
        let _ = fs::remove_dir_all(&stage_root);
    }

    let result = (|| {
        fs::create_dir_all(&stage_root).map_err(|error| error.to_string())?;
        let archive_path = stage_root.join(&distribution.asset_name);
        download_to_file(&managed_node_archive_url(&distribution, env), &archive_path)?;

        let extract_root = stage_root.join("extract");
        match distribution.archive_kind {
            ManagedNodeArchiveKind::TarGz => extract_tar_gz(&archive_path, &extract_root)?,
            ManagedNodeArchiveKind::Zip => extract_zip(&archive_path, &extract_root)?,
        }

        let extracted_root = extract_root.join(&distribution.root_dir_name);
        let Some((node_bin, npm_cli)) =
            verify_managed_node_toolchain(&extracted_root, &distribution)
        else {
            return Err(format!(
                "managed Node.js archive did not contain the expected files for {}",
                distribution.platform_label
            ));
        };

        fs::rename(&extracted_root, &root).map_err(|error| {
            format!(
                "failed to place the managed Node.js toolchain in {}: {error}",
                display_path(&root)
            )
        })?;

        let node_bin = relocate_checked_path(&node_bin, &extracted_root, &root)?;
        let npm_cli = relocate_checked_path(&npm_cli, &extracted_root, &root)?;
        Ok(ManagedNodeToolchain { node_bin, npm_cli })
    })();

    let _ = fs::remove_dir_all(&stage_root);
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
}

pub(crate) fn managed_runtime_install_command(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<CommandSpec, String> {
    let toolchain = ensure_managed_node_toolchain(env, cwd)?;
    Ok(CommandSpec {
        program: display_path(&toolchain.node_bin),
        args: vec![display_path(&toolchain.npm_cli)],
    })
}

pub(crate) fn managed_runtime_launch_command(
    binary_path: &str,
    openclaw_args: &[String],
    env: &BTreeMap<String, String>,
    cwd: &Path,
    bootstrap: bool,
) -> Result<CommandSpec, String> {
    let toolchain = if bootstrap {
        ensure_managed_node_toolchain(env, cwd)?
    } else {
        load_existing_managed_node_toolchain(env, cwd)?.ok_or_else(|| {
            format!(
                "managed Node.js is not installed yet for OpenClaw package runtimes; rerun a release flow like \"{} start\" or \"{} runtime install --channel stable\"",
                command_example(env),
                command_example(env)
            )
        })?
    };
    let mut args = vec![binary_path.to_string()];
    args.extend(openclaw_args.iter().cloned());
    Ok(CommandSpec {
        program: display_path(&toolchain.node_bin),
        args,
    })
}

fn command_example(env: &BTreeMap<String, String>) -> String {
    env.get("OCM_SELF")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ocm")
        .to_string()
}

fn managed_node_distribution() -> Result<ManagedNodeDistribution, String> {
    let version = OPENCLAW_MIN_NODE_VERSION.to_string();
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => {
            return Err(format!(
                "OCM cannot install a private Node.js toolchain on this architecture yet: {other}"
            ));
        }
    };

    match std::env::consts::OS {
        "macos" => managed_node_distribution_for(
            &version,
            &format!("darwin-{arch}"),
            "macOS",
            ManagedNodeArchiveKind::TarGz,
            "bin/node",
            "lib/node_modules/npm/bin/npm-cli.js",
        ),
        "linux" => managed_node_distribution_for(
            &version,
            &format!("linux-{arch}"),
            "Linux",
            ManagedNodeArchiveKind::TarGz,
            "bin/node",
            "lib/node_modules/npm/bin/npm-cli.js",
        ),
        "windows" => managed_node_distribution_for(
            &version,
            &format!("win-{arch}"),
            "Windows",
            ManagedNodeArchiveKind::Zip,
            "node.exe",
            "node_modules/npm/bin/npm-cli.js",
        ),
        other => Err(format!(
            "OCM cannot install a private Node.js toolchain on this operating system yet: {other}"
        )),
    }
}

fn managed_node_distribution_for(
    version: &str,
    suffix: &str,
    platform_label: &'static str,
    archive_kind: ManagedNodeArchiveKind,
    node_relative_path: &'static str,
    npm_cli_relative_path: &'static str,
) -> Result<ManagedNodeDistribution, String> {
    let extension = match archive_kind {
        ManagedNodeArchiveKind::TarGz => "tar.gz",
        ManagedNodeArchiveKind::Zip => "zip",
    };
    let root_dir_name = format!("node-v{version}-{suffix}");
    Ok(ManagedNodeDistribution {
        version: version.to_string(),
        asset_name: format!("{root_dir_name}.{extension}"),
        root_dir_name,
        archive_kind,
        node_relative_path,
        npm_cli_relative_path,
        platform_label,
    })
}

fn managed_node_root(
    distribution: &ManagedNodeDistribution,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<PathBuf, String> {
    let stores = resolve_store_paths(env, cwd)?;
    Ok(stores
        .home
        .join("toolchains")
        .join("node")
        .join(&distribution.root_dir_name))
}

fn managed_node_archive_url(
    distribution: &ManagedNodeDistribution,
    env: &BTreeMap<String, String>,
) -> String {
    if let Some(override_url) = env
        .get(INTERNAL_MANAGED_NODE_ARCHIVE_URL_ENV)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return override_url.to_string();
    }

    format!(
        "{}/v{}/{}",
        NODE_DIST_BASE_URL, distribution.version, distribution.asset_name
    )
}

fn verify_managed_node_toolchain(
    root: &Path,
    distribution: &ManagedNodeDistribution,
) -> Option<(PathBuf, PathBuf)> {
    let node_bin = root.join(distribution.node_relative_path);
    let npm_cli = root.join(distribution.npm_cli_relative_path);
    (node_bin.is_file() && npm_cli.is_file()).then_some((node_bin, npm_cli))
}

fn relocate_checked_path(path: &Path, from_root: &Path, to_root: &Path) -> Result<PathBuf, String> {
    let relative = path
        .strip_prefix(from_root)
        .map_err(|error| error.to_string())?;
    Ok(to_root.join(relative))
}
