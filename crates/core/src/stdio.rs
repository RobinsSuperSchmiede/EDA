//! MCP server over stdin/stdout that proxies to a running shell.
//!
//! Used by `openshell mcp` (CLI) and by installed apps (`MyApp.exe --mcp`). Agents
//! start this as their MCP server; messages are forwarded to the shell's `/mcp`
//! (full profile) or `/mcp/app` (application profile) endpoint. stdout carries only
//! JSON-RPC, diagnostics go to stderr.

use crate::state::{ControlClient, NOT_RUNNING};
use anyhow::Result;
use openshell_agent::mcp::{handle_message, BoxFuture, CallContext, ToolProvider, INSTRUCTIONS};
use openshell_agent::tools::{builtin_tools, ToolDef};
use openshell_agent::ToolOutcome;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Which tool set the agent sees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpProfile {
    /// Everything: runtime, logs, services, files, UI automation, capabilities (developing the app).
    Full,
    /// Only the application's own functions (using the app).
    App,
}

/// Starts the application in the background when an agent connects and it is not running.
pub type Autostart = Box<dyn Fn() -> Result<()> + Send + Sync>;

struct Offline(McpProfile);

impl ToolProvider for Offline {
    fn list_tools(&self) -> Vec<ToolDef> {
        match self.0 {
            McpProfile::Full => builtin_tools(),
            McpProfile::App => vec![],
        }
    }
    fn call_tool<'a>(&'a self, _name: &'a str, _args: Value, _ctx: CallContext) -> BoxFuture<'a, ToolOutcome> {
        Box::pin(async { ToolOutcome::error(NOT_RUNNING) })
    }
    fn instructions(&self) -> String {
        format!("{INSTRUCTIONS} NOTE: the application is currently not running; ask the user to start it.")
    }
}

async fn connect(state_dir: &Option<PathBuf>, agent: &str) -> Option<ControlClient> {
    let dir = state_dir.as_ref()?;
    ControlClient::connect(dir).await.ok().map(|c| c.with_agent(agent.to_string()))
}

/// Wait until no service is still starting, so `tools/list` includes imported capabilities.
async fn wait_settled(client: &ControlClient, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let Ok(s) = client.get("/v1/status").await else { return };
        let busy = s["services"].as_array().into_iter().flatten().any(|svc| {
            matches!(svc["state"].as_str(), Some("pending" | "setup" | "building" | "starting" | "backoff"))
        });
        if !busy {
            // give OpenAPI imports a moment after services became healthy
            tokio::time::sleep(Duration::from_millis(300)).await;
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

pub async fn serve(state_dir: Option<PathBuf>, profile: McpProfile, autostart: Option<Autostart>) -> Result<()> {
    let endpoint = match profile {
        McpProfile::Full => "/mcp",
        McpProfile::App => "/mcp/app",
    };
    let mut agent = "mcp-agent".to_string();
    let mut client: Option<ControlClient> = None;
    let mut autostarted = false;
    let mut stdin = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = stdin.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let err = json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("parse error: {e}")}});
                write_line(&mut stdout, &err).await?;
                continue;
            }
        };
        if let Some(name) = msg.pointer("/params/clientInfo/name").and_then(|n| n.as_str()) {
            agent = name.to_string();
        }
        // (re)connect lazily so the agent can be started before the app
        let alive = match &client {
            Some(c) => c.get("/v1/health").await.is_ok(),
            None => false,
        };
        if !alive {
            client = connect(&state_dir, &agent).await;
            if client.is_none() && !autostarted {
                if let Some(start) = &autostart {
                    autostarted = true;
                    match start() {
                        Ok(()) => {
                            let deadline = Instant::now() + Duration::from_secs(60);
                            while client.is_none() && Instant::now() < deadline {
                                tokio::time::sleep(Duration::from_millis(250)).await;
                                client = connect(&state_dir, &agent).await;
                            }
                        }
                        Err(e) => eprintln!("openshell mcp: could not start the application: {e:#}"),
                    }
                }
            }
            if let Some(c) = &client {
                wait_settled(c, Duration::from_secs(60)).await;
            }
        }
        let ctx = CallContext {
            agent: agent.clone(),
            channel: "mcp".into(),
        };
        let reply = match &client {
            Some(c) => match c.clone().with_agent(agent.clone()).post(endpoint, &msg).await {
                Ok(Value::String(s)) if s.is_empty() => None, // notification accepted
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("openshell mcp: {e:#}");
                    handle_message(&Offline(profile), msg, ctx).await
                }
            },
            None => handle_message(&Offline(profile), msg, ctx).await,
        };
        if let Some(r) = reply {
            write_line(&mut stdout, &r).await?;
        }
    }
    Ok(())
}

async fn write_line(out: &mut tokio::io::Stdout, v: &Value) -> Result<()> {
    let mut s = serde_json::to_string(v)?;
    s.push('\n');
    out.write_all(s.as_bytes()).await?;
    out.flush().await?;
    Ok(())
}
