use std::collections::BTreeMap;

use crate::runtime::{
    AddRuntimeOptions, InstallRuntimeFromReleaseOptions, InstallRuntimeFromUrlOptions,
    InstallRuntimeOptions, UpdateRuntimeFromReleaseOptions,
};

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

    pub(super) fn handle_runtime_install(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, force) = Self::consume_flag(args, "--force");
        let (args, path) = Self::consume_option(args, "--path")?;
        let path = Self::require_option_value(path, "--path")?;
        let (args, url) = Self::consume_option(args, "--url")?;
        let url = Self::require_option_value(url, "--url")?;
        let (args, manifest_url) = Self::consume_option(args, "--manifest-url")?;
        let manifest_url = Self::require_option_value(manifest_url, "--manifest-url")?;
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        let (args, description) = Self::consume_option(args, "--description")?;
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let source_count = usize::from(path.is_some())
            + usize::from(url.is_some())
            + usize::from(manifest_url.is_some());
        if source_count > 1 {
            return Err(
                "runtime install accepts only one of --path, --url, or --manifest-url".to_string(),
            );
        }
        if manifest_url.is_none() {
            if version.is_some() {
                return Err(
                    "runtime install only supports --version with --manifest-url".to_string(),
                );
            }
            if channel.is_some() {
                return Err(
                    "runtime install only supports --channel with --manifest-url".to_string(),
                );
            }
        }

        let meta = match (path, url, manifest_url) {
            (Some(path), None, None) => self.runtime_service().install(InstallRuntimeOptions {
                name: name.clone(),
                path,
                description,
                force,
            })?,
            (None, Some(url), None) => {
                self.runtime_service()
                    .install_from_url(InstallRuntimeFromUrlOptions {
                        name: name.clone(),
                        url,
                        description,
                        force,
                    })?
            }
            (None, None, Some(manifest_url)) => {
                if version.is_some() && channel.is_some() {
                    return Err(
                        "runtime install with --manifest-url accepts only one of --version or --channel"
                            .to_string(),
                    );
                }
                if version.is_none() && channel.is_none() {
                    return Err(
                        "runtime install with --manifest-url requires --version or --channel"
                            .to_string(),
                    );
                }
                self.runtime_service()
                    .install_from_release(InstallRuntimeFromReleaseOptions {
                        name: name.clone(),
                        manifest_url,
                        version,
                        channel,
                        description,
                        force,
                    })?
            }
            (None, None, None) => {
                return Err("runtime install requires --path, --url, or --manifest-url".to_string());
            }
            _ => unreachable!("source_count guards conflicting runtime install sources"),
        };

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Installed runtime {}", meta.name));
        self.stdout_line(format!("  binary path: {}", meta.binary_path));
        if let Some(install_root) = meta.install_root.as_deref() {
            self.stdout_line(format!("  install root: {install_root}"));
        }
        Ok(0)
    }

    pub(super) fn handle_runtime_releases(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        let (args, manifest_url) = Self::consume_option(args, "--manifest-url")?;
        let manifest_url = Self::require_option_value(manifest_url, "--manifest-url")?;
        Self::assert_no_extra_args(&args)?;

        if manifest_url.is_none() {
            return Err("runtime releases requires --manifest-url".to_string());
        }
        if version.is_some() && channel.is_some() {
            return Err(
                "runtime releases accepts only one of --version or --channel".to_string(),
            );
        }

        let releases = self
            .runtime_service()
            .releases_from_manifest(
                manifest_url.as_deref().unwrap_or_default(),
                version.as_deref(),
                channel.as_deref(),
            )?;
        if json_flag {
            self.print_json(&releases)?;
            return Ok(0);
        }
        if releases.is_empty() {
            self.stdout_line("No runtime releases.");
            return Ok(0);
        }
        for release in releases {
            let mut bits = vec![release.version, release.url];
            if let Some(channel) = release.channel {
                bits.push(format!("channel={channel}"));
            }
            if let Some(sha256) = release.sha256 {
                bits.push(format!("sha256={sha256}"));
            }
            self.stdout_line(bits.join("  "));
        }
        Ok(0)
    }

    pub(super) fn handle_runtime_update(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, all_flag) = Self::consume_flag(args, "--all");
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        if all_flag {
            Self::assert_no_extra_args(&args)?;
            let batch = self
                .runtime_service()
                .update_all_from_release(version, channel)?;
            let code = if batch.failed > 0 { 1 } else { 0 };

            if json_flag {
                self.print_json(&batch)?;
                return Ok(code);
            }

            if batch.results.is_empty() {
                self.stdout_line("No runtimes.");
                return Ok(code);
            }

            self.stdout_line(format!(
                "Runtime update summary: total={} updated={} skipped={} failed={}",
                batch.count, batch.updated, batch.skipped, batch.failed
            ));
            for summary in batch.results {
                let mut bits = vec![
                    summary.name,
                    format!("outcome={}", summary.outcome),
                    format!("source={}", summary.source_kind),
                ];
                if let Some(binary_path) = summary.binary_path {
                    bits.push(binary_path);
                }
                if let Some(release_version) = summary.release_version {
                    bits.push(format!("release={release_version}"));
                }
                if let Some(release_channel) = summary.release_channel {
                    bits.push(format!("channel={release_channel}"));
                }
                if let Some(issue) = summary.issue {
                    bits.push(format!("issue={issue}"));
                }
                self.stdout_line(bits.join("  "));
            }
            return Ok(code);
        }
        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let meta = self
            .runtime_service()
            .update_from_release(UpdateRuntimeFromReleaseOptions {
                name: name.clone(),
                version,
                channel,
            })?;

        if json_flag {
            self.print_json(&meta)?;
            return Ok(0);
        }

        self.stdout_line(format!("Updated runtime {}", meta.name));
        self.stdout_line(format!("  binary path: {}", meta.binary_path));
        if let Some(install_root) = meta.install_root.as_deref() {
            self.stdout_line(format!("  install root: {install_root}"));
        }
        Ok(0)
    }

    pub(super) fn handle_runtime_verify(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, all_flag) = Self::consume_flag(args, "--all");
        if all_flag {
            Self::assert_no_extra_args(&args)?;
            let summaries = self.runtime_service().verify_all()?;
            let code = if summaries.iter().all(|summary| summary.healthy) {
                0
            } else {
                1
            };

            if json_flag {
                self.print_json(&summaries)?;
                return Ok(code);
            }

            if summaries.is_empty() {
                self.stdout_line("No runtimes.");
                return Ok(code);
            }

            for summary in summaries {
                let mut bits = vec![
                    summary.name,
                    summary.binary_path,
                    format!("source={}", summary.source_kind),
                    format!("healthy={}", summary.healthy),
                ];
                if let Some(issue) = summary.issue {
                    bits.push(format!("issue={issue}"));
                }
                self.stdout_line(bits.join("  "));
            }
            return Ok(code);
        }

        let Some(name) = args.first() else {
            return Err("runtime name is required".to_string());
        };
        Self::assert_no_extra_args(&args[1..])?;

        let summary = self.runtime_service().verify(name)?;
        let code = if summary.healthy { 0 } else { 1 };

        if json_flag {
            self.print_json(&summary)?;
            return Ok(code);
        }

        self.stdout_line(format!("name: {}", summary.name));
        self.stdout_line(format!("binaryPath: {}", summary.binary_path));
        self.stdout_line(format!("sourceKind: {}", summary.source_kind));
        self.stdout_line(format!("healthy: {}", summary.healthy));
        if let Some(source_path) = summary.source_path {
            self.stdout_line(format!("sourcePath: {source_path}"));
        }
        if let Some(source_url) = summary.source_url {
            self.stdout_line(format!("sourceUrl: {source_url}"));
        }
        if let Some(source_manifest_url) = summary.source_manifest_url {
            self.stdout_line(format!("sourceManifestUrl: {source_manifest_url}"));
        }
        if let Some(source_sha256) = summary.source_sha256 {
            self.stdout_line(format!("sourceSha256: {source_sha256}"));
        }
        if let Some(release_version) = summary.release_version {
            self.stdout_line(format!("releaseVersion: {release_version}"));
        }
        if let Some(release_channel) = summary.release_channel {
            self.stdout_line(format!("releaseChannel: {release_channel}"));
        }
        if let Some(install_root) = summary.install_root {
            self.stdout_line(format!("installRoot: {install_root}"));
        }
        if let Some(issue) = summary.issue {
            self.stdout_line(format!("issue: {issue}"));
        }
        Ok(code)
    }

    pub(super) fn dispatch_runtime_command(
        &self,
        action: &str,
        rest: Vec<String>,
    ) -> Result<i32, String> {
        match action {
            "add" => self.handle_runtime_add(rest),
            "install" => self.handle_runtime_install(rest),
            "update" => self.handle_runtime_update(rest),
            "releases" => self.handle_runtime_releases(rest),
            "list" => self.handle_runtime_list(rest),
            "show" => self.handle_runtime_show(rest),
            "verify" => self.handle_runtime_verify(rest),
            "which" => self.handle_runtime_which(rest),
            "remove" | "rm" => self.handle_runtime_remove(rest),
            _ => Err(format!("unknown runtime command: {action}")),
        }
    }
}
