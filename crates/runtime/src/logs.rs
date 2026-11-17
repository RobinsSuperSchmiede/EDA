//! In-memory ring buffer + per-service log files.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::broadcast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
    /// Messages emitted by OpenShell itself about a service.
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    /// Monotonic sequence number (use as cursor with `since`).
    pub seq: u64,
    pub ts: DateTime<Utc>,
    pub service: String,
    pub stream: LogStream,
    pub text: String,
}

/// Filter for [`LogStore::query`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LogQuery {
    pub service: Option<String>,
    /// Only lines with `seq > since`.
    pub since: Option<u64>,
    /// Maximum number of (most recent) lines.
    pub limit: Option<usize>,
    /// Case-insensitive substring filter.
    pub contains: Option<String>,
    /// Only stderr and system lines.
    #[serde(default)]
    pub errors_only: bool,
}

struct Inner {
    buf: VecDeque<LogLine>,
    next_seq: u64,
    files: HashMap<String, File>,
}

pub struct LogStore {
    inner: Mutex<Inner>,
    capacity: usize,
    dir: Option<PathBuf>,
    tx: broadcast::Sender<LogLine>,
}

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

impl LogStore {
    /// `dir`: optional directory for `<service>.log` files.
    pub fn new(capacity: usize, dir: Option<PathBuf>) -> Self {
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        let (tx, _) = broadcast::channel(1024);
        Self {
            inner: Mutex::new(Inner {
                buf: VecDeque::with_capacity(capacity.min(4096)),
                next_seq: 1,
                files: HashMap::new(),
            }),
            capacity: capacity.max(100),
            dir,
            tx,
        }
    }

    pub fn push(&self, service: &str, stream: LogStream, text: impl Into<String>) {
        let text = strip_ansi(&text.into());
        let text = text.trim_end_matches(['\r', '\n']).to_string();
        let mut inner = self.inner.lock().unwrap();
        let line =
            LogLine { seq: inner.next_seq, ts: Utc::now(), service: service.to_string(), stream, text };
        inner.next_seq += 1;
        if let Some(dir) = &self.dir {
            if !inner.files.contains_key(service) {
                if let Some(f) = open_log_file(dir, service) {
                    inner.files.insert(service.to_string(), f);
                }
            }
            if let Some(f) = inner.files.get_mut(service) {
                let tag = match stream {
                    LogStream::Stdout => "out",
                    LogStream::Stderr => "err",
                    LogStream::System => "sys",
                };
                let _ = writeln!(f, "{} [{tag}] {}", line.ts.format("%Y-%m-%dT%H:%M:%S%.3fZ"), line.text);
            }
        }
        if inner.buf.len() >= self.capacity {
            inner.buf.pop_front();
        }
        inner.buf.push_back(line.clone());
        drop(inner);
        let _ = self.tx.send(line);
    }

    pub fn system(&self, service: &str, text: impl Into<String>) {
        self.push(service, LogStream::System, text);
    }

    pub fn query(&self, q: &LogQuery) -> Vec<LogLine> {
        let inner = self.inner.lock().unwrap();
        let needle = q.contains.as_ref().map(|s| s.to_lowercase());
        let mut out: Vec<LogLine> = inner
            .buf
            .iter()
            .filter(|l| q.since.map(|s| l.seq > s).unwrap_or(true))
            .filter(|l| q.service.as_ref().map(|s| &l.service == s).unwrap_or(true))
            .filter(|l| !q.errors_only || l.stream != LogStream::Stdout)
            .filter(|l| needle.as_ref().map(|n| l.text.to_lowercase().contains(n)).unwrap_or(true))
            .cloned()
            .collect();
        if let Some(limit) = q.limit {
            if out.len() > limit {
                out.drain(..out.len() - limit);
            }
        }
        out
    }

    /// Sequence number of the newest line (0 if empty).
    pub fn last_seq(&self) -> u64 {
        self.inner.lock().unwrap().next_seq - 1
    }

    pub fn subscribe(&self) -> broadcast::Receiver<LogLine> {
        self.tx.subscribe()
    }
}

fn open_log_file(dir: &std::path::Path, service: &str) -> Option<File> {
    let path = dir.join(format!("{service}.log"));
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_FILE_BYTES {
            let _ = std::fs::rename(&path, dir.join(format!("{service}.log.1")));
        }
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Remove ANSI escape sequences (colors, cursor movement) so logs are readable for agents.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    // CSI: parameters until a final byte in 0x40..=0x7E
                    for n in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&n) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    // OSC: until BEL or ESC \
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_and_query() {
        let store = LogStore::new(100, None);
        for i in 0..150 {
            store.push("a", LogStream::Stdout, format!("line {i}"));
        }
        store.push("b", LogStream::Stderr, "\u{1b}[31mERROR\u{1b}[0m boom");
        let all = store.query(&LogQuery::default());
        assert_eq!(all.len(), 100);
        assert_eq!(all.last().unwrap().text, "ERROR boom");
        let b = store.query(&LogQuery { service: Some("b".into()), ..Default::default() });
        assert_eq!(b.len(), 1);
        let since = store.query(&LogQuery { since: Some(store.last_seq() - 2), ..Default::default() });
        assert_eq!(since.len(), 2);
        let errs =
            store.query(&LogQuery { errors_only: true, contains: Some("boom".into()), ..Default::default() });
        assert_eq!(errs.len(), 1);
        let limited = store.query(&LogQuery { limit: Some(3), ..Default::default() });
        assert_eq!(limited.len(), 3);
        assert_eq!(limited[2].service, "b");
    }

    #[test]
    fn writes_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = LogStore::new(10, Some(dir.path().to_path_buf()));
        store.push("svc", LogStream::Stdout, "hello");
        let content = std::fs::read_to_string(dir.path().join("svc.log")).unwrap();
        assert!(content.contains("[out] hello"));
    }
}
