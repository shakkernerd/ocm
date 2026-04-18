use super::Cli;

impl Cli {
    fn handle_daemon_run(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, once) = Self::consume_flag(args, "--once");
        Self::assert_no_extra_args(&args)?;

        let summary = self.supervisor_service().run(once)?;
        if json_flag {
            self.print_json(&summary)?;
        }

        Ok(
            if once && summary.child_results.iter().any(|result| !result.success) {
                1
            } else {
                0
            },
        )
    }

    pub(super) fn dispatch_internal_command(
        &self,
        action: &str,
        rest: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "run" => self.handle_daemon_run(rest),
            _ => Err(format!("unknown internal command: {action}")),
        }
    }
}
