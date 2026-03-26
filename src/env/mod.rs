mod binding;
mod execution;
mod health;
mod inspect;
mod lifecycle;
mod snapshots;

use std::collections::BTreeMap;
use std::path::Path;

pub use execution::ResolvedExecution;

pub struct EnvironmentService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> EnvironmentService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }
}
