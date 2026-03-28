use std::fs;
use std::path::PathBuf;

use serde::Serialize;

use crate::infra::archive::extract_tar_gz;
use crate::infra::download::download_to_file;

use super::{Cli, render};

const RELEASE_REPO: &str = "shakkernerd/ocm";
const INTERNAL_SELF_UPDATE_RELEASE_URL_ENV: &str = "OCM_INTERNAL_SELF_UPDATE_RELEASE_URL";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SelfUpdateMode {
    Check,
    Update,
}

impl SelfUpdateMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Update => "update",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SelfUpdateStatus {
    UpToDate,
    UpdateAvailable,
    Updated,
}

impl SelfUpdateStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::UpToDate => "up_to_date",
            Self::UpdateAvailable => "update_available",
            Self::Updated => "updated",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelfUpdateSummary {
    pub mode: SelfUpdateMode,
    pub status: SelfUpdateStatus,
    pub current_version: String,
    pub target_version: String,
    pub binary_path: String,
    pub asset_name: String,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Clone, Debug, serde::Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    browser_download_url: String,
}

impl Cli {
    pub(super) fn handle_self_update(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "self update")?;
        let (args, check) = Self::consume_flag(args, "--check");
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        Self::assert_no_extra_args(&args)?;

        let target_display = version.clone().unwrap_or_else(|| "latest".to_string());
        let summary = if check {
            self.self_update_check(version.as_deref())?
        } else {
            self.with_progress(format!("Updating ocm to {target_display}"), || {
                self.self_update_install(version.as_deref())
            })?
        };

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::self_update::self_update(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn dispatch_self_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => self.dispatch_help_command(vec!["self".to_string()]),
            "update" => self.handle_self_update(args),
            other => Err(format!("unknown self command: {other}")),
        }
    }

    fn self_update_check(&self, version: Option<&str>) -> Result<SelfUpdateSummary, String> {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let binary_path = self.current_binary_path()?;
        let asset_name = self.current_release_asset_name()?;
        let release = self.fetch_self_release(version)?;
        let target_version = display_version_from_tag(&release.tag_name);

        Ok(SelfUpdateSummary {
            mode: SelfUpdateMode::Check,
            status: if target_version == current_version {
                SelfUpdateStatus::UpToDate
            } else {
                SelfUpdateStatus::UpdateAvailable
            },
            current_version,
            target_version,
            binary_path: binary_path.to_string_lossy().into_owned(),
            asset_name,
        })
    }

    fn self_update_install(&self, version: Option<&str>) -> Result<SelfUpdateSummary, String> {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        let binary_path = self.current_binary_path()?;
        let asset_name = self.current_release_asset_name()?;
        let release = self.fetch_self_release(version)?;
        let target_version = display_version_from_tag(&release.tag_name);

        if target_version == current_version {
            return Ok(SelfUpdateSummary {
                mode: SelfUpdateMode::Update,
                status: SelfUpdateStatus::UpToDate,
                current_version,
                target_version,
                binary_path: binary_path.to_string_lossy().into_owned(),
                asset_name,
            });
        }

        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| {
                format!(
                    "release {} does not publish an asset for this platform: {asset_name}",
                    release.tag_name
                )
            })?;

        let parent = binary_path.parent().ok_or_else(|| {
            format!(
                "cannot update ocm because its binary path has no parent: {}",
                binary_path.display()
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;

        let temp_root =
            std::env::temp_dir().join(format!("ocm-self-update-{}", std::process::id()));
        if temp_root.exists() {
            let _ = fs::remove_dir_all(&temp_root);
        }
        fs::create_dir_all(&temp_root).map_err(|error| error.to_string())?;

        let archive_path = temp_root.join(&asset.name);
        download_to_file(&asset.browser_download_url, &archive_path)?;
        let extract_dir = temp_root.join("extract");
        extract_tar_gz(&archive_path, &extract_dir)?;

        let extracted_binary = extract_dir.join("ocm");
        if !extracted_binary.exists() {
            return Err("release archive did not contain an executable ocm binary".to_string());
        }

        let staged_binary = parent.join(format!(
            ".ocm-update-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        fs::copy(&extracted_binary, &staged_binary).map_err(|error| {
            format!(
                "failed to stage the updated ocm binary in {}: {error}",
                parent.display()
            )
        })?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = fs::metadata(&staged_binary)
                .map_err(|error| error.to_string())?
                .permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&staged_binary, permissions).map_err(|error| error.to_string())?;
        }

        fs::rename(&staged_binary, &binary_path).map_err(|error| {
            let _ = fs::remove_file(&staged_binary);
            format!(
                "failed to replace {}: {error}. If this path is managed elsewhere, reinstall ocm or use your package manager instead.",
                binary_path.display()
            )
        })?;

        let _ = fs::remove_dir_all(&temp_root);

        Ok(SelfUpdateSummary {
            mode: SelfUpdateMode::Update,
            status: SelfUpdateStatus::Updated,
            current_version,
            target_version,
            binary_path: binary_path.to_string_lossy().into_owned(),
            asset_name,
        })
    }

    fn current_binary_path(&self) -> Result<PathBuf, String> {
        std::env::current_exe()
            .map_err(|error| format!("failed to resolve the current ocm binary: {error}"))
    }

    fn current_release_asset_name(&self) -> Result<String, String> {
        let os = match std::env::consts::OS {
            "macos" => "apple-darwin",
            "linux" => "unknown-linux-gnu",
            other => {
                return Err(format!(
                    "ocm self update is not supported on this operating system yet: {other}"
                ));
            }
        };

        let arch = match std::env::consts::ARCH {
            "x86_64" => "x86_64",
            "aarch64" => "aarch64",
            other => {
                return Err(format!(
                    "ocm self update is not supported on this architecture yet: {other}"
                ));
            }
        };

        Ok(format!("ocm-{arch}-{os}.tar.gz"))
    }

    fn fetch_self_release(&self, version: Option<&str>) -> Result<GitHubRelease, String> {
        let url = if let Some(override_url) = self
            .env
            .get(INTERNAL_SELF_UPDATE_RELEASE_URL_ENV)
            .map(String::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            override_url.to_string()
        } else if let Some(version) = version {
            format!(
                "https://api.github.com/repos/{RELEASE_REPO}/releases/tags/{}",
                normalize_release_tag(version)
            )
        } else {
            format!("https://api.github.com/repos/{RELEASE_REPO}/releases/latest")
        };

        let response = ureq::get(&url)
            .set("User-Agent", &format!("ocm/{}", env!("CARGO_PKG_VERSION")))
            .call()
            .map_err(|error| format!("failed to query ocm releases: {error}"))?;
        serde_json::from_reader(response.into_reader())
            .map_err(|error| format!("failed to parse ocm release metadata: {error}"))
    }
}

fn normalize_release_tag(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('v') {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}

fn display_version_from_tag(tag: &str) -> String {
    tag.trim().trim_start_matches('v').to_string()
}

#[cfg(test)]
mod tests {
    use super::{display_version_from_tag, normalize_release_tag};

    #[test]
    fn normalize_release_tag_accepts_prefixed_and_bare_versions() {
        assert_eq!(normalize_release_tag("0.2.1"), "v0.2.1");
        assert_eq!(normalize_release_tag("v0.2.1"), "v0.2.1");
    }

    #[test]
    fn display_version_from_tag_strips_the_v_prefix() {
        assert_eq!(display_version_from_tag("v0.2.1"), "0.2.1");
        assert_eq!(display_version_from_tag("0.2.1"), "0.2.1");
    }
}
