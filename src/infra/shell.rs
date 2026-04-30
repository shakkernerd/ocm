use std::collections::BTreeMap;
use std::path::Path;

use crate::env::EnvMeta;
use crate::store::derive_env_paths;

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
    let mut next = sanitized_openclaw_base_env(base_env);
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
    next.insert(
        "OPENCLAW_SERVICE_REPAIR_POLICY".to_string(),
        "external".to_string(),
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

fn sanitized_openclaw_base_env(base_env: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    base_env
        .iter()
        .filter(|(key, _)| {
            (!key.starts_with("OPENCLAW_") || is_openclaw_diagnostics_passthrough_key(key))
                && key.as_str() != "OCM_ACTIVE_ENV"
                && key.as_str() != "OCM_ACTIVE_ENV_ROOT"
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

fn is_openclaw_diagnostics_passthrough_key(key: &str) -> bool {
    matches!(
        key,
        "OPENCLAW_DIAGNOSTICS"
            | "OPENCLAW_DIAGNOSTICS_RUN_ID"
            | "OPENCLAW_DIAGNOSTICS_ENV"
            | "OPENCLAW_DIAGNOSTICS_TIMELINE_PATH"
            | "OPENCLAW_DIAGNOSTICS_EVENT_LOOP"
            | "OPENCLAW_GATEWAY_STARTUP_TRACE"
    )
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
        render_assignment(shell, "OPENCLAW_SERVICE_REPAIR_POLICY", "external"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::EnvMeta;
    use time::OffsetDateTime;

    #[test]
    fn build_openclaw_env_preserves_diagnostics_passthrough_only() {
        let meta = EnvMeta {
            kind: "ocm-env".to_string(),
            name: "demo".to_string(),
            root: "/tmp/ocm/envs/demo".to_string(),
            gateway_port: Some(19999),
            service_enabled: true,
            service_running: true,
            default_runtime: None,
            default_launcher: None,
            dev: None,
            protected: false,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
            last_used_at: None,
        };
        let base = BTreeMap::from([
            ("NODE_OPTIONS".to_string(), "--cpu-prof".to_string()),
            ("OPENCLAW_HOME".to_string(), "/tmp/wrong".to_string()),
            ("OPENCLAW_GATEWAY_PORT".to_string(), "12345".to_string()),
            ("OPENCLAW_PROFILE".to_string(), "wrong".to_string()),
            ("OPENCLAW_DIAGNOSTICS".to_string(), "timeline".to_string()),
            (
                "OPENCLAW_DIAGNOSTICS_TIMELINE_PATH".to_string(),
                "/tmp/kova/timeline.jsonl".to_string(),
            ),
            (
                "OPENCLAW_DIAGNOSTICS_EVENT_LOOP".to_string(),
                "1".to_string(),
            ),
            (
                "OPENCLAW_GATEWAY_STARTUP_TRACE".to_string(),
                "1".to_string(),
            ),
            (
                "OPENCLAW_RANDOM_USER_VALUE".to_string(),
                "strip-me".to_string(),
            ),
            ("OCM_ACTIVE_ENV".to_string(), "wrong".to_string()),
        ]);

        let env = build_openclaw_env(&meta, &base);

        assert_eq!(
            env.get("NODE_OPTIONS").map(String::as_str),
            Some("--cpu-prof")
        );
        assert_eq!(
            env.get("OPENCLAW_HOME").map(String::as_str),
            Some("/tmp/ocm/envs/demo")
        );
        assert_eq!(
            env.get("OPENCLAW_GATEWAY_PORT").map(String::as_str),
            Some("19999")
        );
        assert_eq!(
            env.get("OPENCLAW_DIAGNOSTICS").map(String::as_str),
            Some("timeline")
        );
        assert_eq!(
            env.get("OPENCLAW_DIAGNOSTICS_TIMELINE_PATH")
                .map(String::as_str),
            Some("/tmp/kova/timeline.jsonl")
        );
        assert_eq!(
            env.get("OPENCLAW_DIAGNOSTICS_EVENT_LOOP")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            env.get("OPENCLAW_GATEWAY_STARTUP_TRACE")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(env.get("OCM_ACTIVE_ENV").map(String::as_str), Some("demo"));
        assert!(!env.contains_key("OPENCLAW_PROFILE"));
        assert!(!env.contains_key("OPENCLAW_RANDOM_USER_VALUE"));
    }
}
