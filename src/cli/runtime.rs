use std::collections::BTreeMap;

use crate::types::AddRuntimeOptions;

use super::Cli;

impl Cli {
    pub(super) fn handle_runtime_add(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, path) = Self::consume_option(args, "--path")?;
        let path = Self::require_option_value(path, "--path")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.runtime_service().add(AddRuntimeOptions {
            name: name.clone(),
            path: path.unwrap_or_default(),
            description,
        })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Added runtime {}", meta.name));
        self.stdout_line(format!("  binary path: {}", meta.binary_path));
        Ok(0)
    }

    pub(super) fn handle_runtime_list(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        Self::assert_no_extra_args(&args)?;

        let runtimes = self.runtime_service().list()?;
        if json_flag {
            self.print_json(&runtimes)?;
            return Ok(0);
        }
        if runtimes.is_empty() {
            self.stdout_line("No runtimes.");
            return Ok(0);
        }
        for meta in runtimes {
            let mut bits = vec![
                meta.name,
                meta.binary_path,
                format!("source={}", meta.source_kind.as_str()),
            ];
            if let Some(release_version) = meta.release_version {
                bits.push(format!("release={release_version}"));
            }
            if let Some(release_channel) = meta.release_channel {
                bits.push(format!("channel={release_channel}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    pub(super) fn handle_runtime_show(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.runtime_service().show(name)?;
        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        let mut lines = BTreeMap::new();
        lines.insert("kind".to_string(), meta.kind.clone());
        lines.insert("name".to_string(), meta.name.clone());
        lines.insert("binaryPath".to_string(), meta.binary_path.clone());
        lines.insert(
            "sourceKind".to_string(),
            meta.source_kind.as_str().to_string(),
        );
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
        if let Some(description) = meta.description {
            lines.insert("description".to_string(), description);
        }
        if let Some(source_path) = meta.source_path {
            lines.insert("sourcePath".to_string(), source_path);
        }
        if let Some(source_url) = meta.source_url {
            lines.insert("sourceUrl".to_string(), source_url);
        }
        if let Some(source_manifest_url) = meta.source_manifest_url {
            lines.insert("sourceManifestUrl".to_string(), source_manifest_url);
        }
        if let Some(source_sha256) = meta.source_sha256 {
            lines.insert("sourceSha256".to_string(), source_sha256);
        }
        if let Some(release_version) = meta.release_version {
            lines.insert("releaseVersion".to_string(), release_version);
        }
        if let Some(release_channel) = meta.release_channel {
            lines.insert("releaseChannel".to_string(), release_channel);
        }
        if let Some(release_selector_kind) = meta.release_selector_kind {
            lines.insert(
                "releaseSelectorKind".to_string(),
                release_selector_kind.as_str().to_string(),
            );
        }
        if let Some(release_selector_value) = meta.release_selector_value {
            lines.insert("releaseSelectorValue".to_string(), release_selector_value);
        }
        if let Some(install_root) = meta.install_root {
            lines.insert("installRoot".to_string(), install_root);
        }
        for (key, value) in lines {
            self.stdout_line(format!("{key}: {value}"));
        }
        Ok(0)
    }

    pub(super) fn handle_runtime_which(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.runtime_service().which(name)?;
        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }
        self.stdout_line(summary.binary_path);
        Ok(0)
    }

    pub(super) fn handle_runtime_remove(&self, args: Vec<String>) -> Result<i32, String> {
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self.runtime_service().remove(name)?;
        self.stdout_line(format!("Removed runtime {}", meta.name));
        Ok(0)
    }
}
