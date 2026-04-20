mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::support::{
    TestDir, ocm_env, path_string, run_ocm, run_ocm_with_stdin, stderr, stdout,
    write_executable_script,
};

fn init_openclaw_repo(root: &TestDir) -> PathBuf {
    let repo = root.child("repo/openclaw");
    fs::create_dir_all(repo.join("scripts")).unwrap();
    fs::write(
        repo.join("package.json"),
        r#"{"name":"openclaw","version":"2026.4.19"}"#,
    )
    .unwrap();
    fs::write(repo.join("scripts/run-node.mjs"), "console.log('run');\n").unwrap();
    fs::write(
        repo.join("scripts/watch-node.mjs"),
        "console.log('watch');\n",
    )
    .unwrap();

    let init = Command::new("git").arg("init").arg(&repo).output().unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let email = Command::new("git")
        .args([
            "-C",
            &path_string(&repo),
            "config",
            "user.email",
            "tests@example.com",
        ])
        .output()
        .unwrap();
    assert!(
        email.status.success(),
        "{}",
        String::from_utf8_lossy(&email.stderr)
    );
    let name = Command::new("git")
        .args([
            "-C",
            &path_string(&repo),
            "config",
            "user.name",
            "OCM Tests",
        ])
        .output()
        .unwrap();
    assert!(
        name.status.success(),
        "{}",
        String::from_utf8_lossy(&name.stderr)
    );
    let add = Command::new("git")
        .args(["-C", &path_string(&repo), "add", "."])
        .output()
        .unwrap();
    assert!(
        add.status.success(),
        "{}",
        String::from_utf8_lossy(&add.stderr)
    );
    let commit = Command::new("git")
        .args(["-C", &path_string(&repo), "commit", "-m", "init"])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "{}",
        String::from_utf8_lossy(&commit.stderr)
    );

    repo
}

fn prepend_fake_bin(env: &mut std::collections::BTreeMap<String, String>, bin_dir: &Path) {
    let existing_path = env.get("PATH").cloned().unwrap_or_default();
    let combined_path = if existing_path.is_empty() {
        path_string(bin_dir)
    } else {
        format!("{}:{existing_path}", path_string(bin_dir))
    };
    env.insert("PATH".to_string(), combined_path);
}

fn install_fake_dev_runners(root: &TestDir, env: &mut std::collections::BTreeMap<String, String>) {
    let bin_dir = root.child("fake-dev-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let pnpm_log = root.child("pnpm.log");
    let node_log = root.child("node.log");
    let pnpm = format!(
        "#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$PWD\" \"$OPENCLAW_CONFIG_PATH\" \"$OPENCLAW_GATEWAY_PORT\" \"$*\" >> \"{}\"\n",
        path_string(&pnpm_log)
    );
    let node = format!(
        "#!/bin/sh\nprintf '%s|%s|%s|%s\\n' \"$PWD\" \"$OPENCLAW_CONFIG_PATH\" \"$OPENCLAW_GATEWAY_PORT\" \"$*\" >> \"{}\"\n",
        path_string(&node_log)
    );
    write_executable_script(&bin_dir.join("pnpm"), &pnpm);
    write_executable_script(&bin_dir.join("node"), &node);
    prepend_fake_bin(env, &bin_dir);
}

#[test]
fn dev_command_provisions_worktree_bootstraps_config_and_runs_gateway() {
    let root = TestDir::new("dev-command-run");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_dev_runners(&root, &mut env);

    let run = run_ocm(&cwd, &env, &["dev", "demo", "--repo", &path_string(&repo)]);
    assert!(run.status.success(), "{}", stderr(&run));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    let worktree_root = PathBuf::from(show_json["devWorktreeRoot"].as_str().unwrap());
    let config_path = PathBuf::from(show_json["configPath"].as_str().unwrap());
    let workspace_dir = PathBuf::from(show_json["workspaceDir"].as_str().unwrap());

    assert_eq!(show_json["devRepoRoot"], path_string(&repo));
    assert!(worktree_root.starts_with(repo.join(".worktrees")));
    assert!(worktree_root.join(".git").exists());

    let config: Value = serde_json::from_str(&fs::read_to_string(&config_path).unwrap()).unwrap();
    assert_eq!(config["gateway"]["mode"], "local");
    assert_eq!(config["gateway"]["bind"], "loopback");
    assert_eq!(
        config["agents"]["defaults"]["workspace"],
        path_string(&workspace_dir)
    );
    assert_eq!(config["agents"]["defaults"]["skipBootstrap"], true);
    assert_eq!(config["agents"]["list"][0]["id"], "dev");
    assert!(workspace_dir.exists());

    let pnpm_log = fs::read_to_string(root.child("pnpm.log")).unwrap();
    assert!(pnpm_log.contains("|install"));
    assert!(pnpm_log.contains("openclaw gateway run --port"));
    assert!(pnpm_log.contains(&path_string(&worktree_root)));
    assert!(pnpm_log.contains(&path_string(&config_path)));
}

#[test]
fn dev_command_can_onboard_then_watch() {
    let root = TestDir::new("dev-command-watch");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_dev_runners(&root, &mut env);

    let run = run_ocm(
        &cwd,
        &env,
        &[
            "dev",
            "demo",
            "--repo",
            &path_string(&repo),
            "--onboard",
            "--watch",
        ],
    );
    assert!(run.status.success(), "{}", stderr(&run));

    let pnpm_log = fs::read_to_string(root.child("pnpm.log")).unwrap();
    assert!(pnpm_log.contains("|install"));
    assert!(pnpm_log.contains("openclaw onboard --mode local --no-install-daemon"));

    let node_log = fs::read_to_string(root.child("node.log")).unwrap();
    assert!(node_log.contains("scripts/watch-node.mjs gateway run --port"));
}

#[test]
fn dev_status_reports_dev_envs() {
    let root = TestDir::new("dev-status");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_dev_runners(&root, &mut env);

    let run = run_ocm(&cwd, &env, &["dev", "demo", "--repo", &path_string(&repo)]);
    assert!(run.status.success(), "{}", stderr(&run));

    let status = run_ocm(&cwd, &env, &["dev", "status", "demo", "--json"]);
    assert!(status.status.success(), "{}", stderr(&status));
    let summary: Value = serde_json::from_str(&stdout(&status)).unwrap();
    assert_eq!(summary["envName"], "demo");
    assert_eq!(summary["repoRoot"], path_string(&repo));
    assert!(
        summary["worktreeRoot"]
            .as_str()
            .unwrap()
            .contains("/.worktrees/demo")
    );
    assert!(summary["gatewayPort"].as_u64().unwrap() > 0);
}

#[test]
fn dev_command_accepts_a_custom_env_root() {
    let root = TestDir::new("dev-command-custom-root");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_dev_runners(&root, &mut env);
    let custom_root = cwd.join("env-roots/demo");

    let run = run_ocm(
        &cwd,
        &env,
        &[
            "dev",
            "demo",
            "--repo",
            &path_string(&repo),
            "--root",
            "./env-roots/demo",
        ],
    );
    assert!(run.status.success(), "{}", stderr(&run));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    let resolved_root = fs::canonicalize(&custom_root).unwrap();
    assert_eq!(show_json["root"], path_string(&resolved_root));
    assert_eq!(show_json["openclawHome"], path_string(&resolved_root));
    assert_eq!(
        show_json["configPath"],
        path_string(&resolved_root.join(".openclaw/openclaw.json"))
    );
}

#[test]
fn dev_command_reuses_the_saved_repo_for_new_envs() {
    let root = TestDir::new("dev-command-saved-repo");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_dev_runners(&root, &mut env);

    let first = run_ocm(&cwd, &env, &["dev", "demo", "--repo", &path_string(&repo)]);
    assert!(first.status.success(), "{}", stderr(&first));

    let second = run_ocm(&cwd, &env, &["dev", "preview"]);
    assert!(second.status.success(), "{}", stderr(&second));

    let show = run_ocm(&cwd, &env, &["env", "show", "preview", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["devRepoRoot"], path_string(&repo));
    assert!(
        show_json["devWorktreeRoot"]
            .as_str()
            .unwrap()
            .contains("/.worktrees/preview")
    );
}

#[test]
fn dev_command_prompts_for_the_repo_when_it_is_not_known_yet() {
    let root = TestDir::new("dev-command-prompt-repo");
    let repo = init_openclaw_repo(&root);
    let cwd = root.child("workspace");
    fs::create_dir_all(&cwd).unwrap();
    let mut env = ocm_env(&root);
    install_fake_dev_runners(&root, &mut env);

    let run = run_ocm_with_stdin(
        &cwd,
        &env,
        &["dev", "demo"],
        &format!("{}\n", path_string(&repo)),
    );
    assert!(run.status.success(), "{}", stderr(&run));
    assert!(stdout(&run).contains("OpenClaw repo path"));

    let show = run_ocm(&cwd, &env, &["env", "show", "demo", "--json"]);
    assert!(show.status.success(), "{}", stderr(&show));
    let show_json: Value = serde_json::from_str(&stdout(&show)).unwrap();
    assert_eq!(show_json["devRepoRoot"], path_string(&repo));
}
