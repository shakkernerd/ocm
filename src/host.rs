use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::service::{ServiceManagerKind, service_manager_kind};
use crate::store::resolve_user_home;

const INTERNAL_NPM_BIN_ENV: &str = "OCM_INTERNAL_NPM_BIN";
pub const OPENCLAW_MIN_NODE_VERSION: &str = "22.14.0";
const CHROME_MCP_MIN_MAJOR: u32 = 144;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostDoctorSummary {
    pub healthy: bool,
    pub official_release_ready: bool,
    pub required_issues: usize,
    pub recommended_gaps: usize,
    pub checks: Vec<HostCheckSummary>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostCheckSummary {
    pub category: String,
    pub name: String,
    pub purpose: String,
    pub level: String,
    pub status: String,
    pub available: bool,
    pub version: Option<String>,
    pub detail: Option<String>,
    pub suggestion: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostCheckLevel {
    Required,
    Recommended,
    Optional,
}

impl HostCheckLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Required => "required",
            Self::Recommended => "recommended",
            Self::Optional => "optional",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum HostCheckStatus {
    Ok,
    Missing,
    Outdated,
    Unsupported,
}

impl HostCheckStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Missing => "missing",
            Self::Outdated => "outdated",
            Self::Unsupported => "unsupported",
        }
    }

    fn available(self) -> bool {
        matches!(self, Self::Ok | Self::Outdated)
    }
}

#[derive(Clone, Debug)]
struct HostCommandSummary {
    version: String,
}

pub fn doctor_host(env: &BTreeMap<String, String>) -> HostDoctorSummary {
    let checks = host_checks(env);
    let required_issues = checks
        .iter()
        .filter(|check| {
            check.level == HostCheckLevel::Required.as_str()
                && check.status != HostCheckStatus::Ok.as_str()
        })
        .count();
    let recommended_gaps = checks
        .iter()
        .filter(|check| {
            check.level == HostCheckLevel::Recommended.as_str()
                && check.status != HostCheckStatus::Ok.as_str()
                && check.status != HostCheckStatus::Unsupported.as_str()
        })
        .count();

    HostDoctorSummary {
        healthy: required_issues == 0,
        official_release_ready: required_issues == 0,
        required_issues,
        recommended_gaps,
        checks,
    }
}

pub fn official_openclaw_runtime_requirement_message(
    detail: &str,
    env: &BTreeMap<String, String>,
) -> String {
    format!(
        "official OpenClaw runtimes require Node.js >= {OPENCLAW_MIN_NODE_VERSION} and npm on PATH; {detail}. Run \"{} doctor host\" for a full machine check.",
        command_example(env)
    )
}

pub fn verify_official_openclaw_runtime_host(env: &BTreeMap<String, String>) -> Result<(), String> {
    let _ = npm_version(env)
        .map_err(|detail| official_openclaw_runtime_requirement_message(&detail, env))?;
    let node_version = node_version(env)
        .map_err(|detail| official_openclaw_runtime_requirement_message(&detail, env))?;
    match compare_version_like(&node_version, OPENCLAW_MIN_NODE_VERSION) {
        Some(std::cmp::Ordering::Less) => Err(official_openclaw_runtime_requirement_message(
            &format!(
                "found Node.js {node_version}; upgrade to {OPENCLAW_MIN_NODE_VERSION} or newer"
            ),
            env,
        )),
        Some(_) => Ok(()),
        None => Err(official_openclaw_runtime_requirement_message(
            &format!("node --version returned an unreadable version: {node_version}"),
            env,
        )),
    }
}

pub fn compare_version_like(left: &str, right: &str) -> Option<std::cmp::Ordering> {
    let left = parse_version_like(left)?;
    let right = parse_version_like(right)?;
    let max_len = left.len().max(right.len());
    for index in 0..max_len {
        let left_value = *left.get(index).unwrap_or(&0);
        let right_value = *right.get(index).unwrap_or(&0);
        match left_value.cmp(&right_value) {
            std::cmp::Ordering::Equal => {}
            ordering => return Some(ordering),
        }
    }
    Some(std::cmp::Ordering::Equal)
}

pub fn configured_npm_bin(env: &BTreeMap<String, String>) -> &str {
    env.get(INTERNAL_NPM_BIN_ENV)
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("npm")
}

fn command_example(env: &BTreeMap<String, String>) -> String {
    env.get("OCM_SELF")
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("ocm")
        .to_string()
}

fn host_checks(env: &BTreeMap<String, String>) -> Vec<HostCheckSummary> {
    let mut checks = vec![node_check(env), npm_check(env)];

    if cfg!(unix) {
        checks.push(python_check());
    }

    checks.extend([
        ffmpeg_check(),
        ffprobe_check(),
        openssl_check(),
        git_check(),
        pnpm_check(),
        bun_check(),
        chrome_check(env),
        service_manager_check(env),
    ]);

    checks
}

fn node_check(env: &BTreeMap<String, String>) -> HostCheckSummary {
    match node_version(env) {
        Ok(version) => match compare_version_like(&version, OPENCLAW_MIN_NODE_VERSION) {
            Some(std::cmp::Ordering::Less) => check(
                "official-release",
                "Node.js",
                "Run published OpenClaw releases",
                HostCheckLevel::Required,
                HostCheckStatus::Outdated,
                Some(version.clone()),
                Some(format!(
                    "found Node.js {version}; official releases need {OPENCLAW_MIN_NODE_VERSION} or newer"
                )),
                Some(format!(
                    "Install Node.js {OPENCLAW_MIN_NODE_VERSION} or newer."
                )),
            ),
            Some(_) => check(
                "official-release",
                "Node.js",
                "Run published OpenClaw releases",
                HostCheckLevel::Required,
                HostCheckStatus::Ok,
                Some(version),
                None,
                None,
            ),
            None => check(
                "official-release",
                "Node.js",
                "Run published OpenClaw releases",
                HostCheckLevel::Required,
                HostCheckStatus::Outdated,
                Some(version.clone()),
                Some(format!(
                    "node --version returned an unreadable version: {version}"
                )),
                Some(format!(
                    "Install Node.js {OPENCLAW_MIN_NODE_VERSION} or newer."
                )),
            ),
        },
        Err(detail) => check(
            "official-release",
            "Node.js",
            "Run published OpenClaw releases",
            HostCheckLevel::Required,
            HostCheckStatus::Missing,
            None,
            Some(detail),
            Some(format!(
                "Install Node.js {OPENCLAW_MIN_NODE_VERSION} or newer."
            )),
        ),
    }
}

fn npm_check(env: &BTreeMap<String, String>) -> HostCheckSummary {
    match npm_version(env) {
        Ok(version) => check(
            "official-release",
            "npm",
            "Install published OpenClaw releases",
            HostCheckLevel::Required,
            HostCheckStatus::Ok,
            Some(version),
            None,
            None,
        ),
        Err(detail) => check(
            "official-release",
            "npm",
            "Install published OpenClaw releases",
            HostCheckLevel::Required,
            HostCheckStatus::Missing,
            None,
            Some(detail),
            Some("Install npm or use a Node.js distribution that includes it.".to_string()),
        ),
    }
}

fn python_check() -> HostCheckSummary {
    match tool_version("python3", &["--version"]) {
        Ok(summary) => check(
            "common-features",
            "python3",
            "Support hardened file operations on macOS/Linux",
            HostCheckLevel::Recommended,
            HostCheckStatus::Ok,
            Some(summary.version),
            None,
            None,
        ),
        Err(detail) => check(
            "common-features",
            "python3",
            "Support hardened file operations on macOS/Linux",
            HostCheckLevel::Recommended,
            HostCheckStatus::Missing,
            None,
            Some(detail),
            Some("Install python3 in a system-managed location.".to_string()),
        ),
    }
}

fn ffmpeg_check() -> HostCheckSummary {
    tool_check(
        "common-features",
        "ffmpeg",
        "Audio and video conversion",
        HostCheckLevel::Recommended,
        "ffmpeg",
        &["-version"],
        Some("Install ffmpeg with your system package manager.".to_string()),
    )
}

fn ffprobe_check() -> HostCheckSummary {
    tool_check(
        "common-features",
        "ffprobe",
        "Media inspection and codec detection",
        HostCheckLevel::Recommended,
        "ffprobe",
        &["-version"],
        Some("Install ffmpeg with your system package manager.".to_string()),
    )
}

fn openssl_check() -> HostCheckSummary {
    tool_check(
        "common-features",
        "openssl",
        "Auto-generate gateway TLS certs",
        HostCheckLevel::Recommended,
        "openssl",
        &["version"],
        Some(
            "Install openssl in a trusted system location if you want gateway TLS auto-generation."
                .to_string(),
        ),
    )
}

fn git_check() -> HostCheckSummary {
    tool_check(
        "local-workflows",
        "git",
        "Source installs and git-based updates",
        HostCheckLevel::Recommended,
        "git",
        &["--version"],
        Some("Install git if you want source or git-channel workflows.".to_string()),
    )
}

fn pnpm_check() -> HostCheckSummary {
    tool_check(
        "local-workflows",
        "pnpm",
        "Local OpenClaw checkout workflows",
        HostCheckLevel::Recommended,
        "pnpm",
        &["--version"],
        Some("Install pnpm if you want local OpenClaw checkout workflows.".to_string()),
    )
}

fn bun_check() -> HostCheckSummary {
    tool_check(
        "local-workflows",
        "bun",
        "Optional Bun-based local workflows",
        HostCheckLevel::Optional,
        "bun",
        &["--version"],
        Some("Install bun only if you plan to run Bun-based local commands.".to_string()),
    )
}

fn chrome_check(env: &BTreeMap<String, String>) -> HostCheckSummary {
    let Some(path) = resolve_google_chrome_executable_for_platform(env, std::env::consts::OS)
    else {
        return check(
            "browser",
            "Google Chrome",
            "Chrome MCP existing-session browser flows",
            HostCheckLevel::Recommended,
            HostCheckStatus::Missing,
            None,
            Some(format!(
                "Google Chrome {CHROME_MCP_MIN_MAJOR}+ was not found on this host"
            )),
            Some(format!(
                "Install Google Chrome {CHROME_MCP_MIN_MAJOR}+ if you want Chrome MCP existing-session flows."
            )),
        );
    };

    match tool_version(path.to_string_lossy().as_ref(), &["--version"]) {
        Ok(summary) => match parse_browser_major_version(&summary.version) {
            Some(major) if major < CHROME_MCP_MIN_MAJOR => check(
                "browser",
                "Google Chrome",
                "Chrome MCP existing-session browser flows",
                HostCheckLevel::Recommended,
                HostCheckStatus::Outdated,
                Some(summary.version.clone()),
                Some(format!(
                    "detected Chrome {summary}; upgrade to {CHROME_MCP_MIN_MAJOR}+",
                    summary = summary.version
                )),
                Some(format!(
                    "Upgrade Google Chrome to {CHROME_MCP_MIN_MAJOR} or newer for Chrome MCP existing-session flows."
                )),
            ),
            Some(_) => check(
                "browser",
                "Google Chrome",
                "Chrome MCP existing-session browser flows",
                HostCheckLevel::Recommended,
                HostCheckStatus::Ok,
                Some(summary.version),
                Some(format!("path: {}", path.display())),
                None,
            ),
            None => check(
                "browser",
                "Google Chrome",
                "Chrome MCP existing-session browser flows",
                HostCheckLevel::Recommended,
                HostCheckStatus::Outdated,
                Some(summary.version.clone()),
                Some(format!(
                    "could not read the Chrome major version from {}",
                    summary.version
                )),
                Some(format!(
                    "Verify Google Chrome is {CHROME_MCP_MIN_MAJOR}+ if you want Chrome MCP existing-session flows."
                )),
            ),
        },
        Err(detail) => check(
            "browser",
            "Google Chrome",
            "Chrome MCP existing-session browser flows",
            HostCheckLevel::Recommended,
            HostCheckStatus::Missing,
            None,
            Some(format!("{} ({detail})", path.display())),
            Some(format!(
                "Install Google Chrome {CHROME_MCP_MIN_MAJOR}+ if you want Chrome MCP existing-session flows."
            )),
        ),
    }
}

fn service_manager_check(env: &BTreeMap<String, String>) -> HostCheckSummary {
    match service_manager_kind(env) {
        ServiceManagerKind::Launchd => match tool_version("launchctl", &["help"]) {
            Ok(_) => check(
                "background-services",
                "launchd",
                "Keep envs running in the background",
                HostCheckLevel::Recommended,
                HostCheckStatus::Ok,
                None,
                Some("launchd is available".to_string()),
                None,
            ),
            Err(detail) => check(
                "background-services",
                "launchd",
                "Keep envs running in the background",
                HostCheckLevel::Recommended,
                HostCheckStatus::Missing,
                None,
                Some(detail),
                Some(
                    "Use --no-service or make sure launchd is available on this machine."
                        .to_string(),
                ),
            ),
        },
        ServiceManagerKind::SystemdUser => match tool_version("systemctl", &["--version"]) {
            Ok(summary) => check(
                "background-services",
                "systemd --user",
                "Keep envs running in the background",
                HostCheckLevel::Recommended,
                HostCheckStatus::Ok,
                Some(summary.version),
                Some("systemd --user service support is available".to_string()),
                None,
            ),
            Err(detail) => check(
                "background-services",
                "systemd --user",
                "Keep envs running in the background",
                HostCheckLevel::Recommended,
                HostCheckStatus::Missing,
                None,
                Some(detail),
                Some(
                    "Use --no-service or install systemd user services on this machine."
                        .to_string(),
                ),
            ),
        },
    }
}

fn tool_check(
    category: &str,
    name: &str,
    purpose: &str,
    level: HostCheckLevel,
    command: &str,
    args: &[&str],
    suggestion: Option<String>,
) -> HostCheckSummary {
    match tool_version(command, args) {
        Ok(summary) => check(
            category,
            name,
            purpose,
            level,
            HostCheckStatus::Ok,
            Some(summary.version),
            None,
            None,
        ),
        Err(detail) => check(
            category,
            name,
            purpose,
            level,
            HostCheckStatus::Missing,
            None,
            Some(detail),
            suggestion,
        ),
    }
}

fn check(
    category: &str,
    name: &str,
    purpose: &str,
    level: HostCheckLevel,
    status: HostCheckStatus,
    version: Option<String>,
    detail: Option<String>,
    suggestion: Option<String>,
) -> HostCheckSummary {
    HostCheckSummary {
        category: category.to_string(),
        name: name.to_string(),
        purpose: purpose.to_string(),
        level: level.as_str().to_string(),
        status: status.as_str().to_string(),
        available: status.available(),
        version,
        detail,
        suggestion,
    }
}

fn npm_version(env: &BTreeMap<String, String>) -> Result<String, String> {
    tool_version(configured_npm_bin(env), &["--version"]).map(|summary| summary.version)
}

fn node_version(env: &BTreeMap<String, String>) -> Result<String, String> {
    let _ = env;
    tool_version("node", &["--version"]).map(|summary| summary.version)
}

fn tool_version(command: &str, args: &[&str]) -> Result<HostCommandSummary, String> {
    let output = Command::new(command)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| format!("{command} {}", command_failure_detail(error)))?;
    if !output.status.success() {
        let detail = summarize_error_output(&output.stderr, &output.stdout).unwrap_or_else(|| {
            format!(
                "{command} exited with code {}",
                output.status.code().unwrap_or(1)
            )
        });
        return Err(format!("{command} could not be run: {detail}"));
    }

    let version = summarize_command_output(&output.stdout, &output.stderr).unwrap_or_else(|| {
        args.first()
            .map(|arg| format!("{command} {arg} succeeded"))
            .unwrap_or_else(|| format!("{command} is available"))
    });
    Ok(HostCommandSummary { version })
}

fn command_failure_detail(error: std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "was not found on PATH".to_string(),
        _ => error.to_string(),
    }
}

fn summarize_command_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    for bytes in [stdout, stderr] {
        let text = String::from_utf8_lossy(bytes);
        if let Some(line) = text.lines().find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed.to_string())
        }) {
            return Some(line);
        }
    }
    None
}

fn summarize_error_output(stderr: &[u8], stdout: &[u8]) -> Option<String> {
    for bytes in [stderr, stdout] {
        let text = String::from_utf8_lossy(bytes);
        if let Some(line) = text.lines().find_map(|line| {
            let trimmed = line.trim();
            (!trimmed.is_empty()).then_some(trimmed.to_string())
        }) {
            return Some(line);
        }
    }
    None
}

fn parse_version_like(version: &str) -> Option<Vec<u64>> {
    let trimmed = version.trim();
    let trimmed = trimmed.strip_prefix('v').unwrap_or(trimmed);
    let mut out = Vec::new();
    for part in trimmed.split('.') {
        if part.is_empty() {
            return None;
        }
        out.push(part.parse::<u64>().ok()?);
    }
    Some(out)
}

fn parse_browser_major_version(raw_version: &str) -> Option<u32> {
    raw_version
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '.'))
        .filter(|token| token.contains('.'))
        .find_map(|token| {
            token.split('.').next().and_then(|major| {
                (!major.is_empty() && major.chars().all(|ch| ch.is_ascii_digit()))
                    .then(|| major.parse::<u32>().ok())
                    .flatten()
            })
        })
}

fn resolve_google_chrome_executable_for_platform(
    env: &BTreeMap<String, String>,
    platform: &str,
) -> Option<PathBuf> {
    match platform {
        "macos" => find_first_existing(&[
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            resolve_user_home(env)
                .join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from(
                "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            ),
            resolve_user_home(env)
                .join("Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary"),
        ]),
        "linux" => find_first_existing(&[
            PathBuf::from("/usr/bin/google-chrome"),
            PathBuf::from("/usr/bin/google-chrome-stable"),
            PathBuf::from("/usr/bin/google-chrome-beta"),
            PathBuf::from("/usr/bin/google-chrome-unstable"),
            PathBuf::from("/snap/bin/google-chrome"),
        ]),
        "windows" => {
            let local_app_data = env
                .get("LOCALAPPDATA")
                .cloned()
                .unwrap_or_default()
                .trim()
                .to_string();
            let program_files = env
                .get("ProgramFiles")
                .cloned()
                .unwrap_or_else(|| "C:\\Program Files".to_string());
            let program_files_x86 = env
                .get("ProgramFiles(x86)")
                .cloned()
                .unwrap_or_else(|| "C:\\Program Files (x86)".to_string());
            let mut candidates = Vec::new();
            if !local_app_data.is_empty() {
                candidates.push(
                    PathBuf::from(local_app_data.clone())
                        .join("Google/Chrome/Application/chrome.exe"),
                );
                candidates.push(
                    PathBuf::from(local_app_data).join("Google/Chrome SxS/Application/chrome.exe"),
                );
            }
            candidates
                .push(PathBuf::from(program_files).join("Google/Chrome/Application/chrome.exe"));
            candidates.push(
                PathBuf::from(program_files_x86).join("Google/Chrome/Application/chrome.exe"),
            );
            find_first_existing(&candidates)
        }
        _ => None,
    }
}

fn find_first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|path| path.exists()).cloned()
}

#[cfg(test)]
mod tests {
    use super::{compare_version_like, parse_browser_major_version};

    #[test]
    fn compare_version_like_orders_semverish_versions() {
        assert_eq!(
            compare_version_like("22.14.0", "22.14.0"),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(
            compare_version_like("v22.15.1", "22.14.0"),
            Some(std::cmp::Ordering::Greater)
        );
        assert_eq!(
            compare_version_like("20.11.0", "22.14.0"),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn parse_browser_major_version_extracts_chrome_major() {
        assert_eq!(
            parse_browser_major_version("Google Chrome 144.0.7540.0"),
            Some(144)
        );
        assert_eq!(
            parse_browser_major_version("Google Chrome Canary 145.1.2.3"),
            Some(145)
        );
        assert_eq!(parse_browser_major_version("unknown"), None);
    }
}
