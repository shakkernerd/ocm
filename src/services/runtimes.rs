use std::collections::BTreeMap;
use std::path::Path;

use crate::releases::{load_release_manifest, query_releases};
use crate::store::{
    add_runtime, get_runtime_verified, install_runtime, install_runtime_from_release,
    install_runtime_from_url, list_runtimes, remove_runtime, runtime_integrity_issue,
};
use crate::types::{
    AddRuntimeOptions, InstallRuntimeFromReleaseOptions, InstallRuntimeFromUrlOptions,
    InstallRuntimeOptions, RuntimeBinarySummary, RuntimeMeta, RuntimeRelease, RuntimeUpdateSummary,
    RuntimeVerifySummary, UpdateRuntimeFromReleaseOptions,
};

pub struct RuntimeService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> RuntimeService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn add(&self, options: AddRuntimeOptions) -> Result<RuntimeMeta, String> {
        add_runtime(options, self.env, self.cwd)
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

    pub fn update_from_release(
        &self,
        options: UpdateRuntimeFromReleaseOptions,
    ) -> Result<RuntimeMeta, String> {
        if options.version.is_some() && options.channel.is_some() {
            return Err("runtime update accepts only one of --version or --channel".to_string());
        }

        let existing = crate::store::get_runtime(&options.name, self.env, self.cwd)?;
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
                (Some(crate::types::RuntimeReleaseSelectorKind::Version), Some(value)) => {
                    (Some(value), None)
                }
                (Some(crate::types::RuntimeReleaseSelectorKind::Channel), Some(value)) => {
                    (None, Some(value))
                }
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

    pub fn list(&self) -> Result<Vec<RuntimeMeta>, String> {
        list_runtimes(self.env, self.cwd)
    }

    pub fn releases_from_manifest(
        &self,
        url: &str,
        version: Option<&str>,
        channel: Option<&str>,
    ) -> Result<Vec<RuntimeRelease>, String> {
        let manifest = load_release_manifest(url)?;
        query_releases(&manifest, version, channel)
    }

    pub fn show(&self, name: &str) -> Result<RuntimeMeta, String> {
        get_runtime_verified(name, self.env, self.cwd)
    }

    pub fn verify(&self, name: &str) -> Result<RuntimeVerifySummary, String> {
        let meta = crate::store::get_runtime(name, self.env, self.cwd)?;
        Ok(build_verify_summary(meta))
    }

    pub fn verify_all(&self) -> Result<Vec<RuntimeVerifySummary>, String> {
        let runtimes = list_runtimes(self.env, self.cwd)?;
        Ok(runtimes.into_iter().map(build_verify_summary).collect())
    }

    pub fn update_all_from_release(
        &self,
        version: Option<String>,
        channel: Option<String>,
    ) -> Result<Vec<RuntimeUpdateSummary>, String> {
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
        Ok(out)
    }

    pub fn which(&self, name: &str) -> Result<RuntimeBinarySummary, String> {
        let meta = get_runtime_verified(name, self.env, self.cwd)?;
        Ok(RuntimeBinarySummary {
            name: meta.name,
            binary_path: meta.binary_path,
            source_kind: meta.source_kind.as_str().to_string(),
            release_version: meta.release_version,
            release_channel: meta.release_channel,
        })
    }

    pub fn remove(&self, name: &str) -> Result<RuntimeMeta, String> {
        remove_runtime(name, self.env, self.cwd)
    }
}

fn build_verify_summary(meta: RuntimeMeta) -> RuntimeVerifySummary {
    let issue = runtime_integrity_issue(&meta);
    RuntimeVerifySummary {
        name: meta.name,
        binary_path: meta.binary_path,
        source_kind: meta.source_kind.as_str().to_string(),
        source_path: meta.source_path,
        source_url: meta.source_url,
        source_manifest_url: meta.source_manifest_url,
        source_sha256: meta.source_sha256,
        release_version: meta.release_version,
        release_channel: meta.release_channel,
        install_root: meta.install_root,
        healthy: issue.is_none(),
        issue,
    }
}
