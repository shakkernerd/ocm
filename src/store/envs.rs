use std::collections::BTreeMap;
use std::fs;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::env::{
    CloneEnvironmentOptions, CreateEnvironmentOptions, EnvExportSummary, EnvImportSummary,
    EnvMarker, EnvMarkerRepairSummary, EnvMeta, ExportEnvironmentOptions, ImportEnvironmentOptions,
};
use crate::infra::archive::{
    ArchivedEnvMeta, EnvArchiveManifest, extract_env_archive, write_env_archive,
};

use super::common::{
    copy_dir_recursive, ensure_dir, load_json_files, path_exists, read_json, write_json,
};
use super::layout::{
    clean_path, default_env_root, derive_env_paths, display_path, env_meta_path,
    resolve_absolute_path, validate_name,
};
use super::now_utc;
use super::{clear_nonportable_runtime_state, rewrite_openclaw_config_for_target};

static NEXT_IMPORT_ID: AtomicU64 = AtomicU64::new(0);

pub fn list_environments(
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<EnvMeta>, String> {
    let stores = super::ensure_store(env, cwd)?;
    let files = load_json_files(&stores.envs_dir)?;
    let mut out: Vec<EnvMeta> = Vec::with_capacity(files.len());
    for file in files {
        out.push(read_json(&file)?);
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(out)
}

pub fn get_environment(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let safe_name = validate_name(name, "Environment name")?;
    let path = env_meta_path(&safe_name, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!("environment \"{safe_name}\" does not exist"));
    }
    read_json(&path)
}

pub fn save_environment(
    mut meta: EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let safe_name = validate_name(&meta.name, "Environment name")?;
    meta.name = safe_name;
    meta.kind = "ocm-env".to_string();
    meta.root = display_path(&clean_path(Path::new(&meta.root)));
    meta.updated_at = now_utc();

    let path = env_meta_path(&meta.name, env, cwd)?;
    write_json(&path, &meta)?;
    Ok(meta)
}

pub fn repair_environment_marker(
    name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMarkerRepairSummary, String> {
    let meta = get_environment(name, env, cwd)?;
    let paths = derive_env_paths(Path::new(&meta.root));
    if !path_exists(&paths.root) {
        return Err(format!(
            "environment root does not exist: {}",
            display_path(&paths.root)
        ));
    }

    let marker = EnvMarker {
        kind: "ocm-env-marker".to_string(),
        name: meta.name.clone(),
        created_at: now_utc(),
    };
    write_json(&paths.marker_path, &marker)?;

    Ok(EnvMarkerRepairSummary {
        env_name: meta.name,
        root: meta.root,
        marker_path: display_path(&paths.marker_path),
    })
}

pub fn create_environment(
    options: CreateEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let name = validate_name(&options.name, "Environment name")?;
    let meta_path = env_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("environment \"{name}\" already exists"));
    }

    let root = if let Some(root) = options.root.as_deref() {
        resolve_absolute_path(root, env, cwd)?
    } else {
        default_env_root(&name, env, cwd)?
    };

    let paths = derive_env_paths(&root);
    if path_exists(&paths.root) {
        let mut entries = fs::read_dir(&paths.root).map_err(|error| error.to_string())?;
        if entries.next().is_some() {
            return Err(format!(
                "root already exists and is not empty: {}",
                display_path(&paths.root)
            ));
        }
    }

    ensure_dir(&paths.root)?;
    ensure_dir(&paths.state_dir)?;
    ensure_dir(&paths.workspace_dir)?;

    let created_at = now_utc();
    let marker = EnvMarker {
        kind: "ocm-env-marker".to_string(),
        name: name.clone(),
        created_at,
    };
    write_json(&paths.marker_path, &marker)?;

    let meta = EnvMeta {
        kind: "ocm-env".to_string(),
        name,
        root: display_path(&paths.root),
        gateway_port: options.gateway_port,
        default_runtime: options.default_runtime,
        default_launcher: options.default_launcher,
        protected: options.protected,
        created_at,
        updated_at: created_at,
        last_used_at: None,
    };
    save_environment(meta, env, cwd)
}

pub fn clone_environment(
    options: CloneEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let source = get_environment(&options.source_name, env, cwd)?;
    let name = validate_name(&options.name, "Environment name")?;
    let meta_path = env_meta_path(&name, env, cwd)?;
    if path_exists(&meta_path) {
        return Err(format!("environment \"{name}\" already exists"));
    }

    let root = if let Some(root) = options.root.as_deref() {
        resolve_absolute_path(root, env, cwd)?
    } else {
        default_env_root(&name, env, cwd)?
    };
    let target_paths = derive_env_paths(&root);
    if path_exists(&target_paths.root) {
        let mut entries = fs::read_dir(&target_paths.root).map_err(|error| error.to_string())?;
        if entries.next().is_some() {
            return Err(format!(
                "root already exists and is not empty: {}",
                display_path(&target_paths.root)
            ));
        }
    }

    let source_paths = derive_env_paths(Path::new(&source.root));
    if !path_exists(&source_paths.root) {
        return Err(format!(
            "environment root does not exist: {}",
            display_path(&source_paths.root)
        ));
    }
    if !path_exists(&source_paths.marker_path) {
        return Err(format!(
            "refusing to clone {} without {}",
            display_path(&source_paths.root),
            source_paths
                .marker_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(".ocm-env.json")
        ));
    }

    let result = (|| {
        copy_dir_recursive(&source_paths.root, &target_paths.root)?;
        let created_at = now_utc();
        let gateway_port = choose_cloned_gateway_port(&source, env, cwd)?;
        let marker = EnvMarker {
            kind: "ocm-env-marker".to_string(),
            name: name.clone(),
            created_at,
        };
        write_json(&target_paths.marker_path, &marker)?;
        rewrite_openclaw_config_for_target(
            &target_paths,
            Some(&source_paths.root),
            Some(gateway_port),
        )?;
        clear_nonportable_runtime_state(&target_paths)?;

        let meta = EnvMeta {
            kind: "ocm-env".to_string(),
            name,
            root: display_path(&target_paths.root),
            gateway_port: Some(gateway_port),
            default_runtime: source.default_runtime,
            default_launcher: source.default_launcher,
            protected: source.protected,
            created_at,
            updated_at: created_at,
            last_used_at: None,
        };
        save_environment(meta, env, cwd)
    })();

    if result.is_err() {
        let _ = fs::remove_file(&meta_path);
        let _ = fs::remove_dir_all(&target_paths.root);
    }

    result
}

fn choose_cloned_gateway_port(
    source: &EnvMeta,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<u32, String> {
    let mut envs = list_environments(env, cwd)?;
    envs.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.name.cmp(&right.name))
    });

    let mut claimed = std::collections::BTreeSet::new();
    let mut effective_ports = std::collections::BTreeMap::new();

    for meta in &envs {
        if let Some(port) = meta.gateway_port.or_else(|| read_config_gateway_port(meta)) {
            claimed.insert(port);
            effective_ports.insert(meta.name.clone(), port);
        }
    }

    for meta in &envs {
        if effective_ports.contains_key(&meta.name) {
            continue;
        }

        let mut port = 18_789;
        while claimed.contains(&port) {
            port = port.saturating_add(1);
        }
        claimed.insert(port);
        effective_ports.insert(meta.name.clone(), port);
    }

    let preferred = effective_ports
        .get(&source.name)
        .copied()
        .or_else(|| {
            source
                .gateway_port
                .or_else(|| read_config_gateway_port(source))
        })
        .unwrap_or(18_789);

    let mut port = preferred.max(18_789);
    while claimed.contains(&port) || !gateway_port_available(port) {
        claimed.insert(port);
        port = port.saturating_add(1);
    }

    Ok(port)
}

fn read_config_gateway_port(meta: &EnvMeta) -> Option<u32> {
    let config_path = derive_env_paths(Path::new(&meta.root)).config_path;
    let raw = fs::read_to_string(config_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let port = value.get("gateway")?.get("port")?.as_u64()?;
    if (1..=u16::MAX as u64).contains(&port) {
        Some(port as u32)
    } else {
        None
    }
}

fn gateway_port_available(port: u32) -> bool {
    TcpListener::bind(("127.0.0.1", port as u16)).is_ok()
}

pub fn export_environment(
    options: ExportEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvExportSummary, String> {
    let meta = get_environment(&options.name, env, cwd)?;
    let env_paths = derive_env_paths(Path::new(&meta.root));
    if !path_exists(&env_paths.root) {
        return Err(format!(
            "environment root does not exist: {}",
            display_path(&env_paths.root)
        ));
    }
    if !path_exists(&env_paths.marker_path) {
        return Err(format!(
            "refusing to export {} without {}",
            display_path(&env_paths.root),
            env_paths
                .marker_path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or(".ocm-env.json")
        ));
    }

    let output_path = if let Some(output) = options.output.as_deref() {
        resolve_absolute_path(output, env, cwd)?
    } else {
        clean_path(&cwd.join(format!("{}.ocm-env.tar", meta.name)))
    };
    if path_exists(&output_path) {
        return Err(format!(
            "export archive already exists: {}",
            display_path(&output_path)
        ));
    }

    let manifest = EnvArchiveManifest {
        kind: "ocm-env-archive".to_string(),
        format_version: 1,
        exported_at: now_utc(),
        env: ArchivedEnvMeta {
            name: meta.name.clone(),
            source_root: Some(meta.root.clone()),
            gateway_port: meta.gateway_port,
            default_runtime: meta.default_runtime.clone(),
            default_launcher: meta.default_launcher.clone(),
            protected: meta.protected,
            created_at: meta.created_at,
            updated_at: meta.updated_at,
            last_used_at: meta.last_used_at,
        },
    };

    let result = write_env_archive(&manifest, &env_paths.root, &output_path);
    if result.is_err() {
        let _ = fs::remove_file(&output_path);
    }
    result?;

    Ok(EnvExportSummary {
        name: meta.name,
        root: meta.root,
        archive_path: display_path(&output_path),
        default_runtime: meta.default_runtime,
        default_launcher: meta.default_launcher,
        protected: meta.protected,
    })
}

pub fn import_environment(
    options: ImportEnvironmentOptions,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvImportSummary, String> {
    let archive_path = resolve_absolute_path(&options.archive, env, cwd)?;
    let staging_dir = import_staging_dir();
    if path_exists(&staging_dir) {
        let _ = fs::remove_dir_all(&staging_dir);
    }

    let result = (|| {
        let extracted = extract_env_archive::<EnvArchiveManifest>(&archive_path, &staging_dir)?;
        if extracted.manifest.kind != "ocm-env-archive" {
            return Err(format!(
                "unsupported archive kind: {}",
                extracted.manifest.kind
            ));
        }
        if extracted.manifest.format_version != 1 {
            return Err(format!(
                "unsupported archive format version: {}",
                extracted.manifest.format_version
            ));
        }

        let source_name = extracted.manifest.env.name.clone();
        let name = if let Some(name) = options.name.as_deref() {
            validate_name(name, "Environment name")?
        } else {
            validate_name(&source_name, "Environment name")?
        };
        let meta_path = env_meta_path(&name, env, cwd)?;
        if path_exists(&meta_path) {
            return Err(format!("environment \"{name}\" already exists"));
        }

        let root = if let Some(root) = options.root.as_deref() {
            resolve_absolute_path(root, env, cwd)?
        } else {
            default_env_root(&name, env, cwd)?
        };
        let target_paths = derive_env_paths(&root);
        if path_exists(&target_paths.root) {
            let mut entries =
                fs::read_dir(&target_paths.root).map_err(|error| error.to_string())?;
            if entries.next().is_some() {
                return Err(format!(
                    "root already exists and is not empty: {}",
                    display_path(&target_paths.root)
                ));
            }
        }

        if !path_exists(&extracted.root_dir) {
            return Err("archive is missing root/".to_string());
        }
        if !path_exists(&extracted.root_dir.join(".ocm-env.json")) {
            return Err("archive environment root is missing .ocm-env.json".to_string());
        }

        let imported = (|| {
            copy_dir_recursive(&extracted.root_dir, &target_paths.root)?;
            rewrite_openclaw_config_for_target(
                &target_paths,
                extracted.manifest.env.source_root.as_deref().map(Path::new),
                extracted.manifest.env.gateway_port,
            )?;

            let created_at = now_utc();
            let marker = EnvMarker {
                kind: "ocm-env-marker".to_string(),
                name: name.clone(),
                created_at,
            };
            write_json(&target_paths.marker_path, &marker)?;

            let meta = EnvMeta {
                kind: "ocm-env".to_string(),
                name: name.clone(),
                root: display_path(&target_paths.root),
                gateway_port: extracted.manifest.env.gateway_port,
                default_runtime: extracted.manifest.env.default_runtime.clone(),
                default_launcher: extracted.manifest.env.default_launcher.clone(),
                protected: extracted.manifest.env.protected,
                created_at,
                updated_at: created_at,
                last_used_at: None,
            };
            save_environment(meta, env, cwd)
        })();

        match imported {
            Ok(meta) => Ok(EnvImportSummary {
                name: meta.name.clone(),
                source_name,
                root: meta.root.clone(),
                archive_path: display_path(&archive_path),
                default_runtime: meta.default_runtime.clone(),
                default_launcher: meta.default_launcher.clone(),
                protected: meta.protected,
            }),
            Err(error) => {
                let _ = fs::remove_file(&meta_path);
                let _ = fs::remove_dir_all(&target_paths.root);
                Err(error)
            }
        }
    })();

    let _ = fs::remove_dir_all(&staging_dir);
    result
}

pub fn remove_environment(
    name: &str,
    force: bool,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<EnvMeta, String> {
    let meta = get_environment(name, env, cwd)?;
    if meta.protected && !force {
        return Err(format!(
            "environment \"{}\" is protected; re-run with --force",
            meta.name
        ));
    }

    let paths = derive_env_paths(Path::new(&meta.root));
    let root_exists = path_exists(&paths.root);
    let marker_exists = path_exists(&paths.marker_path);

    if root_exists && !marker_exists && !force {
        let marker_name = paths
            .marker_path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or(".ocm-env.json");
        return Err(format!(
            "refusing to delete {} without {}; re-run with --force",
            display_path(&paths.root),
            marker_name
        ));
    }

    if root_exists {
        fs::remove_dir_all(&paths.root).map_err(|error| error.to_string())?;
    }

    let meta_path = env_meta_path(&meta.name, env, cwd)?;
    match fs::remove_file(meta_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }

    Ok(meta)
}

fn import_staging_dir() -> PathBuf {
    let id = NEXT_IMPORT_ID.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join("ocm-env-imports")
        .join(format!("{}-{id}", std::process::id()))
}
