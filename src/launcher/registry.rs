use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::store::{add_launcher, get_launcher, list_launchers, remove_launcher};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LauncherMeta {
    pub kind: String,
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub description: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug)]
pub struct AddLauncherOptions {
    pub name: String,
    pub command: String,
    pub cwd: Option<String>,
    pub description: Option<String>,
}

pub struct LauncherService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> LauncherService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn add(&self, options: AddLauncherOptions) -> Result<LauncherMeta, String> {
        add_launcher(options, self.env, self.cwd)
    }

    pub fn list(&self) -> Result<Vec<LauncherMeta>, String> {
        list_launchers(self.env, self.cwd)
    }

    pub fn show(&self, name: &str) -> Result<LauncherMeta, String> {
        get_launcher(name, self.env, self.cwd)
    }

    pub fn remove(&self, name: &str) -> Result<LauncherMeta, String> {
        remove_launcher(name, self.env, self.cwd)
    }
}
