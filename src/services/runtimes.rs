use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::store::{
    add_runtime, get_runtime_verified, install_runtime, install_runtime_from_url, list_runtimes,
    remove_runtime,
};
use crate::types::{
    AddRuntimeOptions, InstallRuntimeFromUrlOptions, InstallRuntimeOptions, RuntimeBinarySummary,
    RuntimeMeta, RuntimeVerifySummary,
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

    pub fn list(&self) -> Result<Vec<RuntimeMeta>, String> {
        list_runtimes(self.env, self.cwd)
    }

    pub fn show(&self, name: &str) -> Result<RuntimeMeta, String> {
        get_runtime_verified(name, self.env, self.cwd)
    }

    pub fn verify(&self, name: &str) -> Result<RuntimeVerifySummary, String> {
        let meta = crate::store::get_runtime(name, self.env, self.cwd)?;
        let issue = runtime_issue(&meta.binary_path);
        Ok(RuntimeVerifySummary {
            name: meta.name,
            binary_path: meta.binary_path,
            source_kind: meta.source_kind.as_str().to_string(),
            source_path: meta.source_path,
            source_url: meta.source_url,
            install_root: meta.install_root,
            healthy: issue.is_none(),
            issue,
        })
    }

    pub fn which(&self, name: &str) -> Result<RuntimeBinarySummary, String> {
        let meta = get_runtime_verified(name, self.env, self.cwd)?;
        Ok(RuntimeBinarySummary {
            name: meta.name,
            binary_path: meta.binary_path,
            source_kind: meta.source_kind.as_str().to_string(),
        })
    }

    pub fn remove(&self, name: &str) -> Result<RuntimeMeta, String> {
        remove_runtime(name, self.env, self.cwd)
    }
}

fn runtime_issue(binary_path: &str) -> Option<String> {
    let path = Path::new(binary_path);
    if !path.exists() {
        return Some(format!("binary path does not exist: {}", path.display()));
    }

    match fs::metadata(path) {
        Ok(metadata) if !metadata.is_file() => {
            Some(format!("binary path is not a file: {}", path.display()))
        }
        Ok(_) => None,
        Err(error) => Some(error.to_string()),
    }
}
