#![cfg(unix)]

mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use crate::support::{TestDir, stderr};

#[test]
fn installer_rejects_linux_aarch64_before_downloading() {
    let root = TestDir::new("install-linux-aarch64");
    let bin_dir = root.child("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let uname = bin_dir.join("uname");
    fs::write(
        &uname,
        "#!/bin/sh\ncase \"$1\" in\n  -s) printf 'Linux\\n' ;;\n  -m) printf 'aarch64\\n' ;;\n  *) exit 1 ;;\nesac\n",
    )
    .unwrap();
    fs::set_permissions(&uname, fs::Permissions::from_mode(0o755)).unwrap();

    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut command = Command::new("bash");
    command
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("install.sh"))
        .env("HOME", root.child("home"))
        .env(
            "PATH",
            format!("{}:{}", bin_dir.display(), path.to_string_lossy()),
        );
    let output = command.output().unwrap();

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("unsupported platform: aarch64-unknown-linux-gnu"),
        "{}",
        stderr(&output)
    );
}
