//! Human-in-the-loop approvals for critical agent actions.
//!
//! A tool call that needs approval waits on [`Approvals::request`]. Humans
//! answer through the desktop dialog, the CLI (`openshell approve <id>`) or the
//! HTTP API. Unanswered requests are denied after a timeout.

use chrono::{DateTime, Utc};
use openshell_config::Permission;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{broadcast, oneshot};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub agent: String,
    pub tool: String,
    pub permission: Permission,
    /// Human readable description of what the agent wants to do.
    pub summary: String,
    pub args: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ApprovalEvent {
    Requested(ApprovalRequest),
    Resolved { id: String, approved: bool },
}

struct Pending {
    request: ApprovalRequest,
    tx: oneshot::Sender<bool>,
}

pub struct Approvals {
    pending: Mutex<HashMap<String, Pending>>,
    events: broadcast::Sender<ApprovalEvent>,
}

impl Default for Approvals {
    fn default() -> Self {
        Self::new()
    }
}

impl Approvals {
    pub fn new() -> Self {
        let (events, _) = broadcast::channel(64);
        Self { pending: Mutex::new(HashMap::new()), events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalEvent> {
        self.events.subscribe()
    }

    /// Ask a human. Resolves to `true` only on explicit approval.
    pub async fn request(
        &self,
        agent: &str,
        tool: &str,
        permission: Permission,
        summary: String,
        args: serde_json::Value,
        timeout: Duration,
    ) -> bool {
        let now = Utc::now();
        let request = ApprovalRequest {
            id: uuid::Uuid::new_v4().simple().to_string()[..8].to_string(),
            agent: agent.to_string(),
            tool: tool.to_string(),
            permission,
            summary,
            args: crate::audit::truncate_value(args),
            created_at: now,
            expires_at: now + chrono::Duration::from_std(timeout).unwrap_or_default(),
        };
        let id = request.id.clone();
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), Pending { request: request.clone(), tx });
        let _ = self.events.send(ApprovalEvent::Requested(request));
        let approved = matches!(tokio::time::timeout(timeout, rx).await, Ok(Ok(true)));
        if self.pending.lock().unwrap().remove(&id).is_some() {
            // timed out
            let _ = self.events.send(ApprovalEvent::Resolved { id, approved: false });
        }
        approved
    }

    /// Answer a pending request. Returns `false` if the id is unknown.
    pub fn resolve(&self, id: &str, approved: bool) -> bool {
        let pending = self.pending.lock().unwrap().remove(id);
        match pending {
            Some(p) => {
                let _ = p.tx.send(approved);
                let _ = self.events.send(ApprovalEvent::Resolved { id: id.to_string(), approved });
                true
            }
            None => false,
        }
    }

    pub fn pending(&self) -> Vec<ApprovalRequest> {
        let mut list: Vec<_> = self.pending.lock().unwrap().values().map(|p| p.request.clone()).collect();
        list.sort_by_key(|r| r.created_at);
        list
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn approve_and_timeout() {
        let a = Arc::new(Approvals::new());
        let mut rx = a.subscribe();
        let a2 = a.clone();
        let waiter = tokio::spawn(async move {
            a2.request(
                "agent",
                "fs_write",
                Permission::Code,
                "write x".into(),
                serde_json::json!({}),
                Duration::from_secs(5),
            )
            .await
        });
        let id = match rx.recv().await.unwrap() {
            ApprovalEvent::Requested(r) => r.id,
            _ => panic!(),
        };
        assert_eq!(a.pending().len(), 1);
        assert!(a.resolve(&id, true));
        assert!(waiter.await.unwrap());
        assert!(a.pending().is_empty());
        assert!(!a.resolve(&id, true));

        let denied = a
            .request(
                "agent",
                "x",
                Permission::System,
                "s".into(),
                serde_json::json!({}),
                Duration::from_millis(50),
            )
            .await;
        assert!(!denied);
        assert!(a.pending().is_empty());
    }
}
