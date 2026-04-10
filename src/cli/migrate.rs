use std::path::Path;

use super::{Cli, render};
use crate::migrate::{
    MigrateHomeOptions, inspect_migration_source, migrate_plain_openclaw_home_with_manifest,
    plan_migration,
};
use crate::store::resolve_absolute_path;

impl Cli {
    fn prepend_human_output_flags(
        mut args: Vec<String>,
        json_flag: bool,
        profile: render::RenderProfile,
    ) -> Vec<String> {
        if json_flag {
            args.insert(0, "--json".to_string());
        } else if !profile.pretty {
            args.insert(0, "--raw".to_string());
        }
        args
    }

    fn migration_name_option_follows_positional(args: &[String]) -> bool {
        let mut saw_positional = false;
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            if arg == "--name" {
                return saw_positional;
            }
            if arg.starts_with("--name=") {
                return saw_positional;
            }
            if matches!(arg.as_str(), "--manifest" | "--root") {
                index += 2;
                continue;
            }
            if arg.starts_with("--manifest=") || arg.starts_with("--root=") {
                index += 1;
                continue;
            }
            if !arg.starts_with('-') {
                saw_positional = true;
            }
            index += 1;
        }
        false
    }

    fn reject_mixed_migrate_alias_syntax(args: &[String]) -> Result<(), String> {
        let mut index = 0;
        let mut saw_frontdoor_flag = false;
        while index < args.len() {
            let arg = &args[index];
            if matches!(arg.as_str(), "--manifest" | "--root" | "--name") {
                saw_frontdoor_flag = true;
                index += 2;
                continue;
            }
            if arg.starts_with("--manifest=")
                || arg.starts_with("--root=")
                || arg.starts_with("--name=")
            {
                saw_frontdoor_flag = true;
                index += 1;
                continue;
            }
            if saw_frontdoor_flag && matches!(arg.as_str(), "inspect" | "plan" | "import") {
                return Err(format!(
                    "mixed migrate syntax: use `migrate {arg} ...` for the alias form or `migrate <env> ...` for direct import, but not both"
                ));
            }
            index += 1;
        }
        Ok(())
    }

    fn parse_migrate_target(
        args: Vec<String>,
        name_value: Option<String>,
        name_follows_positional: bool,
    ) -> Result<(String, Option<String>), String> {
        if args.len() > 2 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let (env_name, source_home) = if let Some(env_name) = name_value {
            if args.len() > 1 || (name_follows_positional && !args.is_empty()) {
                return Err(
                    "migrate accepts only one env name from <env> or --name <env>".to_string(),
                );
            }
            (env_name, args.first().cloned())
        } else {
            let positional_name = args.first().cloned();
            let env_name = positional_name
                .ok_or_else(|| "migrate requires <env> or --name <env>".to_string())?;
            (env_name, args.get(1).cloned())
        };

        Ok((env_name, source_home))
    }

    fn parse_adopt_target(
        args: Vec<String>,
        name_value: Option<String>,
    ) -> Result<(String, Option<String>), String> {
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }
        let env_name = name_value.ok_or_else(|| "--name is required".to_string())?;
        Ok((env_name, args.first().cloned()))
    }

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
        if let Some(action) = args.first().map(String::as_str)
            && matches!(action, "inspect" | "plan" | "import")
        {
            return self.dispatch_adopt_command(
                action,
                Self::prepend_human_output_flags(args[1..].to_vec(), json_flag, profile),
            );
        }
        Self::reject_mixed_migrate_alias_syntax(&args)?;

        let (args, manifest_value) = Self::consume_option(args, "--manifest")?;
        let manifest_value = Self::require_option_value(manifest_value, "--manifest")?;
        let (args, root_value) = Self::consume_option(args, "--root")?;
        let root_value = Self::require_option_value(root_value, "--root")?;
        let name_follows_positional = Self::migration_name_option_follows_positional(&args);
        let (args, name_value) = Self::consume_option(args, "--name")?;
        let name_value = Self::require_option_value(name_value, "--name")?;
        let (env_name, source_home) =
            Self::parse_migrate_target(args, name_value, name_follows_positional)?;
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
        let name_value = Self::require_option_value(name_value, "--name")?;
        let (env_name, source_home) = Self::parse_adopt_target(args, name_value)?;
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
        let name_value = Self::require_option_value(name_value, "--name")?;
        let (env_name, source_home) = Self::parse_adopt_target(args, name_value)?;
        let explicit = source_home.as_deref().map(Path::new);
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
