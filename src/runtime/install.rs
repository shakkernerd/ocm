use super::{
    RuntimeMeta, RuntimeReleaseSelectorKind, RuntimeService,
};
use serde::Serialize;
use crate::store::{
    get_runtime, install_runtime, install_runtime_from_release, install_runtime_from_url,
    list_runtimes,
};

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

impl<'a> RuntimeService<'a> {
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
