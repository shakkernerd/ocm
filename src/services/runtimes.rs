use std::collections::BTreeMap;
use std::path::Path;

use crate::releases::load_release_manifest;
use crate::store::{
    add_runtime, get_runtime_verified, install_runtime, install_runtime_from_release,
    install_runtime_from_url, list_runtimes, remove_runtime, runtime_integrity_issue,
};
use crate::types::{
    AddRuntimeOptions, InstallRuntimeFromReleaseOptions, InstallRuntimeFromUrlOptions,
    InstallRuntimeOptions, RuntimeBinarySummary, RuntimeMeta, RuntimeRelease, RuntimeVerifySummary,
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

    pub fn list(&self) -> Result<Vec<RuntimeMeta>, String> {
        list_runtimes(self.env, self.cwd)
    }

    pub fn releases_from_manifest(&self, url: &str) -> Result<Vec<RuntimeRelease>, String> {
        Ok(load_release_manifest(url)?.releases)
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
