use std::collections::BTreeMap;
use std::path::Path;

use crate::paths::derive_env_paths;
use crate::types::EnvMeta;

pub fn quote_posix(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn quote_fish(value: &str) -> String {
    format!("'{}'", value.replace('\'', "\\'"))
}

fn render_assignment(shell: &str, key: &str, value: &str) -> String {
    if shell == "fish" {
        format!("set -gx {key} {};", quote_fish(value))
    } else {
        format!("export {key}={}", quote_posix(value))
    }
}

fn render_unset(shell: &str, key: &str) -> String {
    if shell == "fish" {
        format!("set -e {key};")
    } else {
        format!("unset {key}")
    }
}

pub fn resolve_shell_name(explicit: Option<&str>, env: &BTreeMap<String, String>) -> String {
    if let Some(shell) = explicit {
        match shell.trim().to_ascii_lowercase().as_str() {
            "bash" | "fish" | "sh" | "zsh" => return shell.trim().to_ascii_lowercase(),
            _ => {}
        }
    }

    if env
        .get("SHELL")
        .map(|value| value.contains("fish"))
        .unwrap_or(false)
    {
        "fish".to_string()
    } else {
        "zsh".to_string()
    }
}

pub fn build_openclaw_env(
    meta: &EnvMeta,
    base_env: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let paths = derive_env_paths(Path::new(&meta.root));
    let mut next = base_env.clone();
    next.insert(
        "OPENCLAW_HOME".to_string(),
        paths.openclaw_home.to_string_lossy().into_owned(),
    );
    next.insert(
        "OPENCLAW_STATE_DIR".to_string(),
        paths.state_dir.to_string_lossy().into_owned(),
    );
    next.insert(
        "OPENCLAW_CONFIG_PATH".to_string(),
        paths.config_path.to_string_lossy().into_owned(),
    );
    next.insert("OCM_ACTIVE_ENV".to_string(), meta.name.clone());
    next.insert(
        "OCM_ACTIVE_ENV_ROOT".to_string(),
        paths.root.to_string_lossy().into_owned(),
    );

    if let Some(port) = meta.gateway_port {
        next.insert("OPENCLAW_GATEWAY_PORT".to_string(), port.to_string());
    } else {
        next.remove("OPENCLAW_GATEWAY_PORT");
    }

    next.remove("OPENCLAW_PROFILE");
    next
}

pub fn render_use_script(meta: &EnvMeta, shell: &str) -> String {
    let paths = derive_env_paths(Path::new(&meta.root));
    let mut lines = vec![
        render_unset(shell, "OPENCLAW_PROFILE"),
        render_assignment(
            shell,
            "OPENCLAW_HOME",
            &paths.openclaw_home.to_string_lossy(),
        ),
        render_assignment(
            shell,
            "OPENCLAW_STATE_DIR",
            &paths.state_dir.to_string_lossy(),
        ),
        render_assignment(
            shell,
            "OPENCLAW_CONFIG_PATH",
            &paths.config_path.to_string_lossy(),
        ),
        render_assignment(shell, "OCM_ACTIVE_ENV", &meta.name),
        render_assignment(shell, "OCM_ACTIVE_ENV_ROOT", &paths.root.to_string_lossy()),
    ];

    if let Some(port) = meta.gateway_port {
        lines.push(render_assignment(
            shell,
            "OPENCLAW_GATEWAY_PORT",
            &port.to_string(),
        ));
    }

    format!("{}\n", lines.join("\n"))
}

fn render_init_posix(command: &str) -> String {
    let command = quote_posix(command);
    format!(
        "ocm_use() {{\n  script=\"$(command {command} env use \"$@\")\" || return $?\n  eval \"$script\"\n}}\n"
    )
}

fn render_init_fish(command: &str) -> String {
    let command = quote_fish(command);
    format!(
        "function ocm_use\n    set -l script (command {command} env use $argv)\n    or return $status\n    eval $script\nend\n"
    )
}

pub fn render_init_script(command: &str, shell: &str) -> Result<String, String> {
    match shell {
        "bash" | "sh" | "zsh" => Ok(render_init_posix(command)),
        "fish" => Ok(render_init_fish(command)),
        _ => Err(format!("unsupported shell: {shell}")),
    }
}
