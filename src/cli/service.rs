use super::{Cli, render};

impl Cli {
    pub(super) fn handle_service_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let services = self.service_service().list()?;
        if json_flag {
            self.print_json(&services)?;
            return Ok(0);
        }

        self.stdout_lines(render::service::service_list(&services));
        Ok(0)
    }

    pub(super) fn handle_service_status(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, all_flag) = Self::consume_flag(args, "--all");

        if all_flag {
            Self::assert_no_extra_args(&args)?;
            let services = self.service_service().list()?;
            if json_flag {
                self.print_json(&services)?;
                return Ok(0);
            }

            self.stdout_lines(render::service::service_list(&services));
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

        self.stdout_lines(render::service::service_status(&summary));
        Ok(0)
    }

    pub(super) fn dispatch_service_command(
        &self,
        action: &str,
        rest: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "list" => self.handle_service_list(rest),
            "status" => self.handle_service_status(rest),
            _ => Err(format!("unknown service command: {action}")),
        }
    }
}
