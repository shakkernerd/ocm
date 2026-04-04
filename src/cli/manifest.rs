use std::path::{Path, PathBuf};

use super::{Cli, render};
use crate::manifest::find_manifest_path;

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
