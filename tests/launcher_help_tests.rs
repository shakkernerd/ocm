mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn help_mentions_launcher_runtime_and_service_commands() {
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
    assert!(output.contains("env import <archive> [--name <name>] [--root <path>] [--json]"));
    assert!(output.contains("env import ./backups/refactor-a.ocm-env.tar --name refactor-b"));
    assert!(output.contains("env snapshot create <name> [--label <label>] [--json]"));
    assert!(output.contains("env snapshot create refactor-a --label before-upgrade"));
    assert!(output.contains("env snapshot show <name> <snapshot> [--json]"));
    assert!(output.contains("env snapshot list <name> [--json]"));
    assert!(output.contains("env snapshot list --all [--json]"));
    assert!(output.contains("env snapshot restore <name> <snapshot> [--json]"));
    assert!(output.contains("env snapshot remove <name> <snapshot> [--json]"));
    assert!(output.contains(
        "env snapshot prune (<name> | --all) [--keep <count>] [--older-than <days>] [--yes] [--json]"
    ));
    assert!(output.contains("env snapshot show refactor-a 1742922000-123456789"));
    assert!(output.contains("env snapshot list refactor-a"));
    assert!(output.contains("env snapshot list --all --json"));
    assert!(output.contains("env snapshot restore refactor-a 1742922000-123456789"));
    assert!(output.contains("env snapshot remove refactor-a 1742922000-123456789"));
    assert!(output.contains("env snapshot prune refactor-a --keep 5 --yes"));
    assert!(output.contains("env snapshot prune --all --older-than 30 --json"));
    assert!(output.contains("env doctor <name> [--json]"));
    assert!(output.contains("env doctor refactor-a --json"));
    assert!(output.contains("env cleanup (<name> | --all) [--yes] [--json]"));
    assert!(output.contains("env cleanup refactor-a --json"));
    assert!(output.contains("env cleanup refactor-a --yes"));
    assert!(output.contains("env cleanup --all --yes"));
    assert!(output.contains("env repair-marker <name> [--json]"));
    assert!(output.contains("env repair-marker refactor-a --json"));
    assert!(output.contains(
        "runtime releases --manifest-url <url> [--version <version> | --channel <channel>] [--json]"
    ));
    assert!(output.contains(
        "runtime releases --manifest-url https://example.test/openclaw-releases.json --channel stable"
    ));
    assert!(output.contains(
        "runtime releases --manifest-url https://example.test/openclaw-releases.json --version 0.2.0 --json"
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
    assert!(output.contains("service adopt-global <env> [--json]"));
    assert!(output.contains("service install <env> [--json]"));
    assert!(output.contains("service list [--json]"));
    assert!(output.contains("service status <env> [--json]"));
    assert!(output.contains("service status --all [--json]"));
    assert!(output.contains("service logs <env> [--stderr] [--tail <count>] [--json]"));
    assert!(output.contains("service start <env> [--json]"));
    assert!(output.contains("service stop <env> [--json]"));
    assert!(output.contains("service restart <env> [--json]"));
    assert!(output.contains("service uninstall <env> [--json]"));
    assert!(output.contains("service adopt-global refactor-a --json"));
    assert!(output.contains("service install refactor-a --json"));
    assert!(output.contains("service list"));
    assert!(output.contains("service status refactor-a --json"));
    assert!(output.contains("service status --all"));
    assert!(output.contains("service logs refactor-a"));
    assert!(output.contains("service logs refactor-a --stderr --tail 50"));
    assert!(output.contains("service start refactor-a"));
    assert!(output.contains("service restart refactor-a --json"));
    assert!(output.contains("service stop refactor-a"));
    assert!(output.contains("service uninstall refactor-a"));
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
fn help_uses_ocm_self_for_usage_examples() {
    let root = TestDir::new("launcher-help-ocm-self");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    env.insert("OCM_SELF".to_string(), "./bin/ocm".to_string());

    let help = run_ocm(&cwd, &env, &["help"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains("./bin/ocm help"));
    assert!(output.contains("eval \"$(./bin/ocm env use refactor-a)\""));
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
