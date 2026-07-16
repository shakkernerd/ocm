use std::collections::BTreeMap;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use super::{
    OPENCLAW_GATEWAY_SERVICE_KIND, OPENCLAW_NATIVE_SERVICE_IDENTITY_KEYS, OPENCLAW_SERVICE_MARKER,
    SupervisorChildSpec,
};
use crate::infra::shell::quote_posix;

const PROTOCOL: &str = "openclaw.gateway.restart-handoff";
const PROTOCOL_VERSION: u8 = 1;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RestartHandoffSupport {
    ProtocolV1,
    Unsupported(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum RestartHandoffConsumeResult {
    Accepted,
    NotAccepted(String),
    Unsupported(String),
    Failed(String),
    NotChecked,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CapabilitiesResponse {
    ok: bool,
    protocol: String,
    protocol_version: u8,
    operations: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsumeResponse {
    ok: bool,
    protocol: String,
    protocol_version: u8,
    status: String,
    reason: Option<String>,
    handoff: Option<ConsumedHandoff>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsumedHandoff {
    pid: u64,
    supervisor_mode: String,
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
}

pub(super) fn probe_restart_handoff_support(spec: &SupervisorChildSpec) -> RestartHandoffSupport {
    let probe_spec = prepare_supervisor_child_spec(
        spec,
        &RestartHandoffSupport::Unsupported("capability probe pending".to_string()),
    );
    let output = match run_openclaw_machine_command(
        &probe_spec,
        &["gateway", "restart-handoff", "capabilities", "--json"],
    ) {
        Ok(output) => output,
        Err(error) => return RestartHandoffSupport::Unsupported(error),
    };
    if !output.status.success() {
        return RestartHandoffSupport::Unsupported(command_failure_detail(output.status));
    }
    let response = match serde_json::from_slice::<CapabilitiesResponse>(&output.stdout) {
        Ok(response) => response,
        Err(error) => {
            return RestartHandoffSupport::Unsupported(format!(
                "invalid capability response: {error}"
            ));
        }
    };
    if response.ok
        && response.protocol == PROTOCOL
        && response.protocol_version == PROTOCOL_VERSION
        && response
            .operations
            .iter()
            .any(|operation| operation == "consume")
    {
        RestartHandoffSupport::ProtocolV1
    } else {
        RestartHandoffSupport::Unsupported(
            "runtime does not advertise restart-handoff protocol version 1 consume".to_string(),
        )
    }
}

pub(super) fn prepare_supervisor_child_spec(
    spec: &SupervisorChildSpec,
    support: &RestartHandoffSupport,
) -> SupervisorChildSpec {
    let mut prepared = spec.clone();
    scrub_native_service_identity(&mut prepared.process_env);
    prepared.process_env.insert(
        "OPENCLAW_SERVICE_MARKER".to_string(),
        OPENCLAW_SERVICE_MARKER.to_string(),
    );
    match support {
        RestartHandoffSupport::ProtocolV1 => {
            prepared.process_env.insert(
                "OPENCLAW_SERVICE_KIND".to_string(),
                OPENCLAW_GATEWAY_SERVICE_KIND.to_string(),
            );
            prepared.process_env.insert(
                "OPENCLAW_SUPERVISOR_MODE".to_string(),
                "external".to_string(),
            );
            prepared.process_env.remove("OPENCLAW_NO_RESPAWN");
        }
        RestartHandoffSupport::Unsupported(_) => {
            prepared.process_env.remove("OPENCLAW_SERVICE_KIND");
            prepared.process_env.remove("OPENCLAW_SUPERVISOR_MODE");
            prepared
                .process_env
                .insert("OPENCLAW_NO_RESPAWN".to_string(), "1".to_string());
        }
    }
    prepared
}

pub(super) fn consume_restart_handoff(
    spec: &SupervisorChildSpec,
    support: &RestartHandoffSupport,
    expected_pid: u32,
) -> RestartHandoffConsumeResult {
    if let RestartHandoffSupport::Unsupported(reason) = support {
        return RestartHandoffConsumeResult::Unsupported(reason.clone());
    }
    let prepared = prepare_supervisor_child_spec(spec, support);
    let expected_pid_arg = expected_pid.to_string();
    let output = match run_openclaw_machine_command(
        &prepared,
        &[
            "gateway",
            "restart-handoff",
            "consume",
            "--expected-pid",
            &expected_pid_arg,
            "--json",
        ],
    ) {
        Ok(output) => output,
        Err(error) => return RestartHandoffConsumeResult::Failed(error),
    };
    if !output.status.success() {
        return RestartHandoffConsumeResult::Failed(command_failure_detail(output.status));
    }
    let response = match serde_json::from_slice::<ConsumeResponse>(&output.stdout) {
        Ok(response) => response,
        Err(error) => {
            return RestartHandoffConsumeResult::Failed(format!(
                "invalid consume response: {error}"
            ));
        }
    };
    if !response.ok
        || response.protocol != PROTOCOL
        || response.protocol_version != PROTOCOL_VERSION
    {
        return RestartHandoffConsumeResult::Failed(
            "consume response does not match restart-handoff protocol version 1".to_string(),
        );
    }
    match response.status.as_str() {
        "accepted" => match response.handoff {
            Some(handoff)
                if handoff.pid == u64::from(expected_pid)
                    && handoff.supervisor_mode == "external" =>
            {
                RestartHandoffConsumeResult::Accepted
            }
            _ => RestartHandoffConsumeResult::Failed(
                "accepted consume response does not match the exited external gateway".to_string(),
            ),
        },
        "none" | "rejected" => RestartHandoffConsumeResult::NotAccepted(
            response
                .reason
                .unwrap_or_else(|| response.status.to_string()),
        ),
        status => RestartHandoffConsumeResult::Failed(format!(
            "unknown consume response status \"{status}\""
        )),
    }
}

fn scrub_native_service_identity(env: &mut BTreeMap<String, String>) {
    for key in OPENCLAW_NATIVE_SERVICE_IDENTITY_KEYS {
        env.remove(key);
    }
}

fn run_openclaw_machine_command(
    spec: &SupervisorChildSpec,
    openclaw_args: &[&str],
) -> Result<CommandOutput, String> {
    let program_arguments = openclaw_program_arguments(spec, openclaw_args)?;
    let program = program_arguments
        .first()
        .ok_or_else(|| "restart-handoff command is missing a program".to_string())?;
    let mut command = Command::new(program);
    command
        .args(program_arguments.iter().skip(1))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_clear()
        .envs(&spec.process_env)
        .current_dir(Path::new(&spec.run_dir));
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        format!(
            "failed running restart-handoff command for env \"{}\": {error}",
            spec.env_name
        )
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "restart-handoff command stdout was not captured".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "restart-handoff command stderr was not captured".to_string())?;
    let stdout_reader = thread::spawn(move || read_bounded(stdout));
    let stderr_reader = thread::spawn(move || read_bounded(stderr));

    let status = wait_for_child(&mut child, COMMAND_TIMEOUT);
    let stdout = stdout_reader
        .join()
        .map_err(|_| "restart-handoff stdout reader panicked".to_string())?
        .map_err(|error| format!("failed reading restart-handoff stdout: {error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "restart-handoff stderr reader panicked".to_string())?
        .map_err(|error| format!("failed reading restart-handoff stderr: {error}"))?;
    drop(stderr);
    Ok(CommandOutput {
        status: status?,
        stdout,
    })
}

fn openclaw_program_arguments(
    spec: &SupervisorChildSpec,
    openclaw_args: &[&str],
) -> Result<Vec<String>, String> {
    let gateway_args = [
        "gateway".to_string(),
        "run".to_string(),
        "--port".to_string(),
        spec.child_port.to_string(),
    ];
    if let Some(binary_path) = spec.binary_path.as_ref() {
        let prefix_len = spec
            .args
            .len()
            .checked_sub(gateway_args.len())
            .ok_or_else(|| {
                format!(
                    "service child env \"{}\" has incomplete gateway arguments",
                    spec.env_name
                )
            })?;
        if spec.args[prefix_len..] != gateway_args {
            return Err(format!(
                "service child env \"{}\" gateway arguments do not match its port",
                spec.env_name
            ));
        }
        let mut program_arguments = vec![binary_path.clone()];
        program_arguments.extend(spec.args[..prefix_len].iter().cloned());
        program_arguments.extend(openclaw_args.iter().map(|arg| (*arg).to_string()));
        return Ok(program_arguments);
    }

    let command = spec.command.as_deref().ok_or_else(|| {
        format!(
            "service child env \"{}\" is missing a launcher command",
            spec.env_name
        )
    })?;
    let suffix = format!(
        " {}",
        gateway_args
            .iter()
            .map(|arg| quote_posix(arg))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let base_command = command.strip_suffix(&suffix).ok_or_else(|| {
        format!(
            "service child env \"{}\" launcher command does not end with its gateway arguments",
            spec.env_name
        )
    })?;
    let machine_args = openclaw_args
        .iter()
        .map(|arg| quote_posix(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let machine_command = format!("{base_command} {machine_args}");
    if cfg!(windows) {
        Ok(vec!["cmd".to_string(), "/C".to_string(), machine_command])
    } else {
        Ok(vec![
            "/bin/sh".to_string(),
            "-lc".to_string(),
            machine_command,
        ])
    }
}

fn read_bounded<R: Read>(mut reader: R) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..read.min(remaining)]);
    }
    Ok(kept)
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> Result<ExitStatus, String> {
    let started_at = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started_at.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                terminate_child(child);
                return Err(format!(
                    "restart-handoff command timed out after {} seconds",
                    timeout.as_secs()
                ));
            }
            Err(error) => {
                terminate_child(child);
                return Err(format!(
                    "failed waiting for restart-handoff command: {error}"
                ));
            }
        }
    }
}

fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let process_group = format!("-{}", child.id());
        let _ = Command::new("kill")
            .args(["-TERM", "--", &process_group])
            .status();
        for _ in 0..20 {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(_) => break,
            }
        }
        let _ = Command::new("kill")
            .args(["-KILL", "--", &process_group])
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn command_failure_detail(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("command exited {code}"),
        None => "command terminated without an exit code".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_spec() -> SupervisorChildSpec {
        SupervisorChildSpec {
            env_name: "demo".to_string(),
            binding_kind: "runtime".to_string(),
            binding_name: "local".to_string(),
            command: None,
            binary_path: Some("/runtime/openclaw".to_string()),
            runtime_source_kind: Some("installed".to_string()),
            runtime_release_version: None,
            runtime_release_channel: None,
            args: vec![
                "--profile".to_string(),
                "demo".to_string(),
                "gateway".to_string(),
                "run".to_string(),
                "--port".to_string(),
                "19001".to_string(),
            ],
            run_dir: "/tmp/demo".to_string(),
            child_port: 19001,
            stdout_path: "/tmp/demo.stdout.log".to_string(),
            stderr_path: "/tmp/demo.stderr.log".to_string(),
            process_env: BTreeMap::from([
                (
                    "OPENCLAW_LAUNCHD_LABEL".to_string(),
                    "ai.openclaw.ocm".to_string(),
                ),
                (
                    "OPENCLAW_SYSTEMD_UNIT".to_string(),
                    "ai.openclaw.ocm.service".to_string(),
                ),
                (
                    "OPENCLAW_WINDOWS_TASK_NAME".to_string(),
                    "OCM Supervisor".to_string(),
                ),
                (
                    "OPENCLAW_SERVICE_MARKER".to_string(),
                    "openclaw".to_string(),
                ),
                ("OPENCLAW_SERVICE_KIND".to_string(), "gateway".to_string()),
            ]),
        }
    }

    #[test]
    fn direct_binding_replaces_only_gateway_arguments() {
        let args = openclaw_program_arguments(
            &direct_spec(),
            &[
                "gateway",
                "restart-handoff",
                "consume",
                "--expected-pid",
                "42",
                "--json",
            ],
        )
        .unwrap();
        assert_eq!(
            args,
            vec![
                "/runtime/openclaw",
                "--profile",
                "demo",
                "gateway",
                "restart-handoff",
                "consume",
                "--expected-pid",
                "42",
                "--json",
            ]
        );
    }

    #[test]
    fn supported_child_uses_external_supervision_without_native_identity() {
        let prepared =
            prepare_supervisor_child_spec(&direct_spec(), &RestartHandoffSupport::ProtocolV1);
        assert_eq!(
            prepared
                .process_env
                .get("OPENCLAW_SUPERVISOR_MODE")
                .map(String::as_str),
            Some("external")
        );
        assert_eq!(
            prepared
                .process_env
                .get("OPENCLAW_SERVICE_KIND")
                .map(String::as_str),
            Some("gateway")
        );
        assert!(!prepared.process_env.contains_key("OPENCLAW_NO_RESPAWN"));
        for key in OPENCLAW_NATIVE_SERVICE_IDENTITY_KEYS {
            assert!(!prepared.process_env.contains_key(key));
        }
    }

    #[test]
    fn unsupported_child_blocks_detached_respawn_and_native_identity_inference() {
        let prepared = prepare_supervisor_child_spec(
            &direct_spec(),
            &RestartHandoffSupport::Unsupported("missing".to_string()),
        );
        assert_eq!(
            prepared
                .process_env
                .get("OPENCLAW_SERVICE_MARKER")
                .map(String::as_str),
            Some("openclaw")
        );
        assert!(!prepared.process_env.contains_key("OPENCLAW_SERVICE_KIND"));
        assert!(
            !prepared
                .process_env
                .contains_key("OPENCLAW_SUPERVISOR_MODE")
        );
        assert_eq!(
            prepared
                .process_env
                .get("OPENCLAW_NO_RESPAWN")
                .map(String::as_str),
            Some("1")
        );
        for key in OPENCLAW_NATIVE_SERVICE_IDENTITY_KEYS {
            assert!(!prepared.process_env.contains_key(key));
        }
    }
}
