use super::{AddRuntimeOptions, RuntimeMeta};
use std::collections::BTreeMap;
use std::path::Path;

use crate::store::{
    add_runtime, get_runtime_verified, list_runtimes, remove_runtime,
};

pub struct RuntimeService<'a> {
    pub(super) env: &'a BTreeMap<String, String>,
    pub(super) cwd: &'a Path,
}

impl<'a> RuntimeService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn add(&self, options: AddRuntimeOptions) -> Result<RuntimeMeta, String> {
        add_runtime(options, self.env, self.cwd)
    }

    pub fn list(&self) -> Result<Vec<RuntimeMeta>, String> {
        list_runtimes(self.env, self.cwd)
    }

    pub fn show(&self, name: &str) -> Result<RuntimeMeta, String> {
        get_runtime_verified(name, self.env, self.cwd)
    }

    pub fn remove(&self, name: &str) -> Result<RuntimeMeta, String> {
        remove_runtime(name, self.env, self.cwd)
    }
}
