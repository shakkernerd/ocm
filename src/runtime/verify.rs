use super::RuntimeService;
use crate::store::{get_runtime, get_runtime_verified, list_runtimes, runtime_integrity_issue};
use crate::types::{RuntimeBinarySummary, RuntimeMeta, RuntimeVerifySummary};

impl<'a> RuntimeService<'a> {
    pub fn verify(&self, name: &str) -> Result<RuntimeVerifySummary, String> {
        let meta = get_runtime(name, self.env, self.cwd)?;
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
