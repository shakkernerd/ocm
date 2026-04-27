mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::{TestDir, path_string, write_executable_script};

struct ReleaseRepo {
    _root: TestDir,
    repo: PathBuf,
    remote: PathBuf,
    env_path: String,
    home: PathBuf,
    git: String,
}

impl ReleaseRepo {
    fn script_path(&self) -> PathBuf {
        self.repo.join("scripts/release.sh")
    }

    fn run_release(&self, version: &str) -> Output {
        let mut command = Command::new(self.script_path());
        command.current_dir(&self.repo);
        command.arg(version);
        command.arg("--remote");
        command.arg("fake");
        command.arg("--skip-checks");
        command.env_clear();
        command.env("HOME", &self.home);
        command.env("PATH", &self.env_path);
        command.output().unwrap()
    }

    fn git_output(&self, args: &[&str]) -> Output {
        let mut command = Command::new(&self.git);
        command.current_dir(&self.repo);
        command.args(args);
        command.env_clear();
        command.env("HOME", &self.home);
        command.env("PATH", &self.env_path);
        command.output().unwrap()
    }

    fn git_stdout(&self, args: &[&str]) -> String {
        let output = self.git_output(args);
        assert!(output.status.success(), "{}", stderr(&output));
        stdout(&output)
    }

    fn remote_ls_remote(&self, pattern: &str) -> String {
        let mut command = Command::new(&self.git);
        command.arg("ls-remote");
        command.arg(&self.remote);
        command.arg(pattern);
        command.env_clear();
        command.env("HOME", &self.home);
        command.env("PATH", &self.env_path);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        stdout(&output)
    }

    fn remote_tag_commit(&self, tag: &str) -> String {
        let mut command = Command::new(&self.git);
        command.arg("ls-remote");
        command.arg(&self.remote);
        command.arg(format!("refs/tags/{tag}^{{}}"));
        command.arg(format!("refs/tags/{tag}"));
        command.env_clear();
        command.env("HOME", &self.home);
        command.env("PATH", &self.env_path);
        let output = command.output().unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let listing = stdout(&output);
        let mut fallback = None;
        for line in listing.lines() {
            let mut parts = line.split_whitespace();
            let Some(sha) = parts.next() else {
                continue;
            };
            let Some(reference) = parts.next() else {
                continue;
            };
            if reference.ends_with("^{}") {
                return sha.to_string();
            }
            if fallback.is_none() {
                fallback = Some(sha.to_string());
            }
        }
        fallback.unwrap_or_default()
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

fn write_release_fixture(path: &Path) {
    fs::create_dir_all(path.join("scripts")).unwrap();
    fs::write(
        path.join("Cargo.toml"),
        r#"[package]
name = "ocm"
version = "0.2.7"
edition = "2024"
"#,
    )
    .unwrap();
    fs::write(
        path.join("Cargo.lock"),
        r#"version = 4

[[package]]
name = "ocm"
version = "0.2.7"
"#,
    )
    .unwrap();
    fs::write(path.join("README.md"), "release fixture\n").unwrap();
}

fn copy_script(repo: &Path, relative: &str) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    let destination = repo.join(relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let contents = fs::read_to_string(source).unwrap();
    write_executable_script(&destination, &contents);
}

fn replace_version(path: &Path, from: &str, to: &str) {
    let original = fs::read_to_string(path).unwrap();
    fs::write(path, original.replace(from, to)).unwrap();
}

fn init_release_repo(label: &str) -> ReleaseRepo {
    let root = TestDir::new(label);
    let repo = root.child("repo");
    let remote = root.child("remote.git");
    let home = root.child("home");
    let fake_bin = root.child("fake-bin");
    fs::create_dir_all(&repo).unwrap();
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();

    write_release_fixture(&repo);
    copy_script(&repo, "scripts/release.sh");
    copy_script(&repo, "scripts/update-version.sh");

    let git = std::process::Command::new("sh")
        .arg("-lc")
        .arg("command -v git")
        .output()
        .unwrap();
    assert!(git.status.success(), "{}", stderr(&git));
    let real_git = stdout(&git).trim().to_string();

    let git_wrapper = fake_bin.join("git");
    write_executable_script(
        &git_wrapper,
        &format!(
            "#!/usr/bin/env bash\nset -euo pipefail\nreal_git=\"{}\"\nif [[ \"${{1:-}}\" == \"-c\" && \"${{2:-}}\" == \"tag.gpgSign=true\" && \"${{3:-}}\" == \"tag\" ]]; then\n  shift 2\n  exec \"$real_git\" \"$@\"\nfi\nif [[ \"${{1:-}}\" == \"cat-file\" && \"${{2:-}}\" == \"-p\" ]]; then\n  \"$real_git\" \"$@\"\n  if [[ -n \"${{3:-}}\" ]] && [[ \"$(\"$real_git\" cat-file -t \"$3\" 2>/dev/null || true)\" == \"tag\" ]]; then\n    printf '%s\\n' '-----BEGIN SSH SIGNATURE-----' 'test-signature' '-----END SSH SIGNATURE-----'\n  fi\n  exit 0\nfi\nexec \"$real_git\" \"$@\"\n",
            real_git
        ),
    );

    let env_path = if let Ok(existing) = std::env::var("PATH") {
        format!("{}:{existing}", path_string(&fake_bin))
    } else {
        path_string(&fake_bin)
    };

    let init = Command::new(&real_git)
        .current_dir(&repo)
        .args(["init"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &env_path)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", stderr(&init));

    let checkout = Command::new(&real_git)
        .current_dir(&repo)
        .args(["checkout", "-B", "main"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &env_path)
        .output()
        .unwrap();
    assert!(checkout.status.success(), "{}", stderr(&checkout));

    for args in [
        ["config", "user.name", "Test User"].as_slice(),
        ["config", "user.email", "test@example.com"].as_slice(),
    ] {
        let output = Command::new(&real_git)
            .current_dir(&repo)
            .args(args)
            .env_clear()
            .env("HOME", &home)
            .env("PATH", &env_path)
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
    }

    let add = Command::new(&real_git)
        .current_dir(&repo)
        .args(["add", "Cargo.toml", "Cargo.lock", "README.md", "scripts"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &env_path)
        .output()
        .unwrap();
    assert!(add.status.success(), "{}", stderr(&add));

    let commit = Command::new(&real_git)
        .current_dir(&repo)
        .args(["commit", "-m", "chore: seed release fixture"])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &env_path)
        .output()
        .unwrap();
    assert!(commit.status.success(), "{}", stderr(&commit));

    let remote_init = Command::new(&real_git)
        .arg("init")
        .arg("--bare")
        .arg(&remote)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &env_path)
        .output()
        .unwrap();
    assert!(remote_init.status.success(), "{}", stderr(&remote_init));

    let remote_add = Command::new(&real_git)
        .current_dir(&repo)
        .args(["remote", "add", "fake", &path_string(&remote)])
        .env_clear()
        .env("HOME", &home)
        .env("PATH", &env_path)
        .output()
        .unwrap();
    assert!(remote_add.status.success(), "{}", stderr(&remote_add));

    ReleaseRepo {
        _root: root,
        repo,
        remote,
        env_path,
        home,
        git: real_git,
    }
}

#[test]
fn release_script_resumes_after_uncommitted_version_bump() {
    let repo = init_release_repo("release-script-resume-version-files");
    replace_version(&repo.repo.join("Cargo.toml"), "0.2.7", "0.2.8");
    replace_version(&repo.repo.join("Cargo.lock"), "0.2.7", "0.2.8");

    let output = repo.run_release("0.2.8");
    assert!(output.status.success(), "{}", stderr(&output));

    let stderr_output = stderr(&output);
    assert!(stderr_output.contains("resume state: version files already updated to 0.2.8"));
    assert!(stderr_output.contains("skip: version files are already set to 0.2.8"));

    assert_eq!(
        repo.git_stdout(&["log", "-1", "--pretty=%s"]).trim(),
        "chore: bump version to 0.2.8"
    );
    let head_sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    let tag_sha = repo.git_stdout(&["rev-list", "-n1", "v0.2.8"]);
    assert_eq!(head_sha.trim(), tag_sha.trim());
    assert!(
        repo.remote_ls_remote("refs/heads/main")
            .contains(head_sha.trim())
    );
    assert_eq!(repo.remote_tag_commit("v0.2.8"), head_sha.trim());
}

#[test]
fn release_script_resumes_after_local_tag_creation() {
    let repo = init_release_repo("release-script-resume-local-tag");
    replace_version(&repo.repo.join("Cargo.toml"), "0.2.7", "0.2.8");
    replace_version(&repo.repo.join("Cargo.lock"), "0.2.7", "0.2.8");

    let add = repo.git_output(&["add", "Cargo.toml", "Cargo.lock"]);
    assert!(add.status.success(), "{}", stderr(&add));
    let commit = repo.git_output(&["commit", "-m", "chore: bump version to 0.2.8"]);
    assert!(commit.status.success(), "{}", stderr(&commit));
    let tag = repo.git_output(&["tag", "v0.2.8"]);
    assert!(tag.status.success(), "{}", stderr(&tag));

    let output = repo.run_release("0.2.8");
    assert!(output.status.success(), "{}", stderr(&output));

    let stderr_output = stderr(&output);
    assert!(stderr_output.contains("resume state: release commit already exists"));
    assert!(stderr_output.contains("skip: release commit already exists"));
    assert!(stderr_output.contains("skip: local tag v0.2.8 already exists"));

    let head_sha = repo.git_stdout(&["rev-parse", "HEAD"]);
    assert!(
        repo.remote_ls_remote("refs/heads/main")
            .contains(head_sha.trim())
    );
    assert_eq!(repo.remote_tag_commit("v0.2.8"), head_sha.trim());
}

#[test]
fn release_script_still_refuses_unrelated_dirty_changes() {
    let repo = init_release_repo("release-script-refuses-unrelated-dirty");
    replace_version(&repo.repo.join("Cargo.toml"), "0.2.7", "0.2.8");
    replace_version(&repo.repo.join("Cargo.lock"), "0.2.7", "0.2.8");
    fs::write(repo.repo.join("README.md"), "dirty change\n").unwrap();

    let output = repo.run_release("0.2.8");
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("tracked changes are present"));
}
