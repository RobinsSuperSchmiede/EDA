//! Agent sessions: every agent can get its own token with its own scopes and profile, instead of
//! all agents sharing the admin token of `.openshell/runtime.json`.
//!
//! `openshell agents token <name> --scopes read,actions --app` creates a session; its token is
//! shown once and stored only as a SHA-256 hash in `<data>/agent-sessions.json`. A session can
//! never get more than the app grants agents in the current mode: its scopes are intersected with
//! `agents.permissions`. Calls are audited as `session:<name>`. Revoking takes effect on the next
//! request, also while the app runs (the file is re-read when it changes).

use crate::remote::sha256_hex;
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use openshell_config::Permission;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

pub const SESSIONS_FILE: &str = "agent-sessions.json";

/// Which MCP endpoint a session may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionProfile {
    /// Only the app's own functions (`/mcp/app`).
    #[default]
    App,
    /// The developer tools as well (`/mcp`, `/v1/tools`).
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub name: String,
    pub token_sha256: String,
    #[serde(default)]
    pub profile: SessionProfile,
    /// Scopes of this session; empty = everything the app grants agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<Permission>,
    pub created_at: DateTime<Utc>,
}

/// What lists and the runtime panel show (never the hash).
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub profile: SessionProfile,
    pub scopes: Vec<Permission>,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
}

struct State {
    list: Vec<AgentSession>,
    modified: Option<SystemTime>,
    seen: std::collections::HashMap<String, DateTime<Utc>>,
}

pub struct Sessions {
    path: PathBuf,
    state: RwLock<State>,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

fn read(path: &Path) -> (Vec<AgentSession>, Option<SystemTime>) {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let list =
        std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    (list, modified)
}

impl Sessions {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join(SESSIONS_FILE);
        let (list, modified) = read(&path);
        Self { path, state: RwLock::new(State { list, modified, seen: Default::default() }) }
    }

    /// Pick up changes written by another process (the CLI while the app runs).
    fn refresh(&self) {
        let modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if modified != self.state.read().unwrap().modified {
            let (list, modified) = read(&self.path);
            let mut s = self.state.write().unwrap();
            s.list = list;
            s.modified = modified;
        }
    }

    fn save(&self, s: &mut State) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&self.path, serde_json::to_vec_pretty(&s.list)?)
            .with_context(|| format!("cannot write {}", self.path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        s.modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        Ok(())
    }

    /// Create (or replace) a session and return its token, shown once.
    pub fn create(&self, name: &str, profile: SessionProfile, scopes: Vec<Permission>) -> Result<String> {
        if !valid_name(name) {
            bail!("`{name}` is not a valid session name (letters, digits, `-`, `_`, `.`)");
        }
        self.refresh();
        let token = format!("osa_{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
        let mut s = self.state.write().unwrap();
        s.list.retain(|x| x.name != name);
        s.list.push(AgentSession {
            name: name.to_string(),
            token_sha256: sha256_hex(token.as_bytes()),
            profile,
            scopes,
            created_at: Utc::now(),
        });
        self.save(&mut s)?;
        Ok(token)
    }

    /// Revoke a session. Returns `false` if it did not exist.
    pub fn revoke(&self, name: &str) -> Result<bool> {
        self.refresh();
        let mut s = self.state.write().unwrap();
        let before = s.list.len();
        s.list.retain(|x| x.name != name);
        if s.list.len() == before {
            return Ok(false);
        }
        self.save(&mut s)?;
        Ok(true)
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.refresh();
        let s = self.state.read().unwrap();
        s.list
            .iter()
            .map(|x| SessionInfo {
                name: x.name.clone(),
                profile: x.profile,
                scopes: x.scopes.clone(),
                created_at: x.created_at,
                last_seen: s.seen.get(&x.name).copied(),
            })
            .collect()
    }

    /// The session a token belongs to.
    pub fn verify(&self, token: &str) -> Option<AgentSession> {
        if !token.starts_with("osa_") {
            return None;
        }
        self.refresh();
        let hash = sha256_hex(token.as_bytes());
        let mut s = self.state.write().unwrap();
        let found = s.list.iter().find(|x| crate::remote::same_token(&x.token_sha256, &hash)).cloned()?;
        s.seen.insert(found.name.clone(), Utc::now());
        Some(found)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_verify_revoke() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = Sessions::load(dir.path());
        let t = sessions.create("claude", SessionProfile::Full, vec![Permission::Read]).unwrap();
        assert!(t.starts_with("osa_"));
        let s = sessions.verify(&t).unwrap();
        assert_eq!(
            (s.name.as_str(), s.profile, s.scopes.as_slice()),
            ("claude", SessionProfile::Full, &[Permission::Read][..])
        );
        assert!(sessions.verify("osa_wrong").is_none());
        assert!(sessions.verify(&t.replace("osa_", "osd_")).is_none());
        assert!(!std::fs::read_to_string(dir.path().join(SESSIONS_FILE)).unwrap().contains(&t));

        // another process (the CLI) revokes it: the running app notices on the next request
        let cli = Sessions::load(dir.path());
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(cli.revoke("claude").unwrap());
        assert!(!cli.revoke("claude").unwrap());
        // make sure the modification time differs on file systems with coarse timestamps
        let f = std::fs::File::options().write(true).open(dir.path().join(SESSIONS_FILE)).unwrap();
        f.set_modified(SystemTime::now() + std::time::Duration::from_secs(2)).unwrap();
        assert!(sessions.verify(&t).is_none());
        assert!(sessions.create("bad name", SessionProfile::App, vec![]).is_err());
        assert!(sessions.list().is_empty());
    }
}
