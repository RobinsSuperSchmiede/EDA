//! `.openshell/runtime.json` – how the CLI and agents find a running shell –
//! plus a small HTTP client for the control API.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use openshell_config::Mode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeInfo {
    pub pid: u32,
    /// Control API base URL.
    pub url: String,
    /// Administrator token (full access; keep private).
    pub token: String,
    pub mode: Mode,
    pub host: String,
    pub app: String,
    pub started_at: DateTime<Utc>,
    pub version: String,
}

pub const RUNTIME_FILE: &str = "runtime.json";

pub fn runtime_file(state_dir: &Path) -> PathBuf {
    state_dir.join(RUNTIME_FILE)
}

pub fn write_runtime_info(state_dir: &Path, info: &RuntimeInfo) -> Result<()> {
    std::fs::create_dir_all(state_dir)?;
    let path = runtime_file(state_dir);
    std::fs::write(&path, serde_json::to_vec_pretty(info)?)
        .with_context(|| format!("cannot write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    // keep runtime state out of version control
    let gi = state_dir.join(".gitignore");
    if !gi.exists() {
        let _ = std::fs::write(gi, "*\n");
    }
    Ok(())
}

pub fn read_runtime_info(state_dir: &Path) -> Option<RuntimeInfo> {
    let text = std::fs::read_to_string(runtime_file(state_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn remove_runtime_info(state_dir: &Path) {
    let _ = std::fs::remove_file(runtime_file(state_dir));
}

/// Client for the control API of a running shell.
#[derive(Clone)]
pub struct ControlClient {
    pub info: RuntimeInfo,
    http: reqwest::Client,
    agent: String,
}

impl ControlClient {
    /// Connect to the shell of the project whose state directory is `state_dir`.
    /// Fails if no shell is running (stale state files are ignored).
    pub async fn connect(state_dir: &Path) -> Result<Self> {
        let info = read_runtime_info(state_dir).ok_or_else(|| anyhow!(NOT_RUNNING))?;
        let client = Self {
            http: reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(300)).build()?,
            info,
            agent: "openshell-cli".into(),
        };
        match client.get("/v1/health").await {
            Ok(_) => Ok(client),
            Err(_) => bail!(NOT_RUNNING),
        }
    }

    /// Agent identity sent with tool calls (`X-OpenShell-Agent`).
    pub fn with_agent(mut self, agent: impl Into<String>) -> Self {
        self.agent = agent.into();
        self
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.info.url, path)
    }

    async fn handle(resp: reqwest::Response) -> Result<Value> {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::String(text.clone()));
        if !status.is_success() {
            let msg = value.get("error").and_then(|e| e.as_str()).map(str::to_string).unwrap_or(text);
            bail!("{msg}");
        }
        Ok(value)
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        let resp = self
            .http
            .get(self.url(path))
            .bearer_auth(&self.info.token)
            .header("X-OpenShell-Agent", &self.agent)
            .send()
            .await
            .context("control API not reachable")?;
        Self::handle(resp).await
    }

    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let resp = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.info.token)
            .header("X-OpenShell-Agent", &self.agent)
            .json(body)
            .send()
            .await
            .context("control API not reachable")?;
        Self::handle(resp).await
    }
}

pub const NOT_RUNNING: &str =
    "OpenShell is not running for this project. Start it with `openshell dev` or `openshell start`.";
