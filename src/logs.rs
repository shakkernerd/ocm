use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use serde::Serialize;
use sha2::{Digest, Sha256};
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
        let stream = normalize_stream(stream)?;
        let targets = self.targets(name, stream)?;
        let merge_streams = stream == "all";
        self.follow_targets(name, targets, tail_lines, merge_streams, writer)?;
        if merge_streams {
            Ok(LogTarget {
                env_name: name.to_string(),
                stream: "all".to_string(),
                source_kind: "mixed".to_string(),
                path: PathBuf::new(),
            })
        } else {
            self.target(name, stream)
        }
    }

    fn follow_targets<W: Write>(
        &self,
        name: &str,
        targets: Vec<LogTarget>,
        tail_lines: Option<usize>,
        merge_streams: bool,
        writer: &mut W,
    ) -> Result<(), String> {
        let mut snapshots = Vec::new();
        let mut cursors = targets
            .into_iter()
            .map(FollowCursor::new)
            .collect::<Vec<_>>();
        for cursor in &mut cursors {
            if let Some(snapshot) = cursor.snapshot(tail_lines, merge_streams)? {
                snapshots.push(snapshot);
            }
        }
        if merge_streams && snapshots.is_empty() {
            return Err(format!(
                "no logs exist for env \"{}\" across stdout or stderr",
                name
            ));
        }
        if !snapshots.is_empty() {
            write_follow_snapshots(writer, snapshots, tail_lines, merge_streams)?;
        }

        loop {
            let mut records = Vec::new();
            let mut wrote = false;
            for cursor in &mut cursors {
                if !cursor.is_initialized() {
                    let Some(snapshot) = cursor.snapshot(tail_lines, merge_streams)? else {
                        continue;
                    };
                    if merge_streams {
                        records.extend(log_records(snapshot.bytes));
                    } else {
                        writer
                            .write_all(&snapshot.bytes)
                            .map_err(|error| error.to_string())?;
                        wrote = !snapshot.bytes.is_empty();
                    }
                    continue;
                }
                let Some(update) = cursor.read_update()? else {
                    continue;
                };
                if merge_streams {
                    records.extend(cursor.collect_complete_records(update));
                } else {
                    let bytes = cursor.single_stream_bytes(update);
                    writer
                        .write_all(&bytes)
                        .map_err(|error| error.to_string())?;
                    wrote = !bytes.is_empty();
                }
            }

            if merge_streams && !records.is_empty() {
                let merged = merge_log_records(records);
                writer
                    .write_all(&merged)
                    .map_err(|error| error.to_string())?;
                wrote = true;
            }
            if wrote {
                writer.flush().map_err(|error| error.to_string())?;
            }

            sleep(Duration::from_millis(FOLLOW_POLL_INTERVAL_MS));
        }
    }
}

struct FollowCursor {
    target: LogTarget,
    offset: u64,
    identity: Option<FileIdentity>,
    initialized: bool,
    pending: Vec<u8>,
    prefix_hasher: Sha256,
    file: Option<File>,
    single_line_open: bool,
    single_separator_pending: bool,
}

impl FollowCursor {
    fn new(target: LogTarget) -> Self {
        Self {
            target,
            offset: 0,
            identity: None,
            initialized: false,
            pending: Vec::new(),
            prefix_hasher: Sha256::new(),
            file: None,
            single_line_open: false,
            single_separator_pending: false,
        }
    }

    fn is_initialized(&self) -> bool {
        self.initialized
    }

    fn snapshot(
        &mut self,
        tail_lines: Option<usize>,
        hold_partial_record: bool,
    ) -> Result<Option<LogSnapshot>, String> {
        let mut attempts = 0;
        let (file, mut bytes, offset, prefix_hasher) = loop {
            let Some((mut file, _metadata)) = open_log(&self.target.path)? else {
                return Ok(None);
            };
            let (bytes, offset) = read_log_snapshot_from(&mut file, tail_lines)?;
            if let Some(prefix_hasher) = hash_file_prefix(&mut file, offset)? {
                break (file, bytes, offset, prefix_hasher);
            }
            attempts += 1;
            if attempts == 2 {
                return Err("log changed repeatedly while taking its snapshot".to_string());
            }
        };
        let final_metadata = file.metadata().map_err(|error| error.to_string())?;
        self.offset = offset;
        self.identity = Some(file_identity(&file, &final_metadata)?);
        self.initialized = true;
        self.prefix_hasher = prefix_hasher;
        self.file = Some(file);
        self.single_line_open = bytes.last().is_some_and(|byte| *byte != b'\n');
        if hold_partial_record && !bytes.ends_with(b"\n") {
            // A merged stream cannot publish a partial record without risking
            // another stream interleaving inside it. Single-stream follow stays immediate.
            let partial_start = trailing_partial_record_start(&bytes);
            self.pending.extend_from_slice(&bytes[partial_start..]);
            bytes.truncate(partial_start);
        }
        Ok(Some(LogSnapshot { bytes }))
    }

    fn read_update(&mut self) -> Result<Option<FollowUpdate>, String> {
        let Some((mut path_file, metadata)) = open_log(&self.target.path)? else {
            if self.identity.is_some() {
                let old = self.read_current_file_to_end()?;
                return Ok(old.into_update());
            }
            return Ok(None);
        };
        let identity = file_identity(&path_file, &metadata)?;
        let Some(seen_identity) = self.identity.as_ref() else {
            let (bytes, offset) = read_file_range(&mut path_file, 0, metadata.len())?;
            let mut prefix_hasher = Sha256::new();
            prefix_hasher.update(&bytes);
            self.adopt_file(path_file, offset, prefix_hasher)?;
            return Ok((!bytes.is_empty()).then_some(FollowUpdate::append(bytes)));
        };

        if seen_identity != &identity {
            // The retained handle still addresses the renamed file, so drain it
            // before adopting the replacement at the original pathname.
            let old = self.read_current_file_to_end()?;
            let (new_bytes, offset) = read_file_range(&mut path_file, 0, metadata.len())?;
            let mut prefix_hasher = Sha256::new();
            prefix_hasher.update(&new_bytes);
            self.adopt_file(path_file, offset, prefix_hasher)?;
            return Ok(Some(FollowUpdate::replace(old, new_bytes)));
        }

        let verified_hasher = if metadata.len() < self.offset {
            None
        } else {
            hash_file_prefix(&mut path_file, self.offset)?
        };
        let content_changed_before_cursor = verified_hasher
            .as_ref()
            .is_none_or(|hasher| !hashers_match(hasher, &self.prefix_hasher));
        let reset = metadata.len() < self.offset || content_changed_before_cursor;
        let start = if reset { 0 } else { self.offset };
        let (bytes, offset) = read_file_range(&mut path_file, start, metadata.len())?;
        let mut prefix_hasher = if reset {
            Sha256::new()
        } else {
            verified_hasher.expect("unchanged prefix has a verified digest")
        };
        prefix_hasher.update(&bytes);
        self.adopt_file(path_file, offset, prefix_hasher)?;

        if bytes.is_empty() && !reset {
            return Ok(None);
        }
        if reset {
            Ok(Some(FollowUpdate::reset(bytes)))
        } else {
            Ok(Some(FollowUpdate::append(bytes)))
        }
    }

    fn collect_complete_records(&mut self, update: FollowUpdate) -> Vec<TimedLogRecord> {
        let mut records = Vec::new();
        for segment in update.segments {
            if segment.reset_before && !self.pending.is_empty() {
                records.extend(log_records(std::mem::take(&mut self.pending)));
            }
            self.pending.extend_from_slice(&segment.bytes);
            records.extend(self.take_complete_records());
        }
        records
    }

    fn single_stream_bytes(&mut self, update: FollowUpdate) -> Vec<u8> {
        let mut output = Vec::new();
        for segment in update.segments {
            if segment.reset_before {
                self.single_separator_pending |= self.single_line_open;
            }
            self.append_single_stream_segment(&mut output, &segment.bytes);
        }
        output
    }

    fn append_single_stream_segment(&mut self, output: &mut Vec<u8>, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        if self.single_separator_pending {
            if bytes.first() != Some(&b'\n') {
                output.push(b'\n');
            }
            self.single_separator_pending = false;
        }
        output.extend_from_slice(bytes);
        self.single_line_open = !bytes.ends_with(b"\n");
    }

    fn take_complete_records(&mut self) -> Vec<TimedLogRecord> {
        let complete_len = if self.pending.ends_with(b"\n") {
            self.pending.len()
        } else {
            trailing_partial_record_start(&self.pending)
        };
        if complete_len > 0 {
            let complete = self.pending.drain(..complete_len).collect::<Vec<_>>();
            log_records(complete)
        } else {
            Vec::new()
        }
    }

    fn read_current_file_to_end(&mut self) -> Result<FileDrain, String> {
        let Some(file) = &mut self.file else {
            return Ok(FileDrain {
                bytes: Vec::new(),
                reset: false,
            });
        };
        let end = file.metadata().map_err(|error| error.to_string())?.len();
        let verified_hasher = if end < self.offset {
            None
        } else {
            hash_file_prefix(file, self.offset)?
        };
        let reset = verified_hasher
            .as_ref()
            .is_none_or(|hasher| !hashers_match(hasher, &self.prefix_hasher));
        let start = if reset { 0 } else { self.offset };
        let (bytes, offset) = read_file_range(file, start, end)?;
        let mut prefix_hasher = if reset {
            Sha256::new()
        } else {
            verified_hasher.expect("unchanged prefix has a verified digest")
        };
        prefix_hasher.update(&bytes);
        self.offset = offset;
        self.prefix_hasher = prefix_hasher;
        Ok(FileDrain { bytes, reset })
    }

    fn adopt_file(&mut self, file: File, offset: u64, prefix_hasher: Sha256) -> Result<(), String> {
        let metadata = file.metadata().map_err(|error| error.to_string())?;
        self.offset = offset;
        self.identity = Some(file_identity(&file, &metadata)?);
        self.prefix_hasher = prefix_hasher;
        self.file = Some(file);
        Ok(())
    }
}

struct FileDrain {
    bytes: Vec<u8>,
    reset: bool,
}

impl FileDrain {
    fn into_update(self) -> Option<FollowUpdate> {
        if self.reset {
            Some(FollowUpdate::reset(self.bytes))
        } else if self.bytes.is_empty() {
            None
        } else {
            Some(FollowUpdate::append(self.bytes))
        }
    }
}

struct FollowUpdate {
    segments: Vec<FollowSegment>,
}

struct FollowSegment {
    bytes: Vec<u8>,
    reset_before: bool,
}

impl FollowUpdate {
    fn append(bytes: Vec<u8>) -> Self {
        Self {
            segments: vec![FollowSegment {
                bytes,
                reset_before: false,
            }],
        }
    }

    fn reset(bytes: Vec<u8>) -> Self {
        Self {
            segments: vec![FollowSegment {
                bytes,
                reset_before: true,
            }],
        }
    }

    fn replace(old: FileDrain, new_bytes: Vec<u8>) -> Self {
        let mut segments = Vec::with_capacity(2);
        if old.reset || !old.bytes.is_empty() {
            segments.push(FollowSegment {
                bytes: old.bytes,
                reset_before: old.reset,
            });
        }
        segments.push(FollowSegment {
            bytes: new_bytes,
            reset_before: true,
        });
        Self { segments }
    }
}

struct LogSnapshot {
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial_number: u32,
        file_index: u64,
    },
    #[cfg(not(any(unix, windows)))]
    Portable {
        created: Option<std::time::SystemTime>,
    },
}

#[cfg(unix)]
fn file_identity(_file: &File, metadata: &fs::Metadata) -> Result<FileIdentity, String> {
    use std::os::unix::fs::MetadataExt;
    Ok(FileIdentity::Unix {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(windows)]
fn file_identity(file: &File, _metadata: &fs::Metadata) -> Result<FileIdentity, String> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = unsafe { std::mem::zeroed::<BY_HANDLE_FILE_INFORMATION>() };
    // The handle and output pointer remain valid for the duration of the call.
    let succeeded =
        unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) };
    if succeeded == 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(FileIdentity::Windows {
        volume_serial_number: information.dwVolumeSerialNumber,
        file_index: ((information.nFileIndexHigh as u64) << 32) | information.nFileIndexLow as u64,
    })
}

#[cfg(not(any(unix, windows)))]
fn file_identity(_file: &File, metadata: &fs::Metadata) -> Result<FileIdentity, String> {
    Ok(FileIdentity::Portable {
        created: metadata.created().ok(),
    })
}

fn open_log(path: &Path) -> Result<Option<(File, fs::Metadata)>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    Ok(Some((file, metadata)))
}

fn hash_file_prefix(file: &mut File, end: u64) -> Result<Option<Sha256>, String> {
    file.seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut remaining = end;
    let mut buffer = [0_u8; 64 * 1024];
    while remaining > 0 {
        let read_len = remaining.min(buffer.len() as u64) as usize;
        let count = file
            .read(&mut buffer[..read_len])
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Ok(None);
        }
        hasher.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(Some(hasher))
}

fn hashers_match(left: &Sha256, right: &Sha256) -> bool {
    left.clone().finalize() == right.clone().finalize()
}

fn read_file_range(file: &mut File, start: u64, end: u64) -> Result<(Vec<u8>, u64), String> {
    file.seek(SeekFrom::Start(start))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    (&mut *file)
        .take(end.saturating_sub(start))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let offset = start
        .checked_add(bytes.len() as u64)
        .ok_or_else(|| "log cursor offset overflowed".to_string())?;
    Ok((bytes, offset))
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
    let (mut file, _) =
        open_log(path)?.ok_or_else(|| format!("log does not exist: {}", display_path(path)))?;
    let (bytes, _) = read_log_snapshot_from(&mut file, tail_lines)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn read_log_snapshot_from(
    file: &mut File,
    tail_lines: Option<usize>,
) -> Result<(Vec<u8>, u64), String> {
    match tail_lines {
        Some(limit) => read_log_tail_snapshot(file, limit),
        None => {
            let snapshot_offset = file.metadata().map_err(|error| error.to_string())?.len();
            file.seek(SeekFrom::Start(0))
                .map_err(|error| error.to_string())?;
            let mut bytes = Vec::new();
            (&mut *file)
                .take(snapshot_offset)
                .read_to_end(&mut bytes)
                .map_err(|error| error.to_string())?;
            Ok((
                bytes,
                file.stream_position().map_err(|error| error.to_string())?,
            ))
        }
    }
}

fn read_log_tail_snapshot(file: &mut File, tail_lines: usize) -> Result<(Vec<u8>, u64), String> {
    if tail_lines == 0 {
        let offset = file
            .seek(SeekFrom::End(0))
            .map_err(|error| error.to_string())?;
        return Ok((Vec::new(), offset));
    }

    let mut position = file
        .seek(SeekFrom::End(0))
        .map_err(|error| error.to_string())?;
    let snapshot_offset = position;
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
    Ok((tail_bytes(&bytes, tail_lines), snapshot_offset))
}

fn tail_bytes(bytes: &[u8], tail_lines: usize) -> Vec<u8> {
    if tail_lines == 0 {
        return Vec::new();
    }
    let lines = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    let start = lines.len().saturating_sub(tail_lines);
    lines[start..].concat()
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
    let snapshots = contents
        .into_iter()
        .map(|(_stream, content)| LogSnapshot {
            bytes: content.into_bytes(),
        })
        .collect::<Vec<_>>();
    String::from_utf8(merge_log_snapshots(snapshots, tail_lines))
        .expect("merging valid UTF-8 preserves valid UTF-8")
}

fn write_follow_snapshots<W: Write>(
    writer: &mut W,
    snapshots: Vec<LogSnapshot>,
    tail_lines: Option<usize>,
    merge_streams: bool,
) -> Result<(), String> {
    let bytes = if merge_streams {
        merge_log_snapshots(snapshots, tail_lines)
    } else {
        snapshots
            .into_iter()
            .next()
            .map_or_else(Vec::new, |snapshot| snapshot.bytes)
    };
    if !bytes.is_empty() {
        writer
            .write_all(&bytes)
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn merge_log_snapshots(snapshots: Vec<LogSnapshot>, tail_lines: Option<usize>) -> Vec<u8> {
    let records = snapshots
        .into_iter()
        .flat_map(|snapshot| log_records(snapshot.bytes))
        .collect::<Vec<_>>();
    let mut output = merge_log_records(records);
    if let Some(limit) = tail_lines {
        output = tail_bytes(&output, limit);
    }
    output
}

fn log_records(bytes: Vec<u8>) -> Vec<TimedLogRecord> {
    if bytes.is_empty() {
        return Vec::new();
    }
    let mut records = Vec::new();
    let mut current: Option<TimedLogRecord> = None;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let timestamp = parse_log_timestamp(line);
        if timestamp.is_some() {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(TimedLogRecord::new(timestamp, line.to_vec()));
        } else if let Some(record) = &mut current {
            record.bytes.extend_from_slice(line);
        } else {
            current = Some(TimedLogRecord::new(None, line.to_vec()));
        }
    }
    if let Some(record) = current {
        records.push(record);
    }
    records
}

fn trailing_partial_record_start(bytes: &[u8]) -> usize {
    let mut offset = 0;
    let mut last_timestamp_start = None;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if parse_log_timestamp(line).is_some() {
            last_timestamp_start = Some(offset);
        }
        offset += line.len();
    }
    last_timestamp_start.unwrap_or(0)
}

fn merge_log_records(mut records: Vec<TimedLogRecord>) -> Vec<u8> {
    records.sort_by(|left, right| match (left.timestamp, right.timestamp) {
        (Some(left_time), Some(right_time)) => left_time
            .cmp(&right_time)
            .then_with(|| left.sequence.cmp(&right.sequence)),
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (None, None) => left.sequence.cmp(&right.sequence),
    });
    let mut output = Vec::new();
    for record in records {
        output.extend_from_slice(&record.bytes);
        if !output.ends_with(b"\n") {
            output.push(b'\n');
        }
    }
    output
}

#[derive(Clone, Debug)]
struct TimedLogRecord {
    timestamp: Option<OffsetDateTime>,
    sequence: usize,
    bytes: Vec<u8>,
}

impl TimedLogRecord {
    fn new(timestamp: Option<OffsetDateTime>, bytes: Vec<u8>) -> Self {
        Self {
            timestamp,
            sequence: next_log_sequence(),
            bytes,
        }
    }
}

fn parse_log_timestamp(line: &[u8]) -> Option<OffsetDateTime> {
    let token_start = line.iter().position(|byte| !byte.is_ascii_whitespace())?;
    let token_end = line[token_start..]
        .iter()
        .position(|byte| byte.is_ascii_whitespace())
        .map_or(line.len(), |offset| token_start + offset);
    let token = std::str::from_utf8(&line[token_start..token_end]).ok()?;
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
    use super::{
        FollowCursor, FollowUpdate, LogService, LogSnapshot, LogTarget, merge_log_snapshots,
        merge_log_texts, tail_bytes,
    };
    use crate::store::ensure_dir;
    use std::collections::BTreeMap;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::thread::sleep;
    use std::time::Duration;

    fn update_bytes(update: &FollowUpdate) -> Vec<u8> {
        update
            .segments
            .iter()
            .flat_map(|segment| segment.bytes.iter().copied())
            .collect()
    }

    fn update_has_reset(update: &FollowUpdate) -> bool {
        update.segments.iter().any(|segment| segment.reset_before)
    }

    #[test]
    fn tail_bytes_keeps_the_last_requested_lines() {
        assert_eq!(tail_bytes(b"one\ntwo\nthree\n", 2), b"two\nthree\n");
        assert_eq!(tail_bytes(b"one\ntwo\nthree", 1), b"three");
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
    fn merge_log_snapshots_keeps_multiline_records_with_their_parent() {
        let merged = merge_log_snapshots(
            vec![
                LogSnapshot {
                    bytes: concat!(
                        "2026-04-20T00:13:45Z [gateway] request failed\n",
                        "    at first frame\n",
                        "    at second frame\n",
                        "2026-04-20T00:13:47Z [gateway] recovered\n"
                    )
                    .as_bytes()
                    .to_vec(),
                },
                LogSnapshot {
                    bytes: b"2026-04-20T00:13:46Z error worker retrying\n".to_vec(),
                },
            ],
            None,
        );
        assert_eq!(
            merged,
            concat!(
                "2026-04-20T00:13:45Z [gateway] request failed\n",
                "    at first frame\n",
                "    at second frame\n",
                "2026-04-20T00:13:46Z error worker retrying\n",
                "2026-04-20T00:13:47Z [gateway] recovered\n"
            )
            .as_bytes()
        );
    }

    #[test]
    fn merge_log_snapshots_separates_unterminated_records_and_preserves_bytes() {
        let merged = merge_log_snapshots(
            vec![
                LogSnapshot {
                    bytes: b"2026-04-20T00:13:46Z [gateway] invalid \xff".to_vec(),
                },
                LogSnapshot {
                    bytes: b"2026-04-20T00:13:45Z error worker first\n".to_vec(),
                },
            ],
            None,
        );
        assert_eq!(
            merged,
            b"2026-04-20T00:13:45Z error worker first\n\
              2026-04-20T00:13:46Z [gateway] invalid \xff\n"
        );
    }

    #[test]
    fn merge_log_snapshots_ignores_empty_sources() {
        let merged = merge_log_snapshots(
            vec![
                LogSnapshot { bytes: Vec::new() },
                LogSnapshot {
                    bytes: b"2026-04-20T00:13:45Z info present\n".to_vec(),
                },
            ],
            None,
        );
        assert_eq!(merged, b"2026-04-20T00:13:45Z info present\n");
    }

    #[test]
    fn merge_log_snapshots_recognizes_indented_timestamps() {
        let merged = merge_log_snapshots(
            vec![
                LogSnapshot {
                    bytes: b"  2026-04-20T00:13:46Z info indented\n".to_vec(),
                },
                LogSnapshot {
                    bytes: b"2026-04-20T00:13:45Z info first\n".to_vec(),
                },
            ],
            None,
        );
        assert_eq!(
            merged,
            b"2026-04-20T00:13:45Z info first\n  2026-04-20T00:13:46Z info indented\n"
        );
    }

    #[test]
    fn merge_log_snapshots_totally_orders_dated_and_undated_records() {
        let merged = merge_log_snapshots(
            vec![
                LogSnapshot {
                    bytes: b"2026-04-20T00:13:47Z info later\n".to_vec(),
                },
                LogSnapshot {
                    bytes: b"undated record\n2026-04-20T00:13:45Z info earlier\n".to_vec(),
                },
            ],
            None,
        );
        assert_eq!(
            merged,
            b"undated record\n\
              2026-04-20T00:13:45Z info earlier\n\
              2026-04-20T00:13:47Z info later\n"
        );
    }

    #[test]
    fn tail_bytes_preserves_invalid_utf8() {
        assert_eq!(tail_bytes(b"first\nsecond \xff\n", 1), b"second \xff\n");
    }

    #[test]
    fn follow_cursor_buffers_split_records_as_bytes() {
        let root = test_root("cursor-split");
        let path = root.join("gateway.log");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        let snapshot = cursor.snapshot(None, false).unwrap().unwrap();
        assert_eq!(snapshot.bytes, b"initial\n");

        append(&path, b"2026-04-20T00:13:45Z info split \xe2");
        let update = cursor.read_update().unwrap().unwrap();
        assert!(cursor.collect_complete_records(update).is_empty());

        append(&path, b"\x82\xac\n");
        let update = cursor.read_update().unwrap().unwrap();
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(merged, "2026-04-20T00:13:45Z info split €\n".as_bytes());
        assert_eq!(cursor.offset, fs::metadata(&path).unwrap().len());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_buffers_the_whole_partial_multiline_record() {
        let root = test_root("cursor-multiline-partial");
        let path = root.join("gateway.log");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, true).unwrap().unwrap();

        append(
            &path,
            b"2026-04-20T00:13:45Z error request failed\n    partial frame",
        );
        let update = cursor.read_update().unwrap().unwrap();
        assert!(cursor.collect_complete_records(update).is_empty());

        append(&path, b" completed\n");
        let update = cursor.read_update().unwrap().unwrap();
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(
            merged,
            concat!(
                "2026-04-20T00:13:45Z error request failed\n",
                "    partial frame completed\n"
            )
            .as_bytes()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_keeps_undated_partial_multiline_records_together() {
        let root = test_root("cursor-undated-multiline-partial");
        let path = root.join("gateway.log");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, true).unwrap().unwrap();

        append(&path, b"frame one\nframe two");
        let update = cursor.read_update().unwrap().unwrap();
        assert!(cursor.collect_complete_records(update).is_empty());

        append(&path, b" completed\n");
        let update = cursor.read_update().unwrap().unwrap();
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(merged, b"frame one\nframe two completed\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn merged_follow_retains_a_partial_startup_record() {
        let root = test_root("cursor-startup-partial");
        let path = root.join("gateway.log");
        fs::write(
            &path,
            concat!(
                "2026-04-20T00:13:44Z info previous\n",
                "2026-04-20T00:13:45Z error failed\n",
                "    split "
            )
            .as_bytes()
            .iter()
            .copied()
            .chain([0xe2])
            .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        let snapshot = cursor.snapshot(None, true).unwrap().unwrap();
        assert_eq!(snapshot.bytes, b"2026-04-20T00:13:44Z info previous\n");

        append(&path, b"\x82\xac\n");
        let update = cursor.read_update().unwrap().unwrap();
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(
            merged,
            concat!("2026-04-20T00:13:45Z error failed\n", "    split €\n").as_bytes()
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn delayed_follow_snapshot_applies_the_requested_tail() {
        let root = test_root("cursor-delayed-tail");
        let path = root.join("gateway.log");
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        assert!(cursor.snapshot(Some(1), false).unwrap().is_none());

        fs::write(&path, b"first\nsecond\nthird\n").unwrap();
        let snapshot = cursor.snapshot(Some(1), false).unwrap().unwrap();
        assert_eq!(snapshot.bytes, b"third\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_detects_replacement_and_separates_pending_bytes() {
        let root = test_root("cursor-replace");
        let path = root.join("gateway.log");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, false).unwrap().unwrap();

        append(&path, b"old partial");
        let update = cursor.read_update().unwrap().unwrap();
        assert!(cursor.collect_complete_records(update).is_empty());

        fs::rename(&path, root.join("gateway.log.1")).unwrap();
        fs::write(&path, b"2026-04-20T00:13:46Z info new file\n").unwrap();
        let update = cursor.read_update().unwrap().unwrap();
        assert!(update_has_reset(&update));
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(merged, b"old partial\n2026-04-20T00:13:46Z info new file\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_drains_a_renamed_file_before_reading_its_replacement() {
        let root = test_root("cursor-rename-drain");
        let path = root.join("gateway.log");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, true).unwrap().unwrap();

        append(&path, b"2026-04-20T00:13:45Z info old final\n");
        fs::rename(&path, root.join("gateway.log.1")).unwrap();
        fs::write(&path, b"2026-04-20T00:13:46Z info new first\n").unwrap();

        let update = cursor.read_update().unwrap().unwrap();
        assert!(update_has_reset(&update));
        assert_eq!(
            update_bytes(&update),
            b"2026-04-20T00:13:45Z info old final\n\
              2026-04-20T00:13:46Z info new first\n"
        );
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(
            merged,
            b"2026-04-20T00:13:45Z info old final\n\
              2026-04-20T00:13:46Z info new first\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_drains_late_writes_across_a_missing_rotation_gap() {
        let root = test_root("cursor-missing-rotation-drain");
        let path = root.join("gateway.log");
        let rotated_path = root.join("gateway.log.1");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, true).unwrap().unwrap();

        fs::rename(&path, &rotated_path).unwrap();
        assert!(cursor.read_update().unwrap().is_none());

        append(&rotated_path, b"2026-04-20T00:13:45Z info late old write\n");
        fs::write(&path, b"2026-04-20T00:13:46Z info new first\n").unwrap();
        let update = cursor.read_update().unwrap().unwrap();
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(
            merged,
            b"2026-04-20T00:13:45Z info late old write\n\
              2026-04-20T00:13:46Z info new first\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_detects_truncation_while_the_path_is_missing() {
        let root = test_root("cursor-missing-truncate");
        let path = root.join("gateway.log");
        let rotated_path = root.join("gateway.log.1");
        fs::write(&path, b"2026-04-20T00:13:45Z info old partial").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        let snapshot = cursor.snapshot(None, true).unwrap().unwrap();
        assert!(snapshot.bytes.is_empty());

        fs::rename(&path, &rotated_path).unwrap();
        assert!(cursor.read_update().unwrap().is_none());
        fs::write(&rotated_path, b"2026-04-20T00:13:46Z info rewritten\n").unwrap();
        let update = cursor.read_update().unwrap().unwrap();
        assert!(update_has_reset(&update));
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(
            merged,
            b"2026-04-20T00:13:45Z info old partial\n\
              2026-04-20T00:13:46Z info rewritten\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn single_stream_follow_separates_rotated_unterminated_records() {
        let root = test_root("cursor-single-rotation-boundary");
        let path = root.join("gateway.log");
        fs::write(&path, b"initial\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, false).unwrap().unwrap();

        append(&path, b"old partial");
        fs::rename(&path, root.join("gateway.log.1")).unwrap();
        fs::write(&path, b"new record\n").unwrap();
        let update = cursor.read_update().unwrap().unwrap();
        assert_eq!(
            cursor.single_stream_bytes(update),
            b"old partial\nnew record\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn single_stream_follow_does_not_duplicate_a_replacement_newline() {
        let root = test_root("cursor-single-newline-boundary");
        let path = root.join("gateway.log");
        fs::write(&path, b"old partial").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, false).unwrap().unwrap();

        assert_eq!(
            cursor.single_stream_bytes(FollowUpdate::reset(b"\nnew record\n".to_vec())),
            b"\nnew record\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_preserves_pending_records_while_the_path_is_missing() {
        let root = test_root("cursor-missing");
        let path = root.join("gateway.log");
        let rotated_path = root.join("gateway.log.1");
        fs::write(&path, b"2026-04-20T00:13:45Z error final failure").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        let snapshot = cursor.snapshot(None, true).unwrap().unwrap();
        assert!(snapshot.bytes.is_empty());

        fs::rename(&path, &rotated_path).unwrap();
        assert!(cursor.read_update().unwrap().is_none());
        append(&rotated_path, b" recovered\n");
        fs::write(&path, b"2026-04-20T00:13:46Z info new file\n").unwrap();
        let update = cursor.read_update().unwrap().unwrap();
        let merged = super::merge_log_records(cursor.collect_complete_records(update));
        assert_eq!(
            merged,
            b"2026-04-20T00:13:45Z error final failure recovered\n\
              2026-04-20T00:13:46Z info new file\n"
        );

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_does_not_reapply_tail_after_a_missing_rotation_gap() {
        let root = test_root("cursor-rotation-gap");
        let path = root.join("gateway.log");
        fs::write(&path, b"old\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        let snapshot = cursor.snapshot(Some(0), false).unwrap().unwrap();
        assert!(snapshot.bytes.is_empty());

        fs::remove_file(&path).unwrap();
        assert!(cursor.read_update().unwrap().is_none());
        fs::write(&path, b"new first\nnew second\n").unwrap();
        assert!(cursor.is_initialized());
        let update = cursor.read_update().unwrap().unwrap();
        assert_eq!(update_bytes(&update), b"new first\nnew second\n");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_reads_from_the_actual_file_length() {
        let root = test_root("cursor-offset");
        let path = root.join("gateway.log");
        fs::write(&path, b"first\n").unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, false).unwrap().unwrap();

        append(&path, b"second\n");
        let update = cursor.read_update().unwrap().unwrap();
        assert_eq!(update_bytes(&update), b"second\n");
        assert_eq!(cursor.offset, fs::metadata(&path).unwrap().len());

        append(&path, b"third\n");
        let update = cursor.read_update().unwrap().unwrap();
        assert_eq!(update_bytes(&update), b"third\n");
        assert_eq!(cursor.offset, fs::metadata(&path).unwrap().len());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn follow_cursor_hashes_the_consumed_prefix_for_same_file_rewrites() {
        let root = test_root("cursor-rewrite");
        let path = root.join("gateway.log");
        let mut original = b"old-prefix\n".to_vec();
        original.extend(std::iter::repeat_n(b'x', 64));
        fs::write(&path, &original).unwrap();
        let mut cursor = FollowCursor::new(log_target(path.clone()));
        cursor.snapshot(None, false).unwrap().unwrap();

        fs::write(&path, &original).unwrap();
        assert!(cursor.read_update().unwrap().is_none());

        let mut rewritten = b"new-prefix\n".to_vec();
        rewritten.extend(std::iter::repeat_n(b'x', 64));
        fs::write(&path, &rewritten).unwrap();
        let update = cursor.read_update().unwrap().unwrap();
        assert!(update_has_reset(&update));
        assert_eq!(update_bytes(&update), rewritten);

        let _ = fs::remove_dir_all(root);
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

    fn test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "ocm-log-{label}-{}-{}",
            std::process::id(),
            crate::store::now_utc().unix_timestamp_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn log_target(path: PathBuf) -> LogTarget {
        LogTarget {
            env_name: "demo".to_string(),
            stream: "stdout".to_string(),
            source_kind: "gateway".to_string(),
            path,
        }
    }

    fn append(path: &Path, bytes: &[u8]) {
        OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(bytes)
            .unwrap();
    }
}
