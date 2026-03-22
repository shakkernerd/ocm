#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    pub fn new(label: &str) -> Self {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir()
            .join("ocm-tests")
            .join(format!("{label}-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn child(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.path.join(relative)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

pub fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub fn base_env(home: &Path) -> BTreeMap<String, String> {
    fs::create_dir_all(home).unwrap();

    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), path_string(home));
    if let Ok(path) = std::env::var("PATH") {
        env.insert("PATH".to_string(), path);
    }
    env
}

pub fn ocm_env(root: &TestDir) -> BTreeMap<String, String> {
    let home = root.child("home");
    let ocm_home = root.child("ocm-home");
    fs::create_dir_all(&ocm_home).unwrap();

    let mut env = base_env(&home);
    env.insert("OCM_HOME".to_string(), path_string(&ocm_home));
    env
}

pub fn write_text(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

pub fn run_ocm(cwd: &Path, env: &BTreeMap<String, String>, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ocm"));
    command.current_dir(cwd);
    command.args(args);
    command.env_clear();
    command.envs(env);
    command.output().unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}
