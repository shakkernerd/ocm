use std::collections::BTreeMap;

use crate::types::AddLauncherOptions;

use super::Cli;

impl Cli {
    pub(super) fn handle_launcher_add(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
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

        self.stdout_line(format!("Added launcher {}", meta.name));
        self.stdout_line(format!("  command: {}", meta.command));
        if let Some(cwd) = meta.cwd.as_deref() {
            self.stdout_line(format!("  cwd: {cwd}"));
        }
        Ok(0)
    }

    pub(super) fn handle_launcher_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let launchers = self.launcher_service().list()?;
        if json_flag {
            self.print_json(&launchers)?;
            return Ok(0);
        }
        if launchers.is_empty() {
            self.stdout_line("No launchers.");
            return Ok(0);
        }
        for meta in launchers {
            let mut bits = vec![meta.name, meta.command];
            if let Some(cwd) = meta.cwd {
                bits.push(format!("cwd={cwd}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    pub(super) fn handle_launcher_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().show(name)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        let mut lines = BTreeMap::new();
        lines.insert("kind".to_string(), meta.kind.clone());
        lines.insert("name".to_string(), meta.name.clone());
        lines.insert("command".to_string(), meta.command.clone());
        lines.insert(
            "createdAt".to_string(),
            meta.created_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
        );
        lines.insert(
            "updatedAt".to_string(),
            meta.updated_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|error| error.to_string())?,
        );
        if let Some(cwd) = meta.cwd {
            lines.insert("cwd".to_string(), cwd);
        }
        if let Some(description) = meta.description {
            lines.insert("description".to_string(), description);
        }
        for (key, value) in lines {
            self.stdout_line(format!("{key}: {value}"));
        }
        Ok(0)
    }

    pub(super) fn handle_launcher_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let Some(name) = args.first() else {
            return Err("launcher name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.launcher_service().remove(name)?;
        self.stdout_line(format!("Removed launcher {}", meta.name));
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
