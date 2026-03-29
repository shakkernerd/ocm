#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

use sha2::{Digest, Sha256};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

pub struct TestDir {
    path: PathBuf,
}

pub struct TestHttpServer {
    addr: String,
    path: String,
    served: Arc<AtomicUsize>,
    request_limit: usize,
    handle: Option<JoinHandle<()>>,
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

impl TestHttpServer {
    pub fn serve_bytes(path: &str, content_type: &str, body: &[u8]) -> Self {
        Self::serve_bytes_times(path, content_type, body, 1)
    }

    pub fn serve_bytes_times(
        path: &str,
        content_type: &str,
        body: &[u8],
        request_limit: usize,
    ) -> Self {
        Self::serve_bytes_sequence(
            path,
            content_type,
            vec![body.to_vec(); request_limit.max(1)],
        )
    }

    pub fn serve_bytes_sequence(path: &str, content_type: &str, bodies: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let addr_string = format!("127.0.0.1:{}", addr.port());
        let path_string = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let response_path = path_string.clone();
        let response_type = content_type.to_string();
        let response_bodies = if bodies.is_empty() {
            vec![Vec::new()]
        } else {
            bodies
        };
        let request_limit = response_bodies.len();
        let served = Arc::new(AtomicUsize::new(0));
        let served_flag = Arc::clone(&served);
        let handle = thread::spawn(move || {
            for response_body in response_bodies {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut request = [0_u8; 4096];
                let _ = stream.read(&mut request);
                let request_text = String::from_utf8_lossy(&request);
                let status_line = if request_text.starts_with(&format!("GET {response_path} ")) {
                    "HTTP/1.1 200 OK"
                } else {
                    "HTTP/1.1 404 Not Found"
                };
                let body = if status_line.ends_with("200 OK") {
                    response_body
                } else {
                    b"not found".to_vec()
                };
                let response = format!(
                    "{status_line}\r\nContent-Length: {}\r\nContent-Type: {}\r\nConnection: close\r\n\r\n",
                    body.len(),
                    response_type
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
                served_flag.fetch_add(1, Ordering::SeqCst);
            }
        });

        Self {
            addr: addr_string,
            path: path_string,
            served,
            request_limit,
            handle: Some(handle),
        }
    }

    pub fn url(&self) -> String {
        format!("http://{}{}", self.addr, self.path)
    }
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        while self.served.load(Ordering::SeqCst) < self.request_limit {
            let Ok(mut stream) = TcpStream::connect(&self.addr) else {
                break;
            };
            let _ = write!(
                stream,
                "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                self.path, self.addr
            );
            let _ = stream.flush();
        }

        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
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

pub fn test_service_store_hash(env: &BTreeMap<String, String>, cwd: &Path) -> String {
    let store = env
        .get("OCM_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| cwd.join(".ocm"));
    let store = if store.is_absolute() {
        store
    } else {
        cwd.join(store)
    };
    let mut hasher = Sha256::new();
    hasher.update(path_string(&store).as_bytes());
    format!("{:x}", hasher.finalize())[..10].to_string()
}

pub fn managed_service_label(env: &BTreeMap<String, String>, cwd: &Path, name: &str) -> String {
    format!(
        "ai.openclaw.gateway.ocm.{}.{}",
        test_service_store_hash(env, cwd),
        name
    )
}

pub fn managed_service_definition_path(
    env: &BTreeMap<String, String>,
    cwd: &Path,
    name: &str,
) -> PathBuf {
    let label = managed_service_label(env, cwd, name);
    let home = PathBuf::from(
        env.get("HOME")
            .cloned()
            .unwrap_or_else(|| path_string(&cwd.join("home"))),
    );
    if env
        .get("OCM_INTERNAL_SERVICE_MANAGER")
        .is_some_and(|value| value.contains("systemd"))
    {
        home.join(".config")
            .join("systemd")
            .join("user")
            .join(format!("{label}.service"))
    } else {
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("{label}.plist"))
    }
}

pub fn write_text(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

pub fn write_executable_script(path: &Path, contents: &str) {
    write_text(path, contents);
    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
}

pub fn install_fake_launchctl(root: &TestDir, env: &mut BTreeMap<String, String>) {
    let bin_dir = root.child("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let log_path = root.child("launchctl.log");
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"{}\"\ncase \"$1\" in\n  print)\n    printf 'state = running\\npid = 23613\\n'\n    exit 0\n    ;;\n  *)\n    exit 0\n    ;;\nesac\n",
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

pub fn run_ocm(cwd: &Path, env: &BTreeMap<String, String>, args: &[&str]) -> Output {
    run_ocm_binary(Path::new(env!("CARGO_BIN_EXE_ocm")), cwd, env, args)
}

pub fn run_ocm_binary(
    binary: &Path,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    args: &[&str],
) -> Output {
    let mut command = Command::new(binary);
    command.current_dir(cwd);
    command.args(args);
    command.env_clear();
    command.envs(env);
    command.output().unwrap()
}

pub fn run_ocm_with_stdin(
    cwd: &Path,
    env: &BTreeMap<String, String>,
    args: &[&str],
    input: &str,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ocm"));
    command.current_dir(cwd);
    command.args(args);
    command.env_clear();
    command.envs(env);
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

pub fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}
