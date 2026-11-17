//! Append-only audit log (`.openshell/audit.jsonl`) of every agent action.

use chrono::{DateTime, Utc};
use openshell_config::Permission;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    Ok,
    Error,
    /// Permission not granted.
    Denied,
    /// A human rejected (or did not answer) the approval request.
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub ts: DateTime<Utc>,
    /// Agent identity as reported by the client (`X-OpenShell-Agent` / MCP clientInfo).
    pub agent: String,
    /// `mcp`, `http`, `sdk`, `cli`.
    pub channel: String,
    pub tool: String,
    pub permission: Permission,
    /// Arguments, truncated for large values.
    pub args: serde_json::Value,
    pub outcome: AuditOutcome,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub approved_by_human: bool,
    pub duration_ms: u64,
}

pub struct AuditLog {
    path: Option<PathBuf>,
    recent: Mutex<VecDeque<AuditEntry>>,
}

const RECENT: usize = 500;
const MAX_STRING: usize = 2_000;

impl AuditLog {
    pub fn new(path: Option<PathBuf>) -> Self {
        if let Some(dir) = path.as_ref().and_then(|p| p.parent()) {
            let _ = std::fs::create_dir_all(dir);
        }
        Self { path, recent: Mutex::new(VecDeque::new()) }
    }

    pub fn record(&self, mut entry: AuditEntry) {
        entry.args = truncate_value(entry.args);
        if let Some(path) = &self.path {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                if let Ok(line) = serde_json::to_string(&entry) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }
        tracing::info!(
            agent = %entry.agent,
            tool = %entry.tool,
            outcome = ?entry.outcome,
            "agent action"
        );
        let mut recent = self.recent.lock().unwrap();
        if recent.len() >= RECENT {
            recent.pop_front();
        }
        recent.push_back(entry);
    }

    /// Most recent entries (oldest first).
    pub fn recent(&self, limit: usize) -> Vec<AuditEntry> {
        let recent = self.recent.lock().unwrap();
        let skip = recent.len().saturating_sub(limit);
        recent.iter().skip(skip).cloned().collect()
    }
}

/// Shorten long strings (e.g. file contents) so the log stays readable.
pub fn truncate_value(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) if s.len() > MAX_STRING => {
            let mut cut = MAX_STRING;
            while !s.is_char_boundary(cut) {
                cut -= 1;
            }
            Value::String(format!("{}… ({} bytes total)", &s[..cut], s.len()))
        }
        Value::Array(a) => Value::Array(a.into_iter().map(truncate_value).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, truncate_value(v))).collect()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_and_truncates() {
        let dir = tempfile::tempdir().unwrap();
        let log = AuditLog::new(Some(dir.path().join("audit.jsonl")));
        log.record(AuditEntry {
            ts: Utc::now(),
            agent: "test".into(),
            channel: "mcp".into(),
            tool: "fs_write".into(),
            permission: Permission::Code,
            args: serde_json::json!({ "path": "a.txt", "content": "x".repeat(5000) }),
            outcome: AuditOutcome::Ok,
            message: None,
            approved_by_human: true,
            duration_ms: 3,
        });
        let text = std::fs::read_to_string(dir.path().join("audit.jsonl")).unwrap();
        assert!(text.contains("\"tool\":\"fs_write\""));
        assert!(text.contains("5000 bytes total"));
        assert_eq!(log.recent(10).len(), 1);
    }
}
