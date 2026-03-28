mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn top_level_help_is_clean_and_points_to_topics() {
    let root = TestDir::new("help-root");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let help = run_ocm(&cwd, &env, &["help"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains(&format!("OpenClaw Manager v{}", env!("CARGO_PKG_VERSION"))));
    assert!(output.contains(
        "Manage isolated OpenClaw environments, releases, runtimes, launchers, and services."
    ));
    assert!(output.contains("ocm [--color <mode>] <command> [args]"));
    assert!(output.contains("--color <mode>"));
    assert!(output.contains("Color policy for pretty output: auto, always, or never"));
    assert!(output.contains("Environment lifecycle, binding, execution, snapshots, and repair"));
    assert!(output.contains("release list --channel stable"));
    assert!(output.contains("runtime install stable --channel stable"));
    assert!(output.contains("env create demo --runtime stable"));
    assert!(output.contains("ocm -- status"));
    assert!(output.contains("ocm @demo -- status"));
    assert!(output.contains("ocm help env"));
    assert!(output.contains("ocm help release"));
    assert!(output.contains("ocm help runtime install"));
    assert!(output.contains("ocm --color always env list"));
    assert!(!output.contains("env snapshot restore <name> <snapshot>"));
    assert!(!output.contains("service restore-global <env> [--dry-run] [--json]"));
}

#[test]
fn help_uses_ocm_self_for_root_and_leaf_examples() {
    let root = TestDir::new("help-ocm-self");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    env.insert("OCM_SELF".to_string(), "./bin/ocm".to_string());

    let help = run_ocm(&cwd, &env, &["help"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains("./bin/ocm [--color <mode>] <command> [args]"));
    assert!(output.contains("eval \"$(./bin/ocm env use demo)\""));

    let help = run_ocm(&cwd, &env, &["help", "env", "run"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains(
        "./bin/ocm env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>"
    ));
    assert!(output.contains("./bin/ocm env run demo -- onboard"));
}

#[test]
fn env_group_help_is_available_from_help_and_bare_group() {
    let root = TestDir::new("help-env-group");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let via_help = run_ocm(&cwd, &env, &["help", "env"]);
    let bare = run_ocm(&cwd, &env, &["env"]);
    assert!(via_help.status.success(), "{}", stderr(&via_help));
    assert!(bare.status.success(), "{}", stderr(&bare));

    let output = stdout(&via_help);
    assert_eq!(output, stdout(&bare));
    assert!(output.contains("Environment commands"));
    assert!(output.contains("snapshot create"));
    assert!(output.contains("Portability:"));
    assert!(output.contains("ocm help env create"));
    assert!(output.contains("ocm help env snapshot"));
}

#[test]
fn env_create_help_mentions_release_selectors() {
    let root = TestDir::new("help-env-create-release-selectors");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let help = run_ocm(&cwd, &env, &["help", "env", "create"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains(
        "ocm env create <name> [--root <path>] [--port <port>] [--runtime <name> | --version <version> | --channel <channel>] [--launcher <name>] [--protect] [--json]"
    ));
    assert!(output.contains("--version <version>"));
    assert!(output.contains("--channel <channel>"));
    assert!(output.contains("ocm env create demo --channel stable"));
    assert!(output.contains("ocm env create pinned --version 2026.3.24"));
}

#[test]
fn env_set_runtime_help_mentions_release_selectors() {
    let root = TestDir::new("help-env-set-runtime-release-selectors");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let help = run_ocm(&cwd, &env, &["help", "env", "set-runtime"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains("ocm env set-runtime <name> <runtime|none>"));
    assert!(output.contains(
        "ocm env set-runtime <name> (--version <version> | --channel <channel>)"
    ));
    assert!(output.contains("--version <version>"));
    assert!(output.contains("--channel <channel>"));
    assert!(output.contains("ocm env set-runtime demo --channel stable"));
    assert!(output.contains("ocm env set-runtime demo --version 2026.3.24"));
}

#[test]
fn release_group_help_is_available_from_help_and_bare_group() {
    let root = TestDir::new("help-release-group");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let via_help = run_ocm(&cwd, &env, &["help", "release"]);
    let bare = run_ocm(&cwd, &env, &["release"]);
    assert!(via_help.status.success(), "{}", stderr(&via_help));
    assert!(bare.status.success(), "{}", stderr(&bare));

    let output = stdout(&via_help);
    assert_eq!(output, stdout(&bare));
    assert!(output.contains("Release commands"));
    assert!(output.contains("List published OpenClaw releases"));
    assert!(output.contains("Install a published OpenClaw release as a runtime"));
    assert!(output.contains("ocm release show 2026.3.24"));
}

#[test]
fn env_run_help_is_available_through_help_keyword_and_flag() {
    let root = TestDir::new("help-env-run");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let via_help = run_ocm(&cwd, &env, &["help", "env", "run"]);
    let via_flag = run_ocm(&cwd, &env, &["env", "run", "--help"]);
    assert!(via_help.status.success(), "{}", stderr(&via_help));
    assert!(via_flag.status.success(), "{}", stderr(&via_flag));

    let output = stdout(&via_help);
    assert_eq!(output, stdout(&via_flag));
    assert!(output.contains("Run OpenClaw inside an environment"));
    assert!(output.contains(
        "ocm env run <name> [--runtime <name> | --launcher <name>] -- <openclaw args...>"
    ));
    assert!(output.contains("ocm -- status"));
    assert!(output.contains("ocm @demo -- status"));
    assert!(output.contains("`--` is required before OpenClaw arguments."));
    assert!(
        output.contains(
            "If an environment is active, you can also use the root-level `--` shortcut."
        )
    );
    assert!(
        output.contains("For one-shot explicit env runs, use the root-level `@<env>` shortcut.")
    );
}

#[test]
fn env_and_service_status_style_help_mentions_raw_mode() {
    let root = TestDir::new("help-status-style");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let env_show = run_ocm(&cwd, &env, &["help", "env", "show"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    let output = stdout(&env_show);
    assert!(output.contains("ocm env show <name> [--raw] [--json]"));
    assert!(output.contains("TTY output uses grouped cards by default."));

    let env_status = run_ocm(&cwd, &env, &["help", "env", "status"]);
    assert!(env_status.status.success(), "{}", stderr(&env_status));
    let output = stdout(&env_status);
    assert!(output.contains("ocm env status <name> [--raw] [--json]"));
    assert!(output.contains("--raw"));

    let env_resolve = run_ocm(&cwd, &env, &["help", "env", "resolve"]);
    assert!(env_resolve.status.success(), "{}", stderr(&env_resolve));
    let output = stdout(&env_resolve);
    assert!(output.contains(
        "ocm env resolve <name> [--runtime <name> | --launcher <name>] [--raw] [--json] [-- <openclaw args...>]"
    ));

    let env_doctor = run_ocm(&cwd, &env, &["help", "env", "doctor"]);
    assert!(env_doctor.status.success(), "{}", stderr(&env_doctor));
    let output = stdout(&env_doctor);
    assert!(output.contains("ocm env doctor <name> [--raw] [--json]"));

    let service_status = run_ocm(&cwd, &env, &["help", "service", "status"]);
    assert!(
        service_status.status.success(),
        "{}",
        stderr(&service_status)
    );
    let output = stdout(&service_status);
    assert!(output.contains("ocm service status <env> [--raw] [--json]"));
    assert!(
        output.contains("TTY output uses cards for one env and a table for `--all` by default.")
    );
}

#[test]
fn nested_snapshot_help_is_available() {
    let root = TestDir::new("help-env-snapshot");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let group = run_ocm(&cwd, &env, &["env", "snapshot"]);
    assert!(group.status.success(), "{}", stderr(&group));
    let output = stdout(&group);
    assert!(output.contains("Environment snapshot commands"));
    assert!(output.contains("snapshot prune"));

    let leaf = run_ocm(&cwd, &env, &["env", "snapshot", "create", "--help"]);
    assert!(leaf.status.success(), "{}", stderr(&leaf));
    let output = stdout(&leaf);
    assert!(output.contains("Create an environment snapshot"));
    assert!(output.contains("ocm env snapshot create <name> [--label <label>] [--json]"));

    let show = run_ocm(&cwd, &env, &["help", "env", "snapshot", "show"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let output = stdout(&show);
    assert!(output.contains("ocm env snapshot show <name> <snapshot> [--raw] [--json]"));
    assert!(output.contains("TTY output uses grouped cards by default."));

    let list = run_ocm(&cwd, &env, &["help", "env", "snapshot", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("ocm env snapshot list <name> [--raw] [--json]"));
    assert!(output.contains("TTY output renders a table by default."));

    let prune = run_ocm(&cwd, &env, &["help", "env", "snapshot", "prune"]);
    assert!(prune.status.success(), "{}", stderr(&prune));
    let output = stdout(&prune);
    assert!(output.contains(
        "ocm env snapshot prune (<name> | --all) [--keep <count>] [--older-than <days>] [--raw] [--yes] [--json]"
    ));
    assert!(
        output.contains("TTY output renders tables for preview and applied removals by default.")
    );
}

#[test]
fn runtime_and_service_leaf_help_are_command_specific() {
    let root = TestDir::new("help-runtime-service");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let runtime = run_ocm(&cwd, &env, &["help", "runtime", "install"]);
    assert!(runtime.status.success(), "{}", stderr(&runtime));
    let output = stdout(&runtime);
    assert!(output.contains("Install a managed runtime"));
    assert!(output.contains("--manifest-url <url>"));
    assert!(output.contains("Exactly one install source must be provided."));

    let service = run_ocm(&cwd, &env, &["service", "discover", "--help"]);
    assert!(service.status.success(), "{}", stderr(&service));
    let output = stdout(&service);
    assert!(output.contains("Discover OpenClaw services"));
    assert!(output.contains("ocm service discover [--raw] [--json]"));
}

#[test]
fn version_flag_uses_the_package_version() {
    let root = TestDir::new("version-flag");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let version = run_ocm(&cwd, &env, &["--version"]);
    assert!(version.status.success(), "{}", stderr(&version));
    assert_eq!(stdout(&version), concat!(env!("CARGO_PKG_VERSION"), "\n"));
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
