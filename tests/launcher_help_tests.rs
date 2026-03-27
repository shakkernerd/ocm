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
    assert!(output.contains("OpenClaw Manager"));
    assert!(
        output
            .contains("Manage isolated OpenClaw environments, runtimes, launchers, and services.")
    );
    assert!(output.contains("ocm <command> [args]"));
    assert!(output.contains("Environment lifecycle, binding, execution, snapshots, and repair"));
    assert!(output.contains("launcher add stable --command openclaw"));
    assert!(output.contains("ocm help env"));
    assert!(output.contains("ocm help runtime install"));
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
    assert!(output.contains("./bin/ocm <command> [args]"));
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
    assert!(output.contains("`--` is required before OpenClaw arguments."));
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
fn unknown_launcher_commands_use_launcher_specific_errors() {
    let root = TestDir::new("launcher-unknown-command");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let output = run_ocm(&cwd, &env, &["launcher", "rename"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("unknown launcher command: rename"));
}
