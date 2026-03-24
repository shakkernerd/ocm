mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_text};

#[test]
fn env_clone_copies_state_into_a_new_environment() {
    let root = TestDir::new("env-clone");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let create = run_ocm(
        &cwd,
        &env,
        &["env", "create", "source", "--port", "19789", "--protect"],
    );
    assert!(create.status.success(), "{}", stderr(&create));

    write_text(
        &root.child("ocm-home/envs/source/.openclaw/workspace/notes.txt"),
        "hello clone",
    );

    let clone = run_ocm(&cwd, &env, &["env", "clone", "source", "target"]);
    assert!(clone.status.success(), "{}", stderr(&clone));
    assert!(stdout(&clone).contains("Cloned env target from source"));

    let show = run_ocm(&cwd, &env, &["env", "show", "target", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_stdout = stdout(&show);
    assert!(show_stdout.contains("\"name\": \"target\""));
    assert!(show_stdout.contains("\"gatewayPort\": 19789"));
    assert!(show_stdout.contains("\"protected\": true"));

    assert_eq!(
        fs::read_to_string(root.child("ocm-home/envs/target/.openclaw/workspace/notes.txt"))
            .unwrap(),
        "hello clone"
    );
}
