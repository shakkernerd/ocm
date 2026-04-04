use std::path::{Path, PathBuf};

use super::{Cli, render};
use crate::manifest::{find_manifest_path, plan_manifest_application, resolve_manifest};
use crate::store::get_environment;

impl Cli {
    pub(super) fn dispatch_manifest_command(
        &self,
        action: &str,
        args: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "" | "help" | "--help" | "-h" => {
                self.dispatch_help_command(vec!["manifest".to_string()])
            }
            "path" => self.handle_manifest_path(args),
            "drift" => self.handle_manifest_drift(args),
            "plan" => self.handle_manifest_plan(args),
            "show" => self.handle_manifest_show(args),
            "resolve" => self.handle_manifest_resolve(args),
            _ => Err(format!("unknown manifest command: {action}")),
        }
    }

    fn handle_manifest_path(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "manifest path")?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let search_root = args
            .first()
            .map(|value| self.resolve_manifest_search_root(value))
            .transpose()?
            .unwrap_or_else(|| self.cwd.clone());

        let summary = render::manifest::ManifestPathSummary {
            found: false,
            path: find_manifest_path(&search_root)?.map(|path| path.to_string_lossy().into_owned()),
            search_root: search_root.to_string_lossy().into_owned(),
        };
        let summary = render::manifest::ManifestPathSummary {
            found: summary.path.is_some(),
            ..summary
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::manifest::manifest_path(&summary, profile));
        }

        Ok(0)
    }

    fn handle_manifest_drift(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "manifest drift")?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let search_root = args
            .first()
            .map(|value| self.resolve_manifest_search_root(value))
            .transpose()?
            .unwrap_or_else(|| self.cwd.clone());

        let resolved = resolve_manifest(&search_root)?;
        let summary = if let Some(resolution) = resolved {
            let env_name = resolution.manifest.env.name.clone();
            let current_env = get_environment(&env_name, &self.env, &self.cwd).ok();
            let desired_runtime = resolution.manifest.runtime.as_ref().and_then(|runtime| {
                runtime
                    .name
                    .clone()
                    .or(runtime.version.clone())
                    .or(runtime.channel.clone())
            });
            let desired_launcher = resolution
                .manifest
                .launcher
                .as_ref()
                .and_then(|launcher| launcher.name.clone());
            let current_runtime = current_env
                .as_ref()
                .and_then(|meta| meta.default_runtime.clone());
            let current_launcher = current_env
                .as_ref()
                .and_then(|meta| meta.default_launcher.clone());

            let mut issues = Vec::new();
            if current_env.is_none() {
                issues.push("env is missing".to_string());
            } else {
                if desired_runtime != current_runtime {
                    issues.push(format!(
                        "runtime differs (desired {}, current {})",
                        desired_runtime.as_deref().unwrap_or("none"),
                        current_runtime.as_deref().unwrap_or("none")
                    ));
                }
                if desired_launcher != current_launcher {
                    issues.push(format!(
                        "launcher differs (desired {}, current {})",
                        desired_launcher.as_deref().unwrap_or("none"),
                        current_launcher.as_deref().unwrap_or("none")
                    ));
                }
            }

            render::manifest::ManifestDriftSummary {
                found: true,
                path: Some(resolution.path.to_string_lossy().into_owned()),
                search_root: search_root.to_string_lossy().into_owned(),
                env_name: Some(env_name),
                env_exists: current_env.is_some(),
                current_runtime,
                current_launcher,
                desired_runtime,
                desired_launcher,
                aligned: issues.is_empty(),
                issues,
            }
        } else {
            render::manifest::ManifestDriftSummary {
                found: false,
                path: None,
                search_root: search_root.to_string_lossy().into_owned(),
                env_name: None,
                env_exists: false,
                current_runtime: None,
                current_launcher: None,
                desired_runtime: None,
                desired_launcher: None,
                aligned: false,
                issues: Vec::new(),
            }
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::manifest::manifest_drift(&summary, profile));
        }

        Ok(0)
    }

    fn handle_manifest_plan(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "manifest plan")?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let search_root = args
            .first()
            .map(|value| self.resolve_manifest_search_root(value))
            .transpose()?
            .unwrap_or_else(|| self.cwd.clone());

        let resolved = resolve_manifest(&search_root)?;
        let summary = if let Some(resolution) = resolved {
            let env_name = resolution.manifest.env.name.clone();
            let current_env = get_environment(&env_name, &self.env, &self.cwd).ok();
            let plan = plan_manifest_application(&resolution.manifest, current_env.as_ref());
            render::manifest::ManifestPlanSummary {
                found: true,
                path: Some(resolution.path.to_string_lossy().into_owned()),
                search_root: search_root.to_string_lossy().into_owned(),
                env_exists: current_env.is_some(),
                env_root: current_env.as_ref().map(|meta| meta.root.clone()),
                plan: Some(plan),
            }
        } else {
            render::manifest::ManifestPlanSummary {
                found: false,
                path: None,
                search_root: search_root.to_string_lossy().into_owned(),
                env_exists: false,
                env_root: None,
                plan: None,
            }
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::manifest::manifest_plan(&summary, profile));
        }

        Ok(0)
    }

    fn handle_manifest_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) = self.consume_human_output_flags(args, "manifest show")?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let search_root = args
            .first()
            .map(|value| self.resolve_manifest_search_root(value))
            .transpose()?
            .unwrap_or_else(|| self.cwd.clone());

        let resolved = resolve_manifest(&search_root)?;
        let summary = render::manifest::ManifestShowSummary {
            found: resolved.is_some(),
            path: resolved
                .as_ref()
                .map(|resolution| resolution.path.to_string_lossy().into_owned()),
            search_root: search_root.to_string_lossy().into_owned(),
            manifest: resolved.map(|resolution| resolution.manifest),
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::manifest::manifest_show(&summary, profile));
        }

        Ok(0)
    }

    fn handle_manifest_resolve(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag, profile) =
            self.consume_human_output_flags(args, "manifest resolve")?;
        if args.len() > 1 {
            return Err(format!("unexpected arguments: {}", args.join(" ")));
        }

        let search_root = args
            .first()
            .map(|value| self.resolve_manifest_search_root(value))
            .transpose()?
            .unwrap_or_else(|| self.cwd.clone());

        let resolved = resolve_manifest(&search_root)?;
        let summary = if let Some(resolution) = resolved {
            let env_name = resolution.manifest.env.name.clone();
            let current_env = get_environment(&env_name, &self.env, &self.cwd).ok();
            render::manifest::ManifestResolveSummary {
                found: true,
                path: Some(resolution.path.to_string_lossy().into_owned()),
                search_root: search_root.to_string_lossy().into_owned(),
                env_name: Some(env_name),
                env_exists: current_env.is_some(),
                env_root: current_env.as_ref().map(|meta| meta.root.clone()),
                current_runtime: current_env
                    .as_ref()
                    .and_then(|meta| meta.default_runtime.clone()),
                current_launcher: current_env
                    .as_ref()
                    .and_then(|meta| meta.default_launcher.clone()),
                desired_runtime: resolution.manifest.runtime.as_ref().and_then(|runtime| {
                    runtime
                        .name
                        .clone()
                        .or(runtime.version.clone())
                        .or(runtime.channel.clone())
                }),
                desired_launcher: resolution
                    .manifest
                    .launcher
                    .as_ref()
                    .and_then(|launcher| launcher.name.clone()),
                desired_service_install: resolution
                    .manifest
                    .service
                    .as_ref()
                    .and_then(|service| service.install),
            }
        } else {
            render::manifest::ManifestResolveSummary {
                found: false,
                path: None,
                search_root: search_root.to_string_lossy().into_owned(),
                env_name: None,
                env_exists: false,
                env_root: None,
                current_runtime: None,
                current_launcher: None,
                desired_runtime: None,
                desired_launcher: None,
                desired_service_install: None,
            }
        };

        if json_flag {
            self.print_json(&summary)?;
        } else {
            self.stdout_lines(render::manifest::manifest_resolve(&summary, profile));
        }

        Ok(0)
    }

    fn resolve_manifest_search_root(&self, raw: &str) -> Result<PathBuf, String> {
        let value = raw.trim();
        if value.is_empty() {
            return Err("manifest path requires a non-empty path".to_string());
        }

        let path = Path::new(value);
        if path.is_absolute() {
            Ok(path.to_path_buf())
        } else {
            Ok(self.cwd.join(path))
        }
    }
}
