use std::collections::BTreeMap;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn run_direct(
    command: &str,
    args: &[String],
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<i32, String> {
    let status = Command::new(command)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .status()
        .map_err(|error| format!("failed to run \"{command}\": {error}"))?;
    Ok(status.code().unwrap_or(1))
}

pub fn run_shell(command: &str, env: &BTreeMap<String, String>, cwd: &Path) -> Result<i32, String> {
    if cfg!(windows) {
        run_direct("cmd", &["/C".to_string(), command.to_string()], env, cwd)
    } else {
        run_direct("sh", &["-lc".to_string(), command.to_string()], env, cwd)
    }
}
