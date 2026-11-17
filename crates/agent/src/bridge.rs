//! Hub for UI bridge sessions.
//!
//! Every page that runs `bridge.js` (the desktop WebView, or a browser tab in
//! browser/headless mode) opens a WebSocket to the control API. The hub keeps
//! track of these sessions, routes UI commands to them and collects the
//! actions pages register through the SDK (`OpenShell.agent.registerAction`).
//!
//! Protocol (JSON text frames):
//!
//! page -> shell: `{"type":"hello","url":..,"title":..,"host":"desktop"|"browser","actions":[..]}`,
//! `{"type":"page","url":..,"title":..}`, `{"type":"actions","actions":[..]}`,
//! `{"type":"result","id":1,"ok":true,"result":..}` / `{"type":"result","id":1,"ok":false,"error":".."}`
//!
//! shell -> page: `{"id":1,"method":"snapshot","params":{..}}`

use chrono::{DateTime, Utc};
use openshell_config::Permission;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// An action registered by the frontend at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageAction {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_action_permission")]
    pub permission: Permission,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
}

fn default_action_permission() -> Permission {
    Permission::Actions
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageInfo {
    pub id: String,
    pub url: String,
    pub title: String,
    /// `desktop` (embedded WebView) or `browser`.
    pub host: String,
    pub connected_at: DateTime<Utc>,
    pub actions: Vec<PageAction>,
}

struct Session {
    info: PageInfo,
    tx: mpsc::UnboundedSender<String>,
}

type Reply = Result<Value, String>;

#[derive(Default)]
pub struct UiHub {
    sessions: Mutex<Vec<Session>>,
    pending: Mutex<HashMap<u64, oneshot::Sender<Reply>>>,
    next_id: AtomicU64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Incoming {
    Hello {
        #[serde(default)]
        url: String,
        #[serde(default)]
        title: String,
        #[serde(default)]
        host: Option<String>,
        #[serde(default)]
        actions: Vec<PageAction>,
    },
    Page {
        #[serde(default)]
        url: String,
        #[serde(default)]
        title: String,
    },
    Actions {
        #[serde(default)]
        actions: Vec<PageAction>,
    },
    Result {
        id: u64,
        #[serde(default)]
        ok: bool,
        #[serde(default)]
        result: Value,
        #[serde(default)]
        error: Option<String>,
    },
}

impl UiHub {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new transport connection. Messages for the page arrive on the receiver.
    pub fn connect(&self) -> (String, mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let id = format!("page-{}", self.next_id.fetch_add(1, Ordering::Relaxed) + 1);
        self.sessions.lock().unwrap().push(Session {
            info: PageInfo {
                id: id.clone(),
                url: String::new(),
                title: String::new(),
                host: "browser".into(),
                connected_at: Utc::now(),
                actions: vec![],
            },
            tx,
        });
        (id, rx)
    }

    pub fn disconnect(&self, id: &str) {
        self.sessions.lock().unwrap().retain(|s| s.info.id != id);
    }

    /// Handle a text frame sent by a page.
    pub fn on_message(&self, session: &str, text: &str) {
        let Ok(msg) = serde_json::from_str::<Incoming>(text) else {
            tracing::debug!("ignoring malformed bridge message: {text}");
            return;
        };
        match msg {
            Incoming::Result { id, ok, result, error } => {
                if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(if ok {
                        Ok(result)
                    } else {
                        Err(error.unwrap_or_else(|| "unknown error".into()))
                    });
                }
            }
            other => {
                let mut sessions = self.sessions.lock().unwrap();
                if let Some(s) = sessions.iter_mut().find(|s| s.info.id == session) {
                    match other {
                        Incoming::Hello { url, title, host, actions } => {
                            s.info.url = url;
                            s.info.title = title;
                            if let Some(h) = host {
                                s.info.host = h;
                            }
                            s.info.actions = actions;
                        }
                        Incoming::Page { url, title } => {
                            s.info.url = url;
                            s.info.title = title;
                        }
                        Incoming::Actions { actions } => s.info.actions = actions,
                        Incoming::Result { .. } => unreachable!(),
                    }
                }
            }
        }
    }

    pub fn pages(&self) -> Vec<PageInfo> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.info.clone())
            .collect()
    }

    /// Default target: the most recently connected desktop page, else the most recent page.
    fn target(&self, page: Option<&str>) -> Result<(String, mpsc::UnboundedSender<String>), String> {
        let sessions = self.sessions.lock().unwrap();
        let found = match page {
            Some(id) => sessions.iter().find(|s| s.info.id == id),
            None => sessions
                .iter()
                .rev()
                .find(|s| s.info.host == "desktop")
                .or_else(|| sessions.last()),
        };
        match found {
            Some(s) => Ok((s.info.id.clone(), s.tx.clone())),
            None if page.is_some() => Err(format!("page `{}` is not connected", page.unwrap_or_default())),
            None => Err("no application page is connected to the UI bridge. Is the frontend open in the \
                 OpenShell window (or a browser tab with the bridge loaded)?"
                .into()),
        }
    }

    /// Send a command to a page and wait for its reply.
    pub async fn call(&self, page: Option<&str>, method: &str, params: Value, timeout: Duration) -> Reply {
        let (_, tx) = self.target(page)?;
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let (reply_tx, reply_rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, reply_tx);
        let frame = json!({ "id": id, "method": method, "params": params }).to_string();
        if tx.send(frame).is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err("page disconnected".into());
        }
        match tokio::time::timeout(timeout, reply_rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err("page disconnected before replying".into()),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(format!("page did not answer `{method}` within {}s", timeout.as_secs()))
            }
        }
    }

    /// All actions registered by connected pages (deduplicated by name).
    pub fn actions(&self) -> Vec<(String, PageAction)> {
        let mut out: Vec<(String, PageAction)> = Vec::new();
        for s in self.sessions.lock().unwrap().iter().rev() {
            for a in &s.info.actions {
                if !out.iter().any(|(_, x)| x.name == a.name) {
                    out.push((s.info.id.clone(), a.clone()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn routes_calls_to_pages() {
        let hub = Arc::new(UiHub::new());
        assert!(hub.call(None, "snapshot", json!({}), Duration::from_millis(50)).await.is_err());

        let (id, mut rx) = hub.connect();
        hub.on_message(
            &id,
            r#"{"type":"hello","url":"http://x/","title":"X","host":"desktop","actions":[{"name":"add_todo","description":"Add"}]}"#,
        );
        assert_eq!(hub.pages()[0].title, "X");
        assert_eq!(hub.actions()[0].1.name, "add_todo");
        assert_eq!(hub.actions()[0].1.permission, Permission::Actions);

        let h = hub.clone();
        let page = tokio::spawn(async move {
            let frame: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
            assert_eq!(frame["method"], "snapshot");
            let reply = json!({"type":"result","id":frame["id"],"ok":true,"result":{"elements":[]}});
            h.on_message(&id, &reply.to_string());
        });
        let r = hub.call(None, "snapshot", json!({}), Duration::from_secs(2)).await.unwrap();
        assert_eq!(r, json!({"elements": []}));
        page.await.unwrap();
    }
}
