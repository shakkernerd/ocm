use std::path::Path;

use super::{Cli, render};
use crate::migrate::inspect_migration_source;

impl Cli {
    pub(super) fn dispatch_migrate_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => {
                self.dispatch_help_command(vec!["migrate".to_string()])
            }
            "inspect" => self.handle_migrate_inspect(args),
            _ => Err(format!("unknown migrate command: {action}")),
        }
    }

    fn handle_migrate_inspect(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "migrate inspect")?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let explicit = args.first().map(|value| Path::new(value.as_str()));
        let summary = inspect_migration_source(explicit, &self.env);

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::migrate::migration_source(&summary, profile));
        }

        Ok(0)
    }
}
