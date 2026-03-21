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
