use crate::launcher::AddLauncherOptions;

use super::{Cli, render};

impl Cli {
    pub(super) fn handle_launcher_add(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "launcher add")?;
        let (args, command) = Self::consume_option(args, "--command")?;
        let command = Self::require_option_value(command, "--command")?;
        let (args, cwd) = Self::consume_option(args, "--cwd")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().add(AddLauncherOptions {
            name: name.clone(),
            command: command.unwrap_or_default(),
            cwd,
            description,
        })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_lines(render::launcher::launcher_added(
            &meta,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn handle_launcher_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "launcher list")?;
        Self::assert_no_extra_args(&args)?;

        let launchers = self.launcher_service().list()?;
        if json_flag {
            self.print_json(&launchers)?;
            return Ok(0);
        }
        self.stdout_lines(render::launcher::launcher_list(&launchers, profile));
        Ok(0)
    }

    pub(super) fn handle_launcher_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "launcher show")?;
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().show(name)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_lines(render::launcher::launcher_show(
            &meta,
            profile,
            &self.command_example(),
        )?);
        Ok(0)
    }

    pub(super) fn handle_launcher_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "launcher remove")?;
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().remove(name)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }
        self.stdout_lines(render::launcher::launcher_removed(
            &meta.name,
            profile,
            &self.command_example(),
        ));
        Ok(0)
    }

    pub(super) fn dispatch_launcher_command(
        &self,
        action: &str,
        rest: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "add" => self.handle_launcher_add(rest),
            "list" => self.handle_launcher_list(rest),
            "show" => self.handle_launcher_show(rest),
            "remove" | "rm" => self.handle_launcher_remove(rest),
            _ => Err(format!("unknown launcher command: {action}")),
        }
    }
}
