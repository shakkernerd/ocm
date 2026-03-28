mod inspect;
mod manage;
mod platform;

use std::collections::BTreeMap;
use std::path::Path;

pub use inspect::{DiscoveredServiceList, DiscoveredServiceSummary};
pub use inspect::{ServiceSummary, ServiceSummaryList};
pub use manage::{
    ServiceActionSummary, ServiceAdoptionSummary, ServiceInstallSummary, ServiceLogSummary,
    ServiceRestoreSummary,
};

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

    pub fn status_fast(&self, name: &str) -> Result<ServiceSummary, String> {
        inspect::service_status_fast(name, self.env, self.cwd)
    }

    pub fn discover(&self) -> Result<DiscoveredServiceList, String> {
        inspect::discover_services(self.env, self.cwd)
    }

    pub fn install(&self, name: &str) -> Result<ServiceInstallSummary, String> {
        manage::install_service(name, self.env, self.cwd)
    }

    pub fn adopt_global(
        &self,
        name: &str,
        dry_run: bool,
    ) -> Result<ServiceAdoptionSummary, String> {
        manage::adopt_global_service(name, self.env, self.cwd, dry_run)
    }

    pub fn restore_global(
        &self,
        name: &str,
        dry_run: bool,
    ) -> Result<ServiceRestoreSummary, String> {
        manage::restore_global_service(name, self.env, self.cwd, dry_run)
    }

    pub fn start(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::start_service(name, self.env, self.cwd)
    }

    pub fn stop(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::stop_service(name, self.env, self.cwd)
    }

    pub fn restart(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::restart_service(name, self.env, self.cwd)
    }

    pub fn uninstall(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::uninstall_service(name, self.env, self.cwd)
    }

    pub fn logs(
        &self,
        name: &str,
        stream: &str,
        tail_lines: Option<usize>,
    ) -> Result<ServiceLogSummary, String> {
        manage::service_logs(name, stream, tail_lines, self.env, self.cwd)
    }
}
