pub(crate) mod inspect;
mod manage;
pub(crate) mod platform;

use std::collections::BTreeMap;
use std::path::Path;

pub use inspect::{ServiceSummary, ServiceSummaryList};
pub use manage::{ServiceActionSummary, ServiceInstallSummary, ServiceRestartOptions};
pub(crate) use platform::{
    ServiceManagerKind, service_backend_support_error, service_manager_kind,
};

pub struct ServiceService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ServiceMaintenanceState {
    pub(crate) enabled: bool,
    pub(crate) running: bool,
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
        self.start_locked(name)
    }

    pub(crate) fn start_locked(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::start_service(name, self.env, self.cwd)
    }

    pub fn stop(&self, name: &str) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        self.stop_locked(name)
    }

    pub(crate) fn stop_locked(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::stop_service(name, self.env, self.cwd)
    }

    pub(crate) fn quiesce_for_snapshot_locked(
        &self,
        name: &str,
    ) -> Result<Option<ServiceMaintenanceState>, String> {
        let status = self.status(name)?;
        if !status.running {
            return Ok(None);
        }
        let meta = crate::env::EnvironmentService::new(self.env, self.cwd).get(name)?;
        let state = ServiceMaintenanceState {
            enabled: meta.service_enabled,
            running: meta.service_running,
        };
        let stopped = self.stop_locked(name)?;
        if stopped.running {
            let stop_error = format!(
                "managed service for {name} remained running after the snapshot safety stop"
            );
            return match self.restore_after_snapshot_locked(name, Some(state)) {
                Ok(()) => Err(stop_error),
                Err(restore_error) => Err(format!(
                    "{stop_error}; also failed to restore its pre-snapshot service policy: {restore_error}"
                )),
            };
        }
        Ok(Some(state))
    }

    pub(crate) fn restore_after_snapshot_locked(
        &self,
        name: &str,
        state: Option<ServiceMaintenanceState>,
    ) -> Result<(), String> {
        if state.is_some_and(|state| state.enabled && state.running) {
            let meta = crate::env::EnvironmentService::new(self.env, self.cwd).get(name)?;
            if meta.service_enabled && meta.service_running && self.status(name)?.running {
                return Ok(());
            }
            self.start_locked(name)?;
        }
        Ok(())
    }

    pub fn restart(&self, name: &str) -> Result<ServiceActionSummary, String> {
        self.restart_with_options(name, ServiceRestartOptions::default())
    }

    pub fn restart_with_options(
        &self,
        name: &str,
        options: ServiceRestartOptions,
    ) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        self.restart_locked_with_options(name, options)
    }

    pub(crate) fn restart_locked_with_options(
        &self,
        name: &str,
        options: ServiceRestartOptions,
    ) -> Result<ServiceActionSummary, String> {
        manage::restart_service(name, options, self.env, self.cwd)
    }

    pub fn uninstall(&self, name: &str) -> Result<ServiceActionSummary, String> {
        let _lock = crate::env::EnvironmentService::new(self.env, self.cwd).lock_operation(name)?;
        self.uninstall_locked(name)
    }

    pub(crate) fn uninstall_locked(&self, name: &str) -> Result<ServiceActionSummary, String> {
        manage::uninstall_service(name, self.env, self.cwd)
    }
}
