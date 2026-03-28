use serde::Serialize;

use super::Cli;
use crate::env::{CreateEnvironmentOptions, EnvMeta};
use crate::launcher::AddLauncherOptions;
use crate::store::validate_name;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct StartSummary {
    env_name: String,
    created: bool,
    root: String,
    gateway_port: u32,
    gateway_port_source: String,
    default_runtime: Option<String>,
    default_launcher: Option<String>,
    protected: bool,
    onboarding_planned: bool,
    service_requested: bool,
    service_started: bool,
    activate_command: String,
    run_command: String,
    onboard_command: String,
    service_command: String,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum StartOnboardingMode {
    Auto,
    Always,
    Never,
}

#[derive(Clone, Debug)]
pub(super) struct StartRequest {
    pub name: String,
    pub root: Option<String>,
    pub gateway_port: Option<u32>,
    pub protect: bool,
    pub service_requested: bool,
    pub onboarding_mode: StartOnboardingMode,
    pub runtime_name: Option<String>,
    pub launcher_name: Option<String>,
    pub version: Option<String>,
    pub channel: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Clone, Debug)]
enum StartBinding {
    Runtime(String),
    Launcher(String),
}

impl Cli {
    pub(super) fn handle_start_command(&self, args: Vec<String>) -> Result<i32, String> {
        let (args, json_flag) = Self::consume_flag(args, "--json");
        let (args, protect) = Self::consume_flag(args, "--protect");
        let (args, no_onboard) = Self::consume_flag(args, "--no-onboard");
        let (args, onboard) = Self::consume_flag(args, "--onboard");
        let (args, service_requested) = Self::consume_flag(args, "--service");
        let (args, root) = Self::consume_option(args, "--root")?;
        let root = Self::require_option_value(root, "--root")?;
        let (args, port_raw) = Self::consume_option(args, "--port")?;
        let gateway_port = match port_raw.as_deref() {
            Some(raw) => Some(Self::parse_positive_u32(raw, "--port")?),
            None => None,
        };
        let (args, runtime_name) = Self::consume_option(args, "--runtime")?;
        let runtime_name = Self::require_option_value(runtime_name, "--runtime")?;
        let (args, launcher_name) = Self::consume_option(args, "--launcher")?;
        let launcher_name = Self::require_option_value(launcher_name, "--launcher")?;
        let (args, version) = Self::consume_option(args, "--version")?;
        let version = Self::require_option_value(version, "--version")?;
        let (args, channel) = Self::consume_option(args, "--channel")?;
        let channel = Self::require_option_value(channel, "--channel")?;
        let (args, command) = Self::consume_option(args, "--command")?;
        let command = Self::require_option_value(command, "--command")?;
        let (args, cwd) = Self::consume_option(args, "--cwd")?;
        let cwd = Self::require_option_value(cwd, "--cwd")?;

        if onboard && no_onboard {
            return Err("start accepts only one of --onboard or --no-onboard".to_string());
        }
        if cwd.is_some() && command.is_none() {
            return Err("start accepts --cwd only with --command".to_string());
        }
        if version.is_some() && channel.is_some() {
            return Err("start accepts only one of --version or --channel".to_string());
        }

        let binding_sources = runtime_name.is_some() as u8
            + launcher_name.is_some() as u8
            + command.is_some() as u8
            + (version.is_some() || channel.is_some()) as u8;
        if binding_sources > 1 {
            return Err(
                "start accepts only one binding source: --runtime, --launcher, --version/--channel, or --command"
                    .to_string(),
            );
        }

        let name = match args.as_slice() {
            [] => "default".to_string(),
            [name] => validate_name(name, "Environment name")?,
            [name, extra @ ..] => {
                Self::assert_no_extra_args(extra)?;
                validate_name(name, "Environment name")?
            }
        };

        let request = StartRequest {
            name,
            root,
            gateway_port,
            protect,
            service_requested,
            onboarding_mode: if onboard {
                StartOnboardingMode::Always
            } else if no_onboard {
                StartOnboardingMode::Never
            } else {
                StartOnboardingMode::Auto
            },
            runtime_name,
            launcher_name,
            version,
            channel,
            command,
            cwd,
        };

        self.run_start_request(request, json_flag)
    }

    pub(super) fn run_start_request(
        &self,
        request: StartRequest,
        json_flag: bool,
    ) -> Result<i32, String> {
        let existing = self.environment_service().find(&request.name)?;
        if existing.is_some() && request.root.is_some() {
            return Err(format!(
                "start cannot change the root for existing env {}; use env create or env clone for a new root",
                request.name
            ));
        }
        if existing.is_some() && request.gateway_port.is_some() {
            return Err(format!(
                "start cannot change the port for existing env {}; use a new env name or keep the current port",
                request.name
            ));
        }

        let desired_binding = self.resolve_start_binding(
            &request.name,
            existing.as_ref(),
            request.runtime_name.clone(),
            request.launcher_name.clone(),
            request.version.clone(),
            request.channel.clone(),
            request.command.clone(),
            request.cwd.clone(),
        )?;

        let created = existing.is_none();
        let mut meta = match existing {
            None => self.environment_service().create(CreateEnvironmentOptions {
                name: request.name.clone(),
                root: request.root.clone(),
                gateway_port: request.gateway_port,
                default_runtime: match desired_binding.as_ref() {
                    Some(StartBinding::Runtime(runtime_name)) => Some(runtime_name.clone()),
                    _ => None,
                },
                default_launcher: match desired_binding.as_ref() {
                    Some(StartBinding::Launcher(launcher_name)) => Some(launcher_name.clone()),
                    _ => None,
                },
                protected: request.protect,
            })?,
            Some(existing) => self.apply_start_to_existing(
                existing,
                desired_binding.as_ref(),
                request.protect,
            )?,
        };

        meta = self.environment_service().apply_effective_gateway_port(meta)?;

        let onboarding_planned = match request.onboarding_mode {
            StartOnboardingMode::Always => true,
            StartOnboardingMode::Never => false,
            StartOnboardingMode::Auto => created,
        };

        if json_flag && onboarding_planned {
            return Err(
                "start cannot combine --json with interactive onboarding; rerun with --no-onboard"
                    .to_string(),
            );
        }

        let mut service_started = false;
        if request.service_requested {
            self.with_progress(format!("Installing service for {}", request.name), || {
                self.service_service().install(&request.name)
            })?;
            self.with_progress(format!("Starting service for {}", request.name), || {
                self.service_service().start(&request.name)
            })?;
            service_started = true;
        }

        let (effective_port, gateway_port_source) =
            self.environment_service().resolve_effective_gateway_port(&meta)?;
        let summary = StartSummary {
            env_name: request.name.clone(),
            created,
            root: meta.root.clone(),
            gateway_port: effective_port,
            gateway_port_source: gateway_port_source.to_string(),
            default_runtime: meta.default_runtime.clone(),
            default_launcher: meta.default_launcher.clone(),
            protected: meta.protected,
            onboarding_planned,
            service_requested: request.service_requested,
            service_started,
            activate_command: format!(
                "eval \"$({} env use {})\"",
                self.command_example(),
                request.name
            ),
            run_command: format!("{} @{} -- status", self.command_example(), request.name),
            onboard_command: format!("{} @{} -- onboard", self.command_example(), request.name),
            service_command: format!("{} service install {}", self.command_example(), request.name),
        };

        if json_flag {
            self.print_json(&summary)?;
            return Ok(0);
        }

        self.stdout_lines(self.start_summary_lines(&summary));

        if onboarding_planned {
            return self.handle_env_run(vec![
                request.name,
                "--".to_string(),
                "onboard".to_string(),
            ]);
        }

        Ok(0)
    }

    fn apply_start_to_existing(
        &self,
        mut meta: EnvMeta,
        desired_binding: Option<&StartBinding>,
        protect: bool,
    ) -> Result<EnvMeta, String> {
        if let Some(binding) = desired_binding {
            meta = match binding {
                StartBinding::Runtime(runtime_name)
                    if meta.default_runtime.as_deref() != Some(runtime_name.as_str())
                        || meta.default_launcher.is_some() =>
                {
                    self.environment_service()
                        .set_runtime(&meta.name, runtime_name)?
                }
                StartBinding::Launcher(launcher_name)
                    if meta.default_launcher.as_deref() != Some(launcher_name.as_str())
                        || meta.default_runtime.is_some() =>
                {
                    self.environment_service()
                        .set_launcher(&meta.name, launcher_name)?
                }
                _ => meta,
            };
        }

        if protect && !meta.protected {
            meta = self.environment_service().set_protected(&meta.name, true)?;
        }

        Ok(meta)
    }

    #[allow(clippy::too_many_arguments)]
    fn resolve_start_binding(
        &self,
        env_name: &str,
        existing: Option<&EnvMeta>,
        runtime_name: Option<String>,
        launcher_name: Option<String>,
        version: Option<String>,
        channel: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
    ) -> Result<Option<StartBinding>, String> {
        if let Some(runtime_name) = runtime_name {
            let runtime_name = self
                .environment_service()
                .resolve_runtime_binding_request(
                    Some(validate_name(&runtime_name, "Runtime name")?),
                    None,
                    None,
                    "start",
                )?
                .expect("validated runtime binding request must resolve");
            return Ok(Some(StartBinding::Runtime(runtime_name)));
        }

        if let Some(launcher_name) = launcher_name {
            let launcher_name = validate_name(&launcher_name, "Launcher name")?;
            self.launcher_service().show(&launcher_name)?;
            return Ok(Some(StartBinding::Launcher(launcher_name)));
        }

        if let Some(command) = command {
            let launcher_name = self.ensure_start_launcher(env_name, &command, cwd.as_deref())?;
            return Ok(Some(StartBinding::Launcher(launcher_name)));
        }

        if version.is_some() || channel.is_some() {
            let runtime_name =
                self.with_progress(format!("Preparing OpenClaw runtime for {env_name}"), || {
                    self.environment_service().resolve_runtime_binding_request(
                        None,
                        version,
                        channel,
                        "start",
                    )
                })?;
            return Ok(runtime_name.map(StartBinding::Runtime));
        }

        if let Some(existing) = existing {
            if existing.default_runtime.is_some() || existing.default_launcher.is_some() {
                return Ok(None);
            }
        }

        let runtime_name = self.with_progress(
            format!("Preparing latest stable OpenClaw for {env_name}"),
            || {
                self.environment_service().resolve_runtime_binding_request(
                    None,
                    None,
                    Some("stable".to_string()),
                    "start",
                )
            },
        )?;
        Ok(runtime_name.map(StartBinding::Runtime))
    }

    fn ensure_start_launcher(
        &self,
        env_name: &str,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<String, String> {
        let launcher_name = format!("{env_name}.local");
        let desired_cwd = cwd.map(str::to_string);
        match self.launcher_service().show(&launcher_name) {
            Ok(existing) => {
                if existing.command == command && existing.cwd == desired_cwd {
                    Ok(launcher_name)
                } else {
                    Err(format!(
                        "launcher \"{launcher_name}\" already exists with different settings; use \"{} launcher add\" or choose another env name",
                        self.command_example()
                    ))
                }
            }
            Err(error) if error.contains("does not exist") => {
                self.launcher_service().add(AddLauncherOptions {
                    name: launcher_name.clone(),
                    command: command.to_string(),
                    cwd: desired_cwd,
                    description: Some(format!("Local command for env {env_name}")),
                })?;
                Ok(launcher_name)
            }
            Err(error) => Err(error),
        }
    }

    fn start_summary_lines(&self, summary: &StartSummary) -> Vec<String> {
        let mut lines = vec![if summary.created {
            format!("Started env {}", summary.env_name)
        } else {
            format!("Using env {}", summary.env_name)
        }];
        lines.push(format!("  root: {}", summary.root));
        lines.push(format!(
            "  port: {} ({})",
            summary.gateway_port, summary.gateway_port_source
        ));
        if let Some(runtime) = summary.default_runtime.as_deref() {
            lines.push(format!("  runtime: {runtime}"));
        }
        if let Some(launcher) = summary.default_launcher.as_deref() {
            lines.push(format!("  launcher: {launcher}"));
        }
        if summary.protected {
            lines.push("  protected: true".to_string());
        }
        if summary.service_requested {
            lines.push(format!(
                "  service: {}",
                if summary.service_started {
                    "running"
                } else {
                    "requested"
                }
            ));
        } else {
            lines.push(format!("  service: {}", summary.service_command));
        }
        if summary.onboarding_planned {
            lines.push("  onboarding: running now".to_string());
        } else {
            lines.push(format!("  onboard: {}", summary.onboard_command));
        }
        lines.push(format!("  activate: {}", summary.activate_command));
        lines.push(format!("  run: {}", summary.run_command));
        lines
    }
}
