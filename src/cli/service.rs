use super::{Cli, render};
use crate::service::{
    ServiceManagerKind, service_manager_kind, unsupported_service_manager_message,
};

impl Cli {
    fn ensure_service_backend_supported(&self) -> Result<(), String> {
        if service_manager_kind(&self.env) == ServiceManagerKind::Unsupported {
            return Err(unsupported_service_manager_message().to_string());
        }
        Ok(())
    }

    pub(super) fn handle_service_discover(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "service discover")?;
        Self::assert_no_extra_args(&args)?;

        let services = self.service_service().discover()?;
        if json_flag {
            self.print_json(&services)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_discover(&services, profile));
        Ok(0)
    }

    pub(super) fn handle_service_restore_global(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "service restore-global")?;
        let (args, dry_run) = Self::consume_flag(args, "--dry-run");
        let Some(name) = args.first() else {
            return Err("service restore-global requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.with_progress(
            if dry_run {
                format!("Planning global service restore for {name}")
            } else {
                format!("Restoring global service for {name}")
            },
            || self.service_service().restore_global(name, dry_run),
        )?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_restored(&summary, profile));
        Ok(0)
    }

    pub(super) fn handle_service_adopt_global(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "service adopt-global")?;
        let (args, dry_run) = Self::consume_flag(args, "--dry-run");
        let Some(name) = args.first() else {
            return Err("service adopt-global requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.with_progress(
            if dry_run {
                format!("Planning global service adoption for {name}")
            } else {
                format!("Adopting global service for {name}")
            },
            || self.service_service().adopt_global(name, dry_run),
        )?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_adopted(&summary, profile));
        Ok(0)
    }

    pub(super) fn handle_service_logs(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, stderr_flag) = Self::consume_flag(args, "--stderr");
        let (args, stdout_flag) = Self::consume_flag(args, "--stdout");
        let (args, tail_raw) = Self::consume_option(args, "--tail")?;
        let tail_lines = match tail_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--tail")? as usize),
            None => None,
        };
        if stdout_flag && stderr_flag {
            return Err("service logs accepts only one of --stdout or --stderr".to_string());
        }

        let Some(name) = args.first() else {
            return Err("service logs requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let stream = if stderr_flag { "stderr" } else { "stdout" };
        let summary = self.service_service().logs(name, stream, tail_lines)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_text(&summary.content)?;
        Ok(0)
    }

    pub(super) fn handle_service_install(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "service install")?;
        let Some(name) = args.first() else {
            return Err("service install requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.with_progress(format!("Installing service for {name}"), || {
            self.service_service().install(name)
        })?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_installed(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_service_list(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "service list")?;
        Self::assert_no_extra_args(&args)?;

        let services = self.service_service().list()?;
        if json_flag {
            self.print_json(&services)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_list(&services, profile));
        Ok(0)
    }

    pub(super) fn handle_service_status(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "service status")?;
        let (args, all_flag) = Self::consume_flag(args, "--all");

        if all_flag {
            Self::assert_no_extra_args(&args)?;
            let services = self.service_service().list()?;
            if json_flag {
                self.print_json(&services)?;
                return Ok(0);
            }

            self.stdout_lines(render::service::service_list(&services, profile));
            return Ok(0);
        }

        let Some(name) = args.first() else {
            return Err("service status requires <env> or --all".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.service_service().status(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_status(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_service_start(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "service start")?;
        let Some(name) = args.first() else {
            return Err("service start requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.service_service().start(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_action(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_service_stop(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "service stop")?;
        let Some(name) = args.first() else {
            return Err("service stop requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.service_service().stop(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_action(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_service_restart(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "service restart")?;
        let Some(name) = args.first() else {
            return Err("service restart requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.service_service().restart(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_action(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_service_uninstall(&self, args: Vec<String>) -> Result<i32, String> {
        self.ensure_service_backend_supported()?;
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "service uninstall")?;
        let Some(name) = args.first() else {
            return Err("service uninstall requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.service_service().uninstall(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_action(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn dispatch_service_command(
        &self,
        action: &str,
        rest: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "discover" => self.handle_service_discover(rest),
            "adopt-global" => self.handle_service_adopt_global(rest),
            "restore-global" => self.handle_service_restore_global(rest),
            "install" => self.handle_service_install(rest),
            "list" => self.handle_service_list(rest),
            "status" => self.handle_service_status(rest),
            "logs" => self.handle_service_logs(rest),
            "start" => self.handle_service_start(rest),
            "stop" => self.handle_service_stop(rest),
            "restart" => self.handle_service_restart(rest),
            "uninstall" => self.handle_service_uninstall(rest),
            _ => Err(format!("unknown service command: {action}")),
        }
    }
}
