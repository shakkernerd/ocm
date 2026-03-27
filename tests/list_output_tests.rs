mod support;

use std::fs;

use serde_json::Value;

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
    assert!(output.contains("port=18789"));
    assert!(!output.contains("┌"));
}

#[test]
fn env_list_json_reports_effective_ports_for_fresh_envs() {
    let root = TestDir::new("env-list-effective-port-json");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let list = run_ocm(&cwd, &env, &["env", "list", "--json"]);
    assert!(list.status.success(), "{}", stderr(&list));
    let value: Value = serde_json::from_str(&stdout(&list)).unwrap();
    assert_eq!(value.as_array().unwrap().len(), 1);
    assert_eq!(value[0]["name"], "demo");
    assert_eq!(value[0]["gatewayPort"], 18789);
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
fn env_and_service_detail_commands_accept_raw_output_mode() {
    let root = TestDir::new("detail-raw");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_env(&root);

    let launcher = run_ocm(
        &cwd,
        &env,
        &["launcher", "add", "stable", "--command", "openclaw"],
    );
    assert!(launcher.status.success(), "{}", stderr(&launcher));

    let created = run_ocm(
        &cwd,
        &env,
        &["env", "create", "demo", "--launcher", "stable"],
    );
    assert!(created.status.success(), "{}", stderr(&created));

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--raw"]);
    assert!(env_show.status.success(), "{}", stderr(&env_show));
    assert!(stdout(&env_show).contains("name: demo"));
    assert!(!stdout(&env_show).contains("┌"));

    let env_status = run_ocm(&cwd, &env, &["env", "status", "demo", "--raw"]);
    assert!(env_status.status.success(), "{}", stderr(&env_status));
    assert!(stdout(&env_status).contains("gatewayPort: 18789"));
    assert!(!stdout(&env_status).contains("┌"));

    let env_resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo", "--raw"]);
    assert!(env_resolve.status.success(), "{}", stderr(&env_resolve));
    assert!(stdout(&env_resolve).contains("bindingKind: launcher"));
    assert!(!stdout(&env_resolve).contains("┌"));

    let env_doctor = run_ocm(&cwd, &env, &["env", "doctor", "demo", "--raw"]);
    assert!(env_doctor.status.success(), "{}", stderr(&env_doctor));
    assert!(stdout(&env_doctor).contains("healthy: true"));
    assert!(!stdout(&env_doctor).contains("┌"));

    let service_status = run_ocm(&cwd, &env, &["service", "status", "demo", "--raw"]);
    assert!(
        service_status.status.success(),
        "{}",
        stderr(&service_status)
    );
    assert!(stdout(&service_status).contains("managedState: absent"));
    assert!(!stdout(&service_status).contains("┌"));
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

    let env_show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json", "--raw"]);
    assert_eq!(env_show.status.code(), Some(1));
    assert!(stderr(&env_show).contains("env show accepts only one of --json or --raw"));

    let env_status = run_ocm(&cwd, &env, &["env", "status", "demo", "--json", "--raw"]);
    assert_eq!(env_status.status.code(), Some(1));
    assert!(stderr(&env_status).contains("env status accepts only one of --json or --raw"));

    let env_resolve = run_ocm(&cwd, &env, &["env", "resolve", "demo", "--json", "--raw"]);
    assert_eq!(env_resolve.status.code(), Some(1));
    assert!(stderr(&env_resolve).contains("env resolve accepts only one of --json or --raw"));

    let env_doctor = run_ocm(&cwd, &env, &["env", "doctor", "demo", "--json", "--raw"]);
    assert_eq!(env_doctor.status.code(), Some(1));
    assert!(stderr(&env_doctor).contains("env doctor accepts only one of --json or --raw"));

    let service_status = run_ocm(
        &cwd,
        &env,
        &["service", "status", "demo", "--json", "--raw"],
    );
    assert_eq!(service_status.status.code(), Some(1));
    assert!(stderr(&service_status).contains("service status accepts only one of --json or --raw"));
}
