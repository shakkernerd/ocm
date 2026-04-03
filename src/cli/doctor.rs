use super::{Cli, render};
use crate::host;

impl Cli {
    pub(super) fn dispatch_doctor_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => self.dispatch_help_command(vec!["doctor".to_string()]),
            "host" => self.handle_doctor_host(args),
            _ => Err(format!("unknown doctor command: {action}")),
        }
    }

    pub(super) fn ensure_official_release_host_ready(
        &self,
        profile: Option<render::RenderProfile>,
        json_output: bool,
    ) -> Result<Option<i32>, String> {
        let host_ready = host::verify_official_openclaw_runtime_host(&self.env);
        match host::verify_official_openclaw_runtime_support(&self.env) {
            Ok(()) if host_ready.is_ok() => Ok(None),
            Ok(()) => {
                if !json_output {
                    let summary = host::doctor_host(&self.env);
                    self.stdout_lines(render::doctor::host_doctor(
                        &summary,
                        profile.unwrap_or_else(|| self.default_render_profile()),
                        &self.command_example(),
                    ));
                }
                Ok(None)
            }
            Err(error) if json_output => Err(error),
            Err(_) => {
                let summary = host::doctor_host(&self.env);
                self.stdout_lines(render::doctor::host_doctor(
                    &summary,
                    profile.unwrap_or_else(|| self.default_render_profile()),
                    &self.command_example(),
                ));
                Ok(Some(1))
            }
        }
    }

    fn handle_doctor_host(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "doctor host")?;
        let (args, fix_target) = Self::consume_option(args, "--fix")?;
        let fix_target = Self::require_option_value(fix_target, "--fix")?;
        let (args, yes_flag) = Self::consume_flag(args, "--yes");
        Self::assert_no_extra_args(&args)?;

        if yes_flag && fix_target.is_none() {
            return Err("doctor host accepts --yes only with --fix".to_string());
        }

        if let Some(target) = fix_target.as_deref() {
            return self.handle_doctor_host_fix(target, yes_flag, json_flag, profile);
        }

        let summary = host::doctor_host(&self.env);
        let code = if summary.healthy { 0 } else { 1 };
        if json_flag {
            self.print_json(&summary)?;
            return Ok(code);
        }

        self.stdout_lines(render::doctor::host_doctor(
            &summary,
            profile,
            &self.command_example(),
        ));
        Ok(code)
    }

    fn handle_doctor_host_fix(
        &self,
        target: &str,
        yes_flag: bool,
        json_flag: bool,
        profile: render::RenderProfile,
    ) -> Result<i32, String> {
        if !yes_flag {
            return Err(format!(
                "doctor host --fix {target} requires --yes because it changes host software"
            ));
        }

        let summary = match target {
            "git" => self.with_progress("Installing git", || host::fix_git_host_tool(&self.env))?,
            _ => {
                return Err(format!(
                    "doctor host can only fix supported tools; unknown fix target: {target}"
                ));
            }
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::doctor::host_tool_fixed(
                &summary,
                profile,
                &self.command_example(),
            ));
        }

        Ok(if summary.ready { 0 } else { 1 })
    }

    fn default_render_profile(&self) -> render::RenderProfile {
        let color_mode = self.color_mode();
        let pretty_enabled =
            self.stdout_is_terminal() || matches!(color_mode, super::ColorMode::Always);
        if pretty_enabled {
            render::RenderProfile::pretty(self.color_output_enabled_for(true, color_mode))
        } else {
            render::RenderProfile::raw()
        }
    }
}
