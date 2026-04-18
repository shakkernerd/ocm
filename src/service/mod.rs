pub(crate) mod inspect;
mod manage;
pub(crate) mod platform;

use std::collections::BTreeMap;
use std::path::Path;

pub use inspect::{ServiceSummary, ServiceSummaryList};
pub use manage::{ServiceActionSummary, ServiceInstallSummary, ServiceLogSummary};
pub(crate) use platform::{
    ServiceManagerKind, service_backend_support_error, service_manager_kind,
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
        inspect::service_status_fast(name, self.env, self.cwd)
    }

    pub fn install(&self, name: &str) -> Result<ServiceInstallSummary, String> {
        manage::install_service(name, self.env, self.cwd)
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
