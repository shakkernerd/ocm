mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout};

#[test]
fn init_zsh_prints_an_ocm_use_helper() {
    let root = TestDir::new("shell-init-zsh");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let init = run_ocm(&cwd, &env, &["init", "zsh"]);
    assert!(init.status.success(), "{}", stderr(&init));
    let output = stdout(&init);
    assert!(output.contains("ocm_use() {"));
    assert!(output.contains("command 'ocm' env use \"$@\""));
    assert!(output.contains("eval \"$script\""));
}

#[test]
fn init_bash_and_sh_print_the_same_posix_helper() {
    let root = TestDir::new("shell-init-posix");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    for shell in ["bash", "sh"] {
        let init = run_ocm(&cwd, &env, &["init", shell]);
        assert!(init.status.success(), "{}: {}", shell, stderr(&init));
        let output = stdout(&init);
        assert!(output.contains("ocm_use() {"), "{shell}");
        assert!(output.contains("command 'ocm' env use \"$@\""), "{shell}");
        assert!(output.contains("eval \"$script\""), "{shell}");
    }
}
