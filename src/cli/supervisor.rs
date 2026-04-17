use super::{Cli, render};

impl Cli {
    pub(super) fn handle_supervisor_logs(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, stderr_flag) = Self::consume_flag(args, "--stderr");
        let (args, stdout_flag) = Self::consume_flag(args, "--stdout");
        let (args, tail_raw) = Self::consume_option(args, "--tail")?;
        let tail_lines = match tail_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--tail")? as usize),
            None => None,
        };
        if stdout_flag && stderr_flag {
            return Err("supervisor logs accepts only one of --stdout or --stderr".to_string());
        }

        let Some(name) = args.first() else {
            return Err("supervisor logs requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let stream = if stderr_flag { "stderr" } else { "stdout" };
        let summary = self.supervisor_service().logs(name, stream, tail_lines)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_text(&summary.content)?;
        Ok(0)
    }

    pub(super) fn handle_supervisor_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "supervisor run")?;
        let (args, once) = Self::consume_flag(args, "--once");
        Self::assert_no_extra_args(&args)?;

        let summary = self.supervisor_service().run(once)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::supervisor::supervisor_run(&summary, profile));
        Ok(
            if once && summary.child_results.iter().any(|result| !result.success) {
                1
            } else {
                0
            },
        )
    }

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

    pub(super) fn handle_supervisor_status(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "supervisor status")?;
        Self::assert_no_extra_args(&args)?;

        let summary = self.supervisor_service().status()?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::supervisor::supervisor_status(&summary, profile));
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
            "run" => self.handle_supervisor_run(rest),
            "logs" => self.handle_supervisor_logs(rest),
            "sync" => self.handle_supervisor_sync(rest),
            "show" => self.handle_supervisor_show(rest),
            "status" => self.handle_supervisor_status(rest),
            _ => Err(format!("unknown supervisor command: {action}")),
        }
    }
}
