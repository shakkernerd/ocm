use super::{Cli, render};
use crate::manifest::{
    ManifestReconcileOptions, plan_manifest_application_with_service,
    reconcile_manifest_with_options, resolve_manifest,
};
use crate::store::get_environment;

impl Cli {
    pub(super) fn handle_sync_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "sync")?;
        let (args, dry_run) = Self::consume_flag(args, "--dry-run");
        let search_root = self.resolve_manifest_input(args, "sync")?;

        let resolved = resolve_manifest(&search_root)?
            .ok_or_else(|| format!("no ocm.yaml found from {}", search_root.to_string_lossy()))?;
        let env_name = resolved.manifest.env.name.clone();
        let current_env = get_environment(&env_name, &self.env, &self.cwd).ok();
        let current_env = current_env.ok_or_else(|| {
            format!(
                "manifest env \"{}\" does not exist yet; run \"{} up\" first",
                env_name,
                self.command_example()
            )
        })?;

        if dry_run {
            let current_service_installed = self
                .service_service()
                .status_fast(&env_name)
                .ok()
                .map(|summary| summary.installed);
            let plan = plan_manifest_application_with_service(
                &resolved.manifest,
                Some(&current_env),
                current_service_installed,
            );
            let summary = render::manifest::UpSummary {
                found: true,
                path: Some(resolved.path.to_string_lossy().into_owned()),
                search_root: search_root.to_string_lossy().into_owned(),
                dry_run: true,
                env_exists: true,
                env_root: Some(current_env.root.clone()),
                plan: Some(plan),
                result: None,
            };

            if json_flag {
                self.print_json(&summary)?;
            } else {
                self.stdout_lines(render::manifest::sync_summary(&summary, profile));
            }
            return Ok(0);
        }

        let result = self.with_progress(
            format!("Synchronizing manifest env {}", resolved.manifest.env.name),
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
            self.stdout_lines(render::manifest::sync_summary(&summary, profile));
        }

        Ok(0)
    }
}
