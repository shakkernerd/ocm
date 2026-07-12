pub(crate) mod inspect;
mod manage;
pub(crate) mod platform;

use std::collections::BTreeMap;
use std::path::Path;

pub use inspect::{ServiceSummary, ServiceSummaryList};
pub use manage::{ServiceActionSummary, ServiceInstallSummary};
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
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        manage::install_service(name, self.env, self.cwd)
    }

    pub fn start(&self, name: &str) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        manage::start_service(name, self.env, self.cwd)
    }

    pub fn stop(&self, name: &str) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        manage::stop_service(name, self.env, self.cwd)
    }

    pub fn restart(&self, name: &str) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        manage::restart_service(name, self.env, self.cwd)
    }

    pub fn uninstall(&self, name: &str) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        self.uninstall_locked(name)
    }

    pub(crate) fn uninstall_locked(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::uninstall_service(name, self.env, self.cwd)
    }
}
