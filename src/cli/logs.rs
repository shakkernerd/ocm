use std::io::{self, Write};

use super::{Cli, render};

impl Cli {
    pub(super) fn handle_logs_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "logs")?;
        let (args, follow) = Self::consume_flag(args, "--follow");
        let (args, stderr_flag) = Self::consume_flag(args, "--stderr");
        let (args, stdout_flag) = Self::consume_flag(args, "--stdout");
        let (args, tail_raw) = Self::consume_option(args, "--tail")?;
        let tail_lines = match tail_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--tail")? as usize),
            None => Some(50),
        };

        if stdout_flag && stderr_flag {
            return Err("logs accepts only one of --stdout or --stderr".to_string());
        }
        if json_flag && follow {
            return Err("logs cannot combine --json with --follow".to_string());
        }

        let Some(name) = args.first() else {
            return Err("logs requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let stream = if stderr_flag { "stderr" } else { "stdout" };
        if follow {
            let target = self.log_service().target(name, stream)?;
            if profile.pretty {
                self.stdout_lines(render::logs::log_header(
                    name,
                    stream,
                    &target.source_kind,
                    &target.path.to_string_lossy(),
                    tail_lines,
                    true,
                    profile,
                ));
            }
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            self.log_service()
                .follow(name, stream, tail_lines, &mut handle)?;
            return Ok(0);
        }

        let summary = self.log_service().read(name, stream, tail_lines)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }
        if profile.pretty {
            self.stdout_lines(render::logs::log_header(
                &summary.env_name,
                &summary.stream,
                &summary.source_kind,
                &summary.path,
                summary.tail_lines,
                false,
                profile,
            ));
        }

        let stdout = io::stdout();
        let mut handle = stdout.lock();
        handle
            .write_all(summary.content.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(0)
    }
}
