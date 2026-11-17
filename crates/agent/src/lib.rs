//! # openshell-agent
//!
//! The Agent Interaction Layer: a model-agnostic, permission-checked interface
//! through which AI agents (MCP clients, coding agents, custom agent systems,
//! local models) can inspect and operate an OpenShell application.
//!
//! This crate contains the transport-independent building blocks:
//!
//! * [`policy`]   – permission scopes and approval requirements per mode
//! * [`approvals`] – human-in-the-loop approval queue for critical actions
//! * [`audit`]    – append-only JSONL audit log of every agent action
//! * [`bridge`]   – hub for UI bridge sessions (pages running `bridge.js`)
//! * [`tools`]    – the catalogue of built-in agent tools
//! * [`mcp`]      – Model Context Protocol (JSON-RPC 2.0) request handling
//! * [`providers`] – discovery of local / configured AI providers
//!
//! Tool execution itself lives in `openshell-core`, which wires these pieces
//! to the runtime and the desktop host.

pub mod approvals;
pub mod audit;
pub mod bridge;
pub mod mcp;
pub mod policy;
pub mod providers;
pub mod tools;

pub use openshell_config::Permission;

/// JavaScript injected into application pages. It connects the page to the
/// shell over a WebSocket and executes UI commands (snapshot, click, fill, ...).
pub const BRIDGE_JS: &str = include_str!("../assets/bridge.js");

/// Result of a tool call, independent of the transport (HTTP or MCP).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolOutcome {
    pub ok: bool,
    /// JSON result on success, `{ "error": "..." }` on failure.
    pub result: serde_json::Value,
}

impl ToolOutcome {
    pub fn ok(result: serde_json::Value) -> Self {
        Self { ok: true, result }
    }
    pub fn error(message: impl Into<String>) -> Self {
        Self { ok: false, result: serde_json::json!({ "error": message.into() }) }
    }
    pub fn error_message(&self) -> Option<String> {
        if self.ok {
            None
        } else {
            self.result.get("error").and_then(|e| e.as_str()).map(str::to_string)
        }
    }
}
