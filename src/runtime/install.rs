use super::{RuntimeMeta, RuntimeReleaseSelectorKind, RuntimeService};
use crate::runtime::releases::{
    is_official_openclaw_releases_url, load_official_openclaw_releases,
    normalize_openclaw_channel_selector, official_openclaw_releases_url,
    select_official_openclaw_release_by_channel, select_official_openclaw_release_by_version,
};
use crate::store::{
    InstallContext, RuntimeReleaseDetails, get_runtime, install_runtime,
    install_runtime_from_official_openclaw_release, install_runtime_from_release,
    install_runtime_from_selected_official_openclaw_release, install_runtime_from_url,
    list_runtimes, runtime_integrity_issue,
};
use serde::Serialize;

#[derive(Clone, Debug)]
pub struct InstallRuntimeOptions {
    pub name: String,
    pub path: String,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct InstallRuntimeFromUrlOptions {
    pub name: String,
    pub url: String,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct InstallRuntimeFromReleaseOptions {
    pub name: String,
    pub manifest_url: String,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct InstallRuntimeFromOfficialReleaseOptions {
    pub name: String,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub description: Option<String>,
    pub force: bool,
}

#[derive(Clone, Debug)]
pub struct UpdateRuntimeFromReleaseOptions {
    pub name: String,
    pub version: Option<String>,
    pub channel: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUpdateSummary {
    pub name: String,
    pub outcome: String,
    pub binary_path: Option<String>,
    pub source_kind: String,
    pub release_version: Option<String>,
    pub release_channel: Option<String>,
    pub issue: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUpdateBatchSummary {
    pub count: usize,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub results: Vec<RuntimeUpdateSummary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficialRuntimePrepareAction {
    Installed,
    Reused,
    Updated,
}

impl<'a> RuntimeService<'a> {
    pub fn canonical_official_openclaw_runtime_name(
        version: Option<&str>,
        channel: Option<&str>,
    ) -> Result<String, String> {
        let version = version
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let channel = channel
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_openclaw_channel_selector)
            .transpose()?;

        match (version, channel) {
            (Some(version), None) => Ok(version),
            (None, Some(channel)) => Ok(channel),
            (None, None) => Err(
                "official OpenClaw runtime selection requires --version or --channel".to_string(),
            ),
            (Some(_), Some(_)) => Err(
                "official OpenClaw runtime selection accepts only one of --version or --channel"
                    .to_string(),
            ),
        }
    }

    pub fn ensure_official_openclaw_runtime(
        &self,
        version: Option<String>,
        channel: Option<String>,
    ) -> Result<RuntimeMeta, String> {
        Ok(self
            .prepare_official_openclaw_runtime(InstallRuntimeFromOfficialReleaseOptions {
                name: Self::canonical_official_openclaw_runtime_name(
                    version.as_deref(),
                    channel.as_deref(),
                )?,
                version,
                channel,
                description: None,
                force: false,
            })?
            .0)
    }

    pub fn prepare_official_openclaw_runtime(
        &self,
        options: InstallRuntimeFromOfficialReleaseOptions,
    ) -> Result<(RuntimeMeta, OfficialRuntimePrepareAction), String> {
        let version = options
            .version
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let channel = options
            .channel
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let channel = channel
            .as_deref()
            .map(normalize_openclaw_channel_selector)
            .transpose()?;

        if version.is_some() && channel.is_some() {
            return Err(
                "official OpenClaw runtime selection accepts only one of --version or --channel"
                    .to_string(),
            );
        }

        let runtime_name =
            Self::canonical_official_openclaw_runtime_name(version.as_deref(), channel.as_deref())?;
        if options.name != runtime_name {
            return Err(format!(
                "official runtime installs use the canonical name \"{runtime_name}\" for this selector"
            ));
        }

        let releases_url = official_openclaw_releases_url(self.env);
        let releases = load_official_openclaw_releases(&releases_url)?;
        let selected_release = match (version.as_deref(), channel.as_deref()) {
            (Some(version), None) => {
                select_official_openclaw_release_by_version(&releases, version)?
            }
            (None, Some(channel)) => {
                select_official_openclaw_release_by_channel(&releases, channel)?
            }
            _ => unreachable!("validated above"),
        };

        let mut existing_meta = None;
        if let Ok(existing) = get_runtime(&runtime_name, self.env, self.cwd) {
            if !is_official_openclaw_releases_url(existing.source_manifest_url.as_deref(), self.env)
            {
                return Err(format!(
                    "runtime \"{runtime_name}\" already exists and is not an official OpenClaw runtime; use --runtime {runtime_name} instead"
                ));
            }

            let healthy = runtime_integrity_issue(&existing, self.env).is_none();
            let same_release = existing.release_version.as_deref()
                == Some(selected_release.version.as_str())
                && existing.source_url.as_deref() == Some(selected_release.tarball_url.as_str());
            let matches_requested_selector = match (version.as_deref(), channel.as_deref()) {
                (Some(requested_version), None) => {
                    existing.release_version.as_deref() == Some(requested_version)
                        || (existing.release_selector_kind
                            == Some(RuntimeReleaseSelectorKind::Version)
                            && existing.release_selector_value.as_deref()
                                == Some(requested_version))
                }
                (None, Some(requested_channel)) => {
                    existing.release_selector_kind == Some(RuntimeReleaseSelectorKind::Channel)
                        && existing.release_selector_value.as_deref() == Some(requested_channel)
                }
                _ => false,
            };

            if !options.force && healthy && same_release && matches_requested_selector {
                return Ok((existing, OfficialRuntimePrepareAction::Reused));
            }

            existing_meta = Some(existing);
        }

        let description = options.description.or_else(|| {
            existing_meta
                .as_ref()
                .and_then(|meta| meta.description.clone())
        });
        let meta = install_runtime_from_selected_official_openclaw_release(
            runtime_name.clone(),
            options.force || existing_meta.is_some(),
            releases_url,
            selected_release,
            RuntimeReleaseDetails::with_selector(
                match (version.as_deref(), channel.as_deref()) {
                    (Some(_), None) => Some(RuntimeReleaseSelectorKind::Version),
                    (None, Some(_)) => Some(RuntimeReleaseSelectorKind::Channel),
                    _ => None,
                },
                match (version, channel) {
                    (Some(version), None) => Some(version),
                    (None, Some(channel)) => Some(channel),
                    _ => None,
                },
            ),
            description,
            InstallContext {
                env: self.env,
                cwd: self.cwd,
            },
        )?;
        let action = if existing_meta.is_some() {
            OfficialRuntimePrepareAction::Updated
        } else {
            OfficialRuntimePrepareAction::Installed
        };
        Ok((meta, action))
    }

    pub fn install(&self, options: InstallRuntimeOptions) -> Result<RuntimeMeta, String> {
        install_runtime(options, self.env, self.cwd)
    }

    pub fn install_from_url(
        &self,
        options: InstallRuntimeFromUrlOptions,
    ) -> Result<RuntimeMeta, String> {
        install_runtime_from_url(options, self.env, self.cwd)
    }

    pub fn install_from_release(
        &self,
        options: InstallRuntimeFromReleaseOptions,
    ) -> Result<RuntimeMeta, String> {
        install_runtime_from_release(options, self.env, self.cwd)
    }

    pub fn install_from_official_openclaw_release(
        &self,
        options: InstallRuntimeFromOfficialReleaseOptions,
    ) -> Result<RuntimeMeta, String> {
        install_runtime_from_official_openclaw_release(options, self.env, self.cwd)
    }

    pub fn update_from_release(
        &self,
        options: UpdateRuntimeFromReleaseOptions,
    ) -> Result<RuntimeMeta, String> {
        if options.version.is_some() && options.channel.is_some() {
            return Err("runtime update accepts only one of --version or --channel".to_string());
        }

        let existing = get_runtime(&options.name, self.env, self.cwd)?;
        let manifest_url = existing.source_manifest_url.ok_or_else(|| {
            format!(
                "runtime \"{}\" is not backed by a release manifest",
                existing.name
            )
        })?;
        let (version, channel) = match (options.version, options.channel) {
            (Some(version), None) => (Some(version), None),
            (None, Some(channel)) => (None, Some(channel)),
            (None, None) => match (
                existing.release_selector_kind.clone(),
                existing.release_selector_value.clone(),
            ) {
                (Some(RuntimeReleaseSelectorKind::Version), Some(value)) => (Some(value), None),
                (Some(RuntimeReleaseSelectorKind::Channel), Some(value)) => (None, Some(value)),
                _ => {
                    return Err(format!(
                        "runtime \"{}\" does not have a stored release selector; pass --version or --channel",
                        existing.name
                    ));
                }
            },
            _ => unreachable!("conflicting selectors are rejected above"),
        };

        if is_official_openclaw_releases_url(Some(manifest_url.as_str()), self.env) {
            return install_runtime_from_official_openclaw_release(
                InstallRuntimeFromOfficialReleaseOptions {
                    name: existing.name,
                    version,
                    channel,
                    description: existing.description,
                    force: true,
                },
                self.env,
                self.cwd,
            );
        }

        install_runtime_from_release(
            InstallRuntimeFromReleaseOptions {
                name: existing.name,
                manifest_url,
                version,
                channel,
                description: existing.description,
                force: true,
            },
            self.env,
            self.cwd,
        )
    }

    pub fn update_all_from_release(
        &self,
        version: Option<String>,
        channel: Option<String>,
    ) -> Result<RuntimeUpdateBatchSummary, String> {
        if version.is_some() && channel.is_some() {
            return Err("runtime update accepts only one of --version or --channel".to_string());
        }

        let runtimes = list_runtimes(self.env, self.cwd)?;
        let mut out = Vec::with_capacity(runtimes.len());
        for runtime in runtimes {
            if runtime.source_manifest_url.is_none() {
                out.push(RuntimeUpdateSummary {
                    name: runtime.name,
                    outcome: "skipped".to_string(),
                    binary_path: Some(runtime.binary_path),
                    source_kind: runtime.source_kind.as_str().to_string(),
                    release_version: runtime.release_version,
                    release_channel: runtime.release_channel,
                    issue: Some("runtime is not backed by a release manifest".to_string()),
                });
                continue;
            }

            match self.update_from_release(UpdateRuntimeFromReleaseOptions {
                name: runtime.name.clone(),
                version: version.clone(),
                channel: channel.clone(),
            }) {
                Ok(meta) => out.push(RuntimeUpdateSummary {
                    name: meta.name,
                    outcome: "updated".to_string(),
                    binary_path: Some(meta.binary_path),
                    source_kind: meta.source_kind.as_str().to_string(),
                    release_version: meta.release_version,
                    release_channel: meta.release_channel,
                    issue: None,
                }),
                Err(error) => out.push(RuntimeUpdateSummary {
                    name: runtime.name,
                    outcome: "failed".to_string(),
                    binary_path: Some(runtime.binary_path),
                    source_kind: runtime.source_kind.as_str().to_string(),
                    release_version: runtime.release_version,
                    release_channel: runtime.release_channel,
                    issue: Some(error),
                }),
            }
        }
        let updated = out
            .iter()
            .filter(|summary| summary.outcome == "updated")
            .count();
        let skipped = out
            .iter()
            .filter(|summary| summary.outcome == "skipped")
            .count();
        let failed = out
            .iter()
            .filter(|summary| summary.outcome == "failed")
            .count();
        Ok(RuntimeUpdateBatchSummary {
            count: out.len(),
            updated,
            skipped,
            failed,
            results: out,
        })
    }
}
