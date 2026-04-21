use std::io::{self, Write};

use super::render::RenderProfile;
use super::{Cli, render};
use crate::logs::LogComponentSummary;

struct PrettyLogWriter<'a, W: Write> {
    inner: &'a mut W,
    profile: RenderProfile,
    pending: String,
}

impl<'a, W: Write> PrettyLogWriter<'a, W> {
    fn new(inner: &'a mut W, profile: RenderProfile) -> Self {
        Self {
            inner,
            profile,
            pending: String::new(),
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let rendered = render::logs::render_log_text(&self.pending, self.profile);
        self.inner
            .write_all(rendered.as_bytes())
            .map_err(|error| error.to_string())?;
        self.pending.clear();
        self.inner.flush().map_err(|error| error.to_string())
    }
}

impl<W: Write> Write for PrettyLogWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.push_str(&String::from_utf8_lossy(buf));
        while let Some(newline_index) = self.pending.find('\n') {
            let line = self.pending[..=newline_index].to_string();
            self.pending.drain(..=newline_index);
            let rendered = render::logs::render_log_text(&line, self.profile);
            self.inner.write_all(rendered.as_bytes())?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl Cli {
    pub(super) fn handle_logs_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "logs")?;
        let (args, follow) = Self::consume_flag(args, "--follow");
        let (args, stream_raw) = Self::consume_option(args, "--stream")?;
        let (args, tail_raw) = Self::consume_option(args, "--tail")?;
        let tail_lines = match tail_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--tail")? as usize),
            None => Some(50),
        };

        if json_flag && follow {
            return Err("logs cannot combine --json with --follow".to_string());
        }

        let Some(name) = args.first() else {
            return Err("logs requires <env>".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let stream = match stream_raw.as_deref() {
            None => "all",
            Some("info") => "stdout",
            Some("error") => "stderr",
            Some(other) => {
                return Err(format!(
                    "unsupported log stream level: {other}; use --stream info or --stream error"
                ));
            }
        };
        if follow {
            if profile.pretty {
                self.stdout_lines(render::logs::log_header(
                    name,
                    &follow_components(self.log_service().targets(name, stream)?),
                    tail_lines,
                    true,
                    profile,
                ));
            }
            let stdout = io::stdout();
            let mut handle = stdout.lock();
            if profile.pretty {
                let mut writer = PrettyLogWriter::new(&mut handle, profile);
                self.log_service()
                    .follow(name, stream, tail_lines, &mut writer)?;
                writer.finish()?;
            } else {
                self.log_service()
                    .follow(name, stream, tail_lines, &mut handle)?;
            }
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
                &summary.components,
                summary.tail_lines,
                false,
                profile,
            ));
        }

        let stdout = io::stdout();
        let mut handle = stdout.lock();
        let content = if profile.pretty {
            render::logs::render_log_text(&summary.content, profile)
        } else {
            summary.content
        };
        handle
            .write_all(content.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(0)
    }
}

fn follow_components(targets: Vec<crate::logs::LogTarget>) -> Vec<LogComponentSummary> {
    targets
        .into_iter()
        .map(|target| LogComponentSummary {
            stream: target.stream,
            source_kind: target.source_kind,
            path: target.path.to_string_lossy().into_owned(),
        })
        .collect()
}
