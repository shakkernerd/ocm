use super::{Cli, render};
use crate::manifest::{
    ManifestReconcileOptions, plan_manifest_application, reconcile_manifest_with_options,
    resolve_manifest,
};
use crate::store::{get_environment, resolve_absolute_path};

impl Cli {
    pub(super) fn handle_up_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "up")?;
        let (args, dry_run) = Self::consume_flag(args, "--dry-run");
        let (args, manifest_value) = Self::consume_option(args, "--manifest")?;
        let explicit_manifest = Self::require_option_value(manifest_value, "--manifest")?;
        if explicit_manifest.is_some() && !args.is_empty() {
            return Err("up accepts only one of [path] or --manifest <path>".to_string());
        }
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let search_root = if let Some(path) = explicit_manifest.as_ref() {
            resolve_absolute_path(path, &self.env, &self.cwd)?
        } else {
            args.first()
                .map(|value| self.resolve_manifest_search_root(value))
                .transpose()?
                .unwrap_or_else(|| self.cwd.clone())
        };

        let resolved = resolve_manifest(&search_root)?
            .ok_or_else(|| format!("no ocm.yaml found from {}", search_root.to_string_lossy()))?;

        if dry_run {
            let env_name = resolved.manifest.env.name.clone();
            let current_env = get_environment(&env_name, &self.env, &self.cwd).ok();
            let plan = plan_manifest_application(&resolved.manifest, current_env.as_ref());
            let summary = render::manifest::UpSummary {
                found: true,
                path: Some(resolved.path.to_string_lossy().into_owned()),
                search_root: search_root.to_string_lossy().into_owned(),
                dry_run: true,
                env_exists: current_env.is_some(),
                env_root: current_env.as_ref().map(|meta| meta.root.clone()),
                plan: Some(plan),
                result: None,
            };

            if json_flag {
                self.print_json(&summary)?;
            } else {
                self.stdout_lines(render::manifest::up_summary(&summary, profile));
            }
            return Ok(0);
        }

        let result = self.with_progress(
            format!("Reconciling manifest env {}", resolved.manifest.env.name),
            || {
                reconcile_manifest_with_options(
                    &resolved.path,
                    &resolved.manifest,
                    &self.env,
                    &self.cwd,
                    ManifestReconcileOptions {
                        snapshot_existing_env: true,
                        rollback_on_failure: true,
                    },
                )
            },
        )?;

        let summary = render::manifest::UpSummary {
            found: true,
            path: Some(resolved.path.to_string_lossy().into_owned()),
            search_root: search_root.to_string_lossy().into_owned(),
            dry_run: false,
            env_exists: true,
            env_root: Some(result.env_root.clone()),
            plan: None,
            result: Some(result),
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::manifest::up_summary(&summary, profile));
        }

        Ok(0)
    }
}
