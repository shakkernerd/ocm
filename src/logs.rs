use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use serde::Serialize;

use crate::env::EnvironmentService;
use crate::store::{derive_env_paths, display_path, supervisor_logs_dir};

const FOLLOW_POLL_INTERVAL_MS: u64 = 250;
const TAIL_READ_CHUNK_SIZE: usize = 8 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSummary {
    pub env_name: String,
    pub stream: String,
    pub source_kind: String,
    pub path: String,
    pub tail_lines: Option<usize>,
    pub content: String,
}

#[derive(Clone, Debug)]
pub struct LogTarget {
    pub env_name: String,
    pub stream: String,
    pub source_kind: String,
    pub path: PathBuf,
}

pub struct LogService<'a> {
    env: &'a BTreeMap<String, String>,
    cwd: &'a Path,
}

impl<'a> LogService<'a> {
    pub fn new(env: &'a BTreeMap<String, String>, cwd: &'a Path) -> Self {
        Self { env, cwd }
    }

    pub fn read(
        &self,
        name: &str,
        stream: &str,
        tail_lines: Option<usize>,
    ) -> Result<LogSummary, String> {
        let target = self.target(name, stream)?;
        if !target.path.exists() {
            return Err(format!(
                "{} log does not exist for env \"{}\": {}",
                target.stream,
                target.env_name,
                display_path(&target.path)
            ));
        }

        Ok(LogSummary {
            env_name: target.env_name,
            stream: target.stream,
            source_kind: target.source_kind,
            path: display_path(&target.path),
            tail_lines,
            content: read_log_text(&target.path, tail_lines)?,
        })
    }

    pub fn target(&self, name: &str, stream: &str) -> Result<LogTarget, String> {
        let stream = normalize_stream(stream)?.to_string();
        let meta = EnvironmentService::new(self.env, self.cwd).get(name)?;
        let env_paths = derive_env_paths(Path::new(&meta.root));
        let gateway_path = env_paths
            .state_dir
            .join("logs")
            .join(match stream.as_str() {
                "stdout" => "gateway.log",
                "stderr" => "gateway.err.log",
                _ => unreachable!("stream validated by normalize_stream"),
            });
        let supervisor_path =
            supervisor_logs_dir(self.env, self.cwd)?.join(format!("{}.{}.log", name, stream));

        if gateway_path.exists() || !supervisor_path.exists() {
            return Ok(LogTarget {
                env_name: name.to_string(),
                stream,
                source_kind: "gateway".to_string(),
                path: gateway_path,
            });
        }

        Ok(LogTarget {
            env_name: name.to_string(),
            stream,
            source_kind: "service-fallback".to_string(),
            path: supervisor_path,
        })
    }

    pub fn follow<W: std::io::Write>(
        &self,
        name: &str,
        stream: &str,
        tail_lines: Option<usize>,
        writer: &mut W,
    ) -> Result<LogTarget, String> {
        let target = self.target(name, stream)?;
        let mut offset = 0_u64;
        let mut printed_snapshot = false;

        loop {
            if !target.path.exists() {
                sleep(Duration::from_millis(FOLLOW_POLL_INTERVAL_MS));
                continue;
            }

            let metadata = fs::metadata(&target.path).map_err(|error| error.to_string())?;
            if !printed_snapshot {
                let content = read_log_text(&target.path, tail_lines)?;
                writer
                    .write_all(content.as_bytes())
                    .map_err(|error| error.to_string())?;
                writer.flush().map_err(|error| error.to_string())?;
                offset = metadata.len();
                printed_snapshot = true;
            } else if metadata.len() < offset {
                offset = 0;
            }

            if metadata.len() > offset {
                let mut file = File::open(&target.path).map_err(|error| error.to_string())?;
                file.seek(SeekFrom::Start(offset))
                    .map_err(|error| error.to_string())?;
                let mut chunk = String::new();
                file.read_to_string(&mut chunk)
                    .map_err(|error| error.to_string())?;
                writer
                    .write_all(chunk.as_bytes())
                    .map_err(|error| error.to_string())?;
                writer.flush().map_err(|error| error.to_string())?;
                offset = metadata.len();
            }

            sleep(Duration::from_millis(FOLLOW_POLL_INTERVAL_MS));
        }
    }
}

fn normalize_stream(stream: &str) -> Result<&str, String> {
    match stream {
        "stdout" | "stderr" => Ok(stream),
        _ => Err(format!("unsupported log stream: {stream}")),
    }
}

fn read_log_text(path: &Path, tail_lines: Option<usize>) -> Result<String, String> {
    match tail_lines {
        Some(limit) => read_log_tail(path, limit),
        None => fs::read_to_string(path).map_err(|error| error.to_string()),
    }
}

fn read_log_tail(path: &Path, tail_lines: usize) -> Result<String, String> {
    if tail_lines == 0 {
        return Ok(String::new());
    }

    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut position = file
        .seek(SeekFrom::End(0))
        .map_err(|error| error.to_string())?;
    let mut chunks = Vec::new();
    let mut newline_count = 0_usize;

    while position > 0 && newline_count <= tail_lines {
        let next_chunk = position.min(TAIL_READ_CHUNK_SIZE as u64) as usize;
        position -= next_chunk as u64;
        file.seek(SeekFrom::Start(position))
            .map_err(|error| error.to_string())?;

        let mut chunk = vec![0_u8; next_chunk];
        file.read_exact(&mut chunk)
            .map_err(|error| error.to_string())?;
        newline_count += chunk.iter().filter(|byte| **byte == b'\n').count();
        chunks.push(chunk);
    }

    let mut bytes = Vec::new();
    for mut chunk in chunks.into_iter().rev() {
        bytes.append(&mut chunk);
    }
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    Ok(tail_text(&raw, tail_lines))
}

fn tail_text(text: &str, tail_lines: usize) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(tail_lines);
    let mut output = lines[start..].join("\n");
    if text.ends_with('\n') && !output.is_empty() {
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{LogService, tail_text};
    use crate::store::ensure_dir;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;

    #[test]
    fn tail_text_keeps_the_last_requested_lines() {
        assert_eq!(tail_text("one\ntwo\nthree\n", 2), "two\nthree\n");
        assert_eq!(tail_text("one\ntwo\nthree", 1), "three");
    }

    #[test]
    fn target_prefers_gateway_logs_when_present() {
        let root = std::env::temp_dir().join(format!(
            "ocm-log-target-{}-{}",
            std::process::id(),
            crate::store::now_utc().unix_timestamp_nanos()
        ));
        let cwd = root.join("workspace");
        let ocm_home = root.join("ocm-home");
        let env_root = ocm_home.join("envs/demo");
        ensure_dir(&cwd).unwrap();
        ensure_dir(&env_root.join(".openclaw/logs")).unwrap();
        fs::write(
            ocm_home.join("envs.json"),
            format!(
                "{{\"kind\":\"ocm-env-registry\",\"envs\":[{{\"kind\":\"ocm-env\",\"name\":\"demo\",\"root\":\"{}\",\"gatewayPort\":18789,\"serviceEnabled\":false,\"serviceRunning\":false,\"protected\":false,\"createdAt\":\"1970-01-01T00:00:00Z\",\"updatedAt\":\"1970-01-01T00:00:00Z\"}}]}}",
                env_root.display()
            ),
        )
        .unwrap();
        fs::write(env_root.join(".openclaw/logs/gateway.log"), "hello\n").unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), root.join("home").display().to_string());
        env.insert("OCM_HOME".to_string(), ocm_home.display().to_string());
        let target = LogService::new(&env, Path::new(&cwd))
            .target("demo", "stdout")
            .unwrap();
        assert_eq!(target.source_kind, "gateway");
        assert!(target.path.ends_with(".openclaw/logs/gateway.log"));

        let _ = fs::remove_dir_all(&root);
    }
}
