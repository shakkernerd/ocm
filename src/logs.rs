use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use serde::Serialize;
use time::OffsetDateTime;

use crate::env::EnvironmentService;
use crate::store::{derive_env_paths, display_path, supervisor_logs_dir};

const FOLLOW_POLL_INTERVAL_MS: u64 = 250;
const TAIL_READ_CHUNK_SIZE: usize = 8 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogComponentSummary {
    pub stream: String,
    pub source_kind: String,
    pub path: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSummary {
    pub env_name: String,
    pub stream: String,
    pub source_kind: String,
    pub path: String,
    pub tail_lines: Option<usize>,
    pub content: String,
    pub components: Vec<LogComponentSummary>,
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
        let targets = self.targets(name, stream)?;
        if targets.len() == 1 {
            let target = &targets[0];
            if !target.path.exists() {
                return Err(format!(
                    "{} log does not exist for env \"{}\": {}",
                    target.stream,
                    target.env_name,
                    display_path(&target.path)
                ));
            }

            return Ok(LogSummary {
                env_name: target.env_name.clone(),
                stream: target.stream.clone(),
                source_kind: target.source_kind.clone(),
                path: display_path(&target.path),
                tail_lines,
                content: read_log_text(&target.path, tail_lines)?,
                components: vec![LogComponentSummary {
                    stream: target.stream.clone(),
                    source_kind: target.source_kind.clone(),
                    path: display_path(&target.path),
                }],
            });
        }

        let mut contents = Vec::new();
        let mut components = Vec::new();
        for target in &targets {
            components.push(LogComponentSummary {
                stream: target.stream.clone(),
                source_kind: target.source_kind.clone(),
                path: display_path(&target.path),
            });
            if target.path.exists() {
                contents.push((
                    target.stream.clone(),
                    read_log_text(&target.path, tail_lines)?,
                ));
            }
        }
        if contents.is_empty() {
            return Err(format!(
                "no logs exist for env \"{}\" across stdout or stderr",
                name
            ));
        }

        Ok(LogSummary {
            env_name: name.to_string(),
            stream: "stdout + stderr".to_string(),
            source_kind: summarize_sources(&targets),
            path: "multiple".to_string(),
            tail_lines,
            content: merge_log_texts(contents, tail_lines),
            components,
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

        let gateway_modified = modified_at(&gateway_path);
        let supervisor_modified = modified_at(&supervisor_path);

        let (source_kind, path) = match (gateway_modified, supervisor_modified) {
            (Some(gateway_time), Some(supervisor_time)) => {
                if supervisor_time > gateway_time {
                    ("service", supervisor_path)
                } else {
                    ("gateway", gateway_path)
                }
            }
            (Some(_), None) => ("gateway", gateway_path),
            (None, Some(_)) => ("service", supervisor_path),
            (None, None) => ("gateway", gateway_path),
        };

        Ok(LogTarget {
            env_name: name.to_string(),
            stream,
            source_kind: source_kind.to_string(),
            path,
        })
    }

    pub fn targets(&self, name: &str, stream: &str) -> Result<Vec<LogTarget>, String> {
        match normalize_stream(stream)? {
            "stdout" | "stderr" => Ok(vec![self.target(name, stream)?]),
            "all" => Ok(vec![
                self.target(name, "stdout")?,
                self.target(name, "stderr")?,
            ]),
            _ => unreachable!("stream validated by normalize_stream"),
        }
    }

    pub fn follow<W: std::io::Write>(
        &self,
        name: &str,
        stream: &str,
        tail_lines: Option<usize>,
        writer: &mut W,
    ) -> Result<LogTarget, String> {
        if normalize_stream(stream)? == "all" {
            self.follow_all(name, tail_lines, writer)?;
            return Ok(LogTarget {
                env_name: name.to_string(),
                stream: "all".to_string(),
                source_kind: "mixed".to_string(),
                path: PathBuf::new(),
            });
        }

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

    fn follow_all<W: std::io::Write>(
        &self,
        name: &str,
        tail_lines: Option<usize>,
        writer: &mut W,
    ) -> Result<(), String> {
        let targets = self.targets(name, "all")?;
        let summary = self.read(name, "all", tail_lines)?;
        writer
            .write_all(summary.content.as_bytes())
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;

        let mut cursors = targets
            .into_iter()
            .map(|target| FollowCursor {
                target,
                offset: 0,
                pending: String::new(),
            })
            .collect::<Vec<_>>();
        for cursor in &mut cursors {
            if let Ok(metadata) = fs::metadata(&cursor.target.path) {
                cursor.offset = metadata.len();
            }
        }

        loop {
            let mut updates = Vec::new();
            for cursor in &mut cursors {
                if !cursor.target.path.exists() {
                    continue;
                }

                let metadata =
                    fs::metadata(&cursor.target.path).map_err(|error| error.to_string())?;
                if metadata.len() < cursor.offset {
                    cursor.offset = 0;
                    cursor.pending.clear();
                }
                if metadata.len() == cursor.offset {
                    continue;
                }

                let mut file =
                    File::open(&cursor.target.path).map_err(|error| error.to_string())?;
                file.seek(SeekFrom::Start(cursor.offset))
                    .map_err(|error| error.to_string())?;
                let mut chunk = String::new();
                file.read_to_string(&mut chunk)
                    .map_err(|error| error.to_string())?;
                cursor.offset = metadata.len();
                updates.extend(collect_complete_lines(&mut cursor.pending, &chunk));
            }

            if !updates.is_empty() {
                let merged = merge_log_lines(updates);
                writer
                    .write_all(merged.as_bytes())
                    .map_err(|error| error.to_string())?;
                writer.flush().map_err(|error| error.to_string())?;
            }

            sleep(Duration::from_millis(FOLLOW_POLL_INTERVAL_MS));
        }
    }
}

struct FollowCursor {
    target: LogTarget,
    offset: u64,
    pending: String,
}

fn modified_at(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn normalize_stream(stream: &str) -> Result<&str, String> {
    match stream {
        "stdout" | "stderr" | "all" => Ok(stream),
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

fn summarize_sources(targets: &[LogTarget]) -> String {
    let mut kinds = targets
        .iter()
        .map(|target| target.source_kind.as_str())
        .collect::<Vec<_>>();
    kinds.sort_unstable();
    kinds.dedup();
    if kinds.len() == 1 {
        kinds[0].to_string()
    } else {
        "mixed".to_string()
    }
}

fn merge_log_texts(contents: Vec<(String, String)>, tail_lines: Option<usize>) -> String {
    let lines = contents
        .into_iter()
        .flat_map(|(_stream, content)| {
            content
                .split_inclusive('\n')
                .filter(|line| !line.is_empty())
                .map(|line| TimedLogLine::new(line.to_string()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let mut output = merge_log_lines(lines);
    if let Some(limit) = tail_lines {
        output = tail_text(&output, limit);
    }
    output
}

fn collect_complete_lines(pending: &mut String, chunk: &str) -> Vec<TimedLogLine> {
    pending.push_str(chunk);
    let mut lines = Vec::new();
    while let Some(newline_index) = pending.find('\n') {
        let line = pending[..=newline_index].to_string();
        pending.drain(..=newline_index);
        lines.push(TimedLogLine::new(line));
    }
    lines
}

fn merge_log_lines(mut lines: Vec<TimedLogLine>) -> String {
    lines.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.sequence.cmp(&right.sequence))
    });
    lines.into_iter().map(|line| line.text).collect()
}

#[derive(Clone, Debug)]
struct TimedLogLine {
    timestamp: Option<OffsetDateTime>,
    sequence: usize,
    text: String,
}

impl TimedLogLine {
    fn new(text: String) -> Self {
        Self {
            timestamp: parse_log_timestamp(&text),
            sequence: next_log_sequence(),
            text,
        }
    }
}

fn parse_log_timestamp(line: &str) -> Option<OffsetDateTime> {
    let token = line.split_whitespace().next()?;
    OffsetDateTime::parse(token, &time::format_description::well_known::Rfc3339)
        .ok()
        .or_else(|| {
            time::format_description::parse(
                "[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]",
            )
            .ok()
            .and_then(|format| OffsetDateTime::parse(token, &format).ok())
        })
}

fn next_log_sequence() -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
    NEXT_SEQUENCE.fetch_add(1, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::{LogService, merge_log_texts, tail_text};
    use crate::store::ensure_dir;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn tail_text_keeps_the_last_requested_lines() {
        assert_eq!(tail_text("one\ntwo\nthree\n", 2), "two\nthree\n");
        assert_eq!(tail_text("one\ntwo\nthree", 1), "three");
    }

    #[test]
    fn merge_log_texts_orders_stdout_and_stderr_by_timestamp() {
        let merged = merge_log_texts(
            vec![
                (
                    "stdout".to_string(),
                    concat!(
                        "2026-04-20T00:13:45.497+01:00 [gateway] one\n",
                        "2026-04-20T00:13:47.497+01:00 [gateway] three\n"
                    )
                    .to_string(),
                ),
                (
                    "stderr".to_string(),
                    "2026-04-20T00:13:46.497+01:00 error gateway two\n".to_string(),
                ),
            ],
            Some(10),
        );
        assert_eq!(
            merged,
            concat!(
                "2026-04-20T00:13:45.497+01:00 [gateway] one\n",
                "2026-04-20T00:13:46.497+01:00 error gateway two\n",
                "2026-04-20T00:13:47.497+01:00 [gateway] three\n",
            )
        );
    }

    #[test]
    fn target_prefers_the_newer_service_log_when_gateway_log_is_stale() {
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
        ensure_dir(&ocm_home.join("supervisor/logs")).unwrap();
        fs::write(
            ocm_home.join("envs.json"),
            format!(
                "{{\"kind\":\"ocm-env-registry\",\"envs\":[{{\"kind\":\"ocm-env\",\"name\":\"demo\",\"root\":\"{}\",\"gatewayPort\":18789,\"serviceEnabled\":false,\"serviceRunning\":false,\"protected\":false,\"createdAt\":\"1970-01-01T00:00:00Z\",\"updatedAt\":\"1970-01-01T00:00:00Z\"}}]}}",
                env_root.display()
            ),
        )
        .unwrap();
        fs::write(env_root.join(".openclaw/logs/gateway.log"), "hello\n").unwrap();
        sleep(Duration::from_millis(1100));
        fs::write(
            ocm_home.join("supervisor/logs/demo.stdout.log"),
            "newer service output\n",
        )
        .unwrap();

        let mut env = BTreeMap::new();
        env.insert("HOME".to_string(), root.join("home").display().to_string());
        env.insert("OCM_HOME".to_string(), ocm_home.display().to_string());
        let target = LogService::new(&env, Path::new(&cwd))
            .target("demo", "stdout")
            .unwrap();
        assert_eq!(target.source_kind, "service");
        assert!(target.path.ends_with("supervisor/logs/demo.stdout.log"));

        let _ = fs::remove_dir_all(&root);
    }
}
