use std::path::Path;

use super::{Cli, render};
use crate::migrate::{
    MigrateHomeOptions, inspect_migration_source, migrate_plain_openclaw_home_with_manifest,
    plan_migration,
};
use crate::store::resolve_absolute_path;

impl Cli {
    pub(super) fn dispatch_adopt_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => self.dispatch_help_command(vec!["adopt".to_string()]),
            "import" => self.handle_adopt_import(args),
            "inspect" => self.handle_adopt_inspect(args),
            "plan" => self.handle_adopt_plan(args),
            _ => Err(format!("unknown adopt command: {action}")),
        }
    }

    pub(super) fn handle_migrate_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "migrate")?;
        let (args, manifest_value) = Self::consume_option(args, "--manifest")?;
        let manifest_value = Self::require_option_value(manifest_value, "--manifest")?;
        let (args, root_value) = Self::consume_option(args, "--root")?;
        let root_value = Self::require_option_value(root_value, "--root")?;
        let (args, name_value) = Self::consume_option(args, "--name")?;
        let name_value = Self::require_option_value(name_value, "--name")?;
        if args.len() > 2 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let positional_name = args.first().cloned();
        if positional_name.is_some() && name_value.is_some() {
            return Err("migrate accepts only one env name from <env> or --name <env>".to_string());
        }
        let env_name = positional_name
            .or(name_value)
            .ok_or_else(|| "migrate requires <env> or --name <env>".to_string())?;
        let source_home = if args.len() == 2 {
            Some(args[1].clone())
        } else {
            None
        };
        let manifest_path = manifest_value
            .as_deref()
            .map(|path| resolve_absolute_path(path, &self.env, &self.cwd))
            .transpose()?;
        let summary = self.with_progress("Migrating existing OpenClaw into OCM", || {
            migrate_plain_openclaw_home_with_manifest(
                MigrateHomeOptions {
                    source_home,
                    name: env_name.clone(),
                    root: root_value.clone(),
                },
                manifest_path.as_deref(),
                &self.env,
                &self.cwd,
            )
        })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::migrate::migration_import(
            &summary,
            &self.command_example(),
            profile,
        ));
        Ok(0)
    }

    fn handle_adopt_import(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "adopt import")?;
        let (args, manifest_value) = Self::consume_option(args, "--manifest")?;
        let manifest_value = Self::require_option_value(manifest_value, "--manifest")?;
        let (args, root_value) = Self::consume_option(args, "--root")?;
        let root_value = Self::require_option_value(root_value, "--root")?;
        let (args, name_value) = Self::consume_option(args, "--name")?;
        let env_name = Self::require_option_value(name_value, "--name")?
            .ok_or_else(|| "--name is required".to_string())?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let source_home = args.first().cloned();
        let manifest_path = manifest_value
            .as_deref()
            .map(|path| resolve_absolute_path(path, &self.env, &self.cwd))
            .transpose()?;
        let summary = self.with_progress("Migrating plain OpenClaw home", || {
            migrate_plain_openclaw_home_with_manifest(
                MigrateHomeOptions {
                    source_home,
                    name: env_name.clone(),
                    root: root_value.clone(),
                },
                manifest_path.as_deref(),
                &self.env,
                &self.cwd,
            )
        })?;

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(render::migrate::migration_import(
            &summary,
            &self.command_example(),
            profile,
        ));
        Ok(0)
    }

    fn handle_adopt_inspect(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "adopt inspect")?;
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

    fn handle_adopt_plan(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "adopt plan")?;
        let (args, manifest_value) = Self::consume_option(args, "--manifest")?;
        let manifest_value = Self::require_option_value(manifest_value, "--manifest")?;
        let (args, root_value) = Self::consume_option(args, "--root")?;
        let root_value = Self::require_option_value(root_value, "--root")?;
        let (args, name_value) = Self::consume_option(args, "--name")?;
        let env_name = Self::require_option_value(name_value, "--name")?
            .ok_or_else(|| "--name is required".to_string())?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let explicit = args.first().map(|value| Path::new(value.as_str()));
        let manifest_path = manifest_value
            .as_deref()
            .map(|path| resolve_absolute_path(path, &self.env, &self.cwd))
            .transpose()?;
        let summary = plan_migration(
            explicit,
            manifest_path.as_deref(),
            &env_name,
            root_value.as_deref(),
            &self.env,
            &self.cwd,
        )?;

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::migrate::migration_plan(&summary, profile));
        }

        Ok(0)
    }
}
