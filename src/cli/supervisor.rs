use super::{Cli, render};

impl Cli {
    pub(super) fn handle_supervisor_plan(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "supervisor plan")?;
        Self::assert_no_extra_args(&args)?;

        let summary = self.supervisor_service().plan()?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::supervisor::supervisor_state(&summary, profile));
        Ok(0)
    }

    pub(super) fn handle_supervisor_sync(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "supervisor sync")?;
        Self::assert_no_extra_args(&args)?;

        let summary = self.with_progress("Syncing supervisor state", || {
            self.supervisor_service().sync()
        })?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::supervisor::supervisor_state(&summary, profile));
        Ok(0)
    }

    pub(super) fn handle_supervisor_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "supervisor show")?;
        Self::assert_no_extra_args(&args)?;

        let summary = self.supervisor_service().show()?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::supervisor::supervisor_state(&summary, profile));
        Ok(0)
    }

    pub(super) fn dispatch_supervisor_command(
        &self,
        action: &str,
        rest: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => {
                self.dispatch_help_command(vec!["supervisor".to_string()])
            }
            "plan" => self.handle_supervisor_plan(rest),
            "sync" => self.handle_supervisor_sync(rest),
            "show" => self.handle_supervisor_show(rest),
            _ => Err(format!("unknown supervisor command: {action}")),
        }
    }
}
