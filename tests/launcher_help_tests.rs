mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn help_mentions_launcher_and_runtime_commands() {
    let root = TestDir::new("launcher-help");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let help = run_ocm(&cwd, &env, &["help"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains("init [zsh|bash|sh|fish]"));
    assert!(output.contains("init bash"));
    assert!(output.contains("init fish"));
    assert!(output.contains("launcher add <name> --command"));
    assert!(output.contains("env clone <source> <target> [--root <path>] [--json]"));
    assert!(output.contains("env clone refactor-a refactor-b"));
    assert!(output.contains("env export <name> [--output <path>] [--json]"));
    assert!(output.contains("env export refactor-a --output ./backups/refactor-a.ocm-env.tar"));
    assert!(output.contains("runtime releases --manifest-url <url> [--json]"));
    assert!(output.contains(
        "runtime releases --manifest-url https://example.test/openclaw-releases.json --json"
    ));
    assert!(output.contains("launcher list [--json]"));
    assert!(output.contains("launcher show <name> [--json]"));
    assert!(output.contains("launcher remove <name>"));
    assert!(output.contains("runtime add <name> --path <binary> [--description <text>]"));
    assert!(output.contains(
        "runtime install <name> (--path <binary> | --url <url> | --manifest-url <url> (--version <version> | --channel <channel>)) [--description <text>] [--force]"
    ));
    assert!(output.contains(
        "runtime update (<name> | --all) [--version <version> | --channel <channel>] [--json]"
    ));
    assert!(output.contains("runtime update stable"));
    assert!(output.contains("runtime update --all"));
    assert!(output.contains(
        "runtime install stable --manifest-url https://example.test/openclaw-releases.json --version 0.2.0"
    ));
    assert!(output.contains(
        "runtime install stable --manifest-url https://example.test/openclaw-releases.json --channel stable"
    ));
    assert!(output.contains("runtime update stable --version 0.3.0"));
    assert!(output.contains("runtime list [--json]"));
    assert!(output.contains("runtime show <name> [--json]"));
    assert!(output.contains("runtime verify (<name> | --all) [--json]"));
    assert!(output.contains("runtime verify --all"));
    assert!(
        output.contains(
            "runtime install nightly --url https://example.test/openclaw-nightly --force"
        )
    );
    assert!(output.contains("runtime which <name> [--json]"));
    assert!(output.contains("runtime which nightly --json"));
    assert!(output.contains("runtime remove <name>"));
    assert!(output.contains(
        "env create <name> [--root <path>] [--port <port>] [--runtime <name>] [--launcher <name>] [--protect]"
    ));
    assert!(output.contains("env status <name> [--json]"));
    assert!(output.contains(
        "env resolve <name> [--runtime <name> | --launcher <name>] [--json] [-- <openclaw args...>]"
    ));
    assert!(
        output.contains(
            "env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>"
        )
    );
    assert!(output.contains("env set-runtime <name> <runtime|none>"));
    assert!(output.contains("env set-launcher <name> <launcher|none>"));
    assert!(!output.contains("version add <name> --command"));
}

#[test]
fn unknown_launcher_commands_use_launcher_specific_errors() {
    let root = TestDir::new("launcher-unknown-command");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["launcher", "rename"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown launcher command: rename"));
}
