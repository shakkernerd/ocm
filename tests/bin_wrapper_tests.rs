mod support;

use std::process::Command;

use support::{TestDir, path_string, stderr, stdout};

#[test]
fn bin_wrapper_runs_with_an_overridden_home() {
    let root = TestDir::new("bin-wrapper-home-override");
    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let wrapper = repo_root.join("bin/ocm");
    let home = root.child("isolated-home");
    let ocm_home = root.child("ocm-home");

    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&ocm_home).unwrap();

    let mut command = Command::new(&wrapper);
    command.current_dir(&repo_root);
    command.arg("help");
    command.env_clear();
    command.env("HOME", path_string(&home));
    command.env("OCM_HOME", path_string(&ocm_home));
    command.env("PATH", std::env::var("PATH").unwrap_or_default());
    command.env_remove("RUSTUP_HOME");

    let output = command.output().unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("OpenClaw Manager"));
}
