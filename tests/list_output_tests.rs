mod support;

use std::fs;

use crate::support::{TestDir, ocm_env, run_ocm, stderr, stdout, write_executable_script};

#[test]
fn env_list_accepts_raw_output_mode() {
    let root = TestDir::new("env-list-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--raw"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let output = stdout(&list);
    assert!(output.contains("demo"));
    assert!(!output.contains("┌"));
}

#[test]
fn launcher_runtime_and_service_lists_accept_raw_output_mode() {
    let root = TestDir::new("list-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let binary_path = cwd.join("bin/openclaw");
    write_executable_script(&binary_path, "#!/bin/sh\nexit 0\n");
    let runtime = run_ocm(
        &cwd,
        &env,
        &["runtime", "add", "stable", "--path", "./bin/openclaw"],
    );
    assert!(runtime.status.success(), "{}", stderr(&runtime));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let launcher_list = run_ocm(&cwd, &env, &["launcher", "list", "--raw"]);
    assert!(launcher_list.status.success(), "{}", stderr(&launcher_list));
    assert!(stdout(&launcher_list).contains("stable  openclaw"));
    assert!(!stdout(&launcher_list).contains("┌"));

    let runtime_list = run_ocm(&cwd, &env, &["runtime", "list", "--raw"]);
    assert!(runtime_list.status.success(), "{}", stderr(&runtime_list));
    assert!(stdout(&runtime_list).contains("stable"));
    assert!(!stdout(&runtime_list).contains("┌"));

    let service_list = run_ocm(&cwd, &env, &["service", "list", "--raw"]);
    assert!(service_list.status.success(), "{}", stderr(&service_list));
    assert!(stdout(&service_list).contains("demo"));
    assert!(!stdout(&service_list).contains("┌"));
}

#[test]
fn list_output_flags_reject_mixed_json_and_raw() {
    let root = TestDir::new("list-output-flags-validation");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(&cwd, &env, &["launcher", "list", "--json", "--raw"]);
    assert_eq!(launcher.status.code(), Some(1));
    assert!(stderr(&launcher).contains("launcher list accepts only one of --json or --raw"));

    let runtime = run_ocm(&cwd, &env, &["runtime", "list", "--json", "--raw"]);
    assert_eq!(runtime.status.code(), Some(1));
    assert!(stderr(&runtime).contains("runtime list accepts only one of --json or --raw"));

    let service = run_ocm(&cwd, &env, &["service", "list", "--json", "--raw"]);
    assert_eq!(service.status.code(), Some(1));
    assert!(stderr(&service).contains("service list accepts only one of --json or --raw"));
}
