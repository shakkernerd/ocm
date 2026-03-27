mod inspect;

use std::collections::BTreeMap;
use std::path::Path;

pub use inspect::{ServiceSummary, ServiceSummaryList};

pub struct ServiceService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> ServiceService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn list(&self) -> Result<ServiceSummaryList, String> {
        inspect::list_services(self.env, self.cwd)
    }

    pub fn status(&self, name: &str) -> Result<ServiceSummary, String> {
        inspect::service_status(name, self.env, self.cwd)
    }
}
