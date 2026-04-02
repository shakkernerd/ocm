use crate::runtime::{
    InstallRuntimeFromOfficialReleaseOptions, OfficialRuntimePrepareAction, RuntimeService,
};

use super::{Cli, render};

impl Cli {
    pub(super) fn handle_release_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "release list")?;
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        Self::assert_no_extra_args(&args)?;

        if version.is_some() && channel.is_some() {
            return Err("release list accepts only one of --version or --channel".to_string());
        }

        let releases = self
            .runtime_service()
            .official_openclaw_release_catalog(version.as_deref(), channel.as_deref())?;
        if json_flag {
            self.print_json(&releases)?;
            return Ok(0);
        }

        self.stdout_lines(render::release::release_list(&releases, profile));
        Ok(0)
    }

    pub(super) fn handle_release_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "release show")?;
        let (args, version_flag) = Self::consume_option(args, "--version")?;
        let version_flag = Self::require_option_value(version_flag, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        if args.len() > 1 {
            Self::assert_no_extra_args(&args[1..])?;
        }
        if args.first().is_some() && (version_flag.is_some() || channel.is_some()) {
            return Err(
                "release show accepts either a version argument, --version, or --channel"
                    .to_string(),
            );
        }
        if version_flag.is_some() && channel.is_some() {
            return Err("release show accepts only one of --version or --channel".to_string());
        }

        let version = version_flag.or_else(|| args.first().cloned());
        if version.is_none() && channel.is_none() {
            return Err("release show requires a version or --channel".to_string());
        }

        let release = self
            .runtime_service()
            .official_openclaw_release_catalog(version.as_deref(), channel.as_deref())?
            .into_iter()
            .next()
            .ok_or_else(|| {
                if let Some(version) = version.as_deref() {
                    format!("OpenClaw release version \"{version}\" was not found")
                } else if let Some(channel) = channel.as_deref() {
                    format!("OpenClaw release channel \"{channel}\" was not found")
                } else {
                    "OpenClaw release was not found".to_string()
                }
            })?;
        if json_flag {
            self.print_json(&release)?;
            return Ok(0);
        }

        self.stdout_lines(render::release::release_show(
            &release,
            profile,
            &self.command_example(),
        )?);
        Ok(0)
    }

    pub(super) fn handle_release_install(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "release install")?;
        let (args, force) = Self::consume_flag(args, "--force");
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        if args.len() > 1 {
            Self::assert_no_extra_args(&args[1..])?;
        }

        if version.is_some() && channel.is_some() {
            return Err("release install accepts only one of --version or --channel".to_string());
        }
        if version.is_none() && channel.is_none() {
            return Err("release install requires --version or --channel".to_string());
        }

        let runtime_name = RuntimeService::canonical_official_openclaw_runtime_name(
            version.as_deref(),
            channel.as_deref(),
        )?;
        if let Some(name) = args.first() {
            if name != runtime_name.as_str() {
                return Err(format!(
                    "release install uses the canonical runtime name \"{runtime_name}\" for this selector"
                ));
            }
        }
        if let Some(code) = self.ensure_official_release_host_ready(Some(profile), json_flag)? {
            return Ok(code);
        }

        let (meta, action) =
            self.with_progress(format!("Installing runtime {runtime_name}"), || {
                self.runtime_service().prepare_official_openclaw_runtime(
                    InstallRuntimeFromOfficialReleaseOptions {
                        name: runtime_name.clone(),
                        version,
                        channel,
                        description,
                        force,
                    },
                )
            })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_lines(match action {
            OfficialRuntimePrepareAction::Installed => {
                render::runtime::runtime_installed(&meta, profile, &self.command_example())
            }
            OfficialRuntimePrepareAction::Reused => {
                render::runtime::runtime_reused(&meta, profile, &self.command_example())
            }
            OfficialRuntimePrepareAction::Updated => {
                render::runtime::runtime_updated(&meta, profile, &self.command_example())
            }
        });
        Ok(0)
    }

    pub(super) fn dispatch_release_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => {
                self.dispatch_help_command(vec!["release".to_string()])
            }
            "install" => self.handle_release_install(args),
            "list" => self.handle_release_list(args),
            "show" => self.handle_release_show(args),
            other => Err(format!("unknown release command: {other}")),
        }
    }
}
