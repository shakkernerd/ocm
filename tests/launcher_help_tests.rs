mod support;

use std::fs;

use crate::support::{ocm_env, run_ocm, stderr, stdout, TestDir};

#[test]
fn help_mentions_launcher_alias_commands_alongside_version_commands() {
    let root = TestDir::new("launcher-help");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let help = run_ocm(&cwd, &env, &["help"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let output = stdout(&help);
    assert!(output.contains("version add <name> --command"));
    assert!(output.contains("launcher add <name> --command"));
    assert!(output.contains("launcher list [--json]"));
    assert!(output.contains("launcher show <name> [--json]"));
    assert!(output.contains("launcher remove <name>"));
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
