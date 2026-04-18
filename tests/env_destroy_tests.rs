mod support;

use std::collections::BTreeMap;
use std::fs;

use crate::support::{
    TestDir, managed_service_definition_path, ocm_env, path_string, run_ocm, stderr, stdout,
    write_executable_script,
};

fn install_fake_launchctl(root: &TestDir, env: &mut BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log_path = root.child("launchctl.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\ncase \"$1\" in\n  print)\n    exit 1\n    ;;\n  *)\n    exit 0\n    ;;\nesac\n",
        path_string(&log_path)
    );
    write_executable_script(&bin_dir.join("launchctl"), &script);

    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(&bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(&bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
}

fn ocm_launchd_env(root: &TestDir) -> BTreeMap<String, String> {
    let mut env = ocm_env(root);
    env.insert(
        "OCM_INTERNAL_SERVICE_MANAGER".to_string(),
        "launchd".to_string(),
    );
    env
}

#[test]
fn env_destroy_preview_reports_service_snapshot_and_env_steps() {
    let root = TestDir::new("env-destroy-preview");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

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

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "demo",
            "--label",
            "before-destroy",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));

    let preview = run_ocm(&cwd, &env, &["env", "destroy", "demo"]);
    assert!(preview.status.success(), "{}", stderr(&preview));
    let output = stdout(&preview);
    assert!(output.contains("Destroy preview for env demo"));
    assert!(output.contains("snapshots: 1"));
    assert!(output.contains("service: ocm"));
    assert!(output.contains("snapshots: remove 1 env snapshot(s)"));
    assert!(output.contains("service: disable env service in the OCM background service"));
    assert!(output.contains("env: remove env root and metadata"));
    assert!(output.contains("re-run with --yes to destroy it"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
}

#[test]
fn env_destroy_yes_uninstalls_service_removes_snapshots_and_deletes_env() {
    let root = TestDir::new("env-destroy-apply");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_launchd_env(&root);
    install_fake_launchctl(&root, &mut env);

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

    let snapshot = run_ocm(
        &cwd,
        &env,
        &[
            "env",
            "snapshot",
            "create",
            "demo",
            "--label",
            "before-destroy",
        ],
    );
    assert!(snapshot.status.success(), "{}", stderr(&snapshot));

    let install = run_ocm(&cwd, &env, &["service", "install", "demo"]);
    assert!(install.status.success(), "{}", stderr(&install));
    let supervisor_path = managed_service_definition_path(&env, &cwd, "supervisor");

    let destroy = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes"]);
    assert!(destroy.status.success(), "{}", stderr(&destroy));
    let output = stdout(&destroy);
    assert!(output.contains("Destroyed env demo"));
    assert!(output.contains("snapshots removed: 1"));
    assert!(output.contains("service removed: ocm"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(!show.status.success());
    assert!(stderr(&show).contains("environment \"demo\" does not exist"));

    assert!(supervisor_path.exists());
    assert!(!root.child("ocm-home/snapshots/demo").exists());
}

#[test]
fn env_destroy_requires_force_for_protected_envs() {
    let root = TestDir::new("env-destroy-protected");
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let env = ocm_launchd_env(&root);

    let created = run_ocm(&cwd, &env, &["env", "create", "demo", "--protect"]);
    assert!(created.status.success(), "{}", stderr(&created));

    let blocked = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes"]);
    assert!(!blocked.status.success());
    let output = stdout(&blocked);
    assert!(output.contains("Destroy preview for env demo"));
    assert!(output.contains("env is protected; re-run with --force to destroy it"));

    let forced = run_ocm(&cwd, &env, &["env", "destroy", "demo", "--yes", "--force"]);
    assert!(forced.status.success(), "{}", stderr(&forced));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(!show.status.success());
}
