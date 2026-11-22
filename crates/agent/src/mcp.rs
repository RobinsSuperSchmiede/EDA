//! Model Context Protocol (MCP) server logic, transport independent.
//!
//! OpenShell exposes the same tool catalogue over two MCP transports:
//! * Streamable HTTP: `POST /mcp` on the control API (JSON responses),
//! * stdio: `openshell mcp`, which proxies to the running shell.
//!
//! MCP is an adapter on top of OpenShell's own tool API (`/v1/tools`), so
//! agents that do not speak MCP can use plain HTTP/JSON.

use crate::tools::ToolDef;
use crate::ToolOutcome;
use openshell_config::Permission;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

pub const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Identity of the caller, used for permission checks and the audit log.
#[derive(Debug, Clone)]
pub struct CallContext {
    pub agent: String,
    /// `mcp`, `http`, `sdk`, `cli`
    pub channel: String,
}

pub trait ToolProvider: Send + Sync {
    fn list_tools(&self) -> Vec<ToolDef>;
    fn call_tool<'a>(&'a self, name: &'a str, args: Value, ctx: CallContext) -> BoxFuture<'a, ToolOutcome>;
    /// Server instructions shown to the model.
    /// MCP resources (`{uri, name, description, mimeType}`) – read-only context for the model.
    fn resources(&self) -> Vec<Value> {
        vec![]
    }
    /// Contents of a resource (`[{uri, mimeType, text}]`).
    fn read_resource<'a>(&'a self, uri: &'a str) -> BoxFuture<'a, Result<Vec<Value>, String>> {
        Box::pin(async move { Err(format!("unknown resource `{uri}`")) })
    }
    fn instructions(&self) -> String {
        INSTRUCTIONS.to_string()
    }
}

pub const INSTRUCTIONS: &str = "You are connected to an OpenShell desktop application. \
OpenShell runs the app's frontend in a native window and manages its services (backend, database, workers). \
Start with `app_info`. Use `runtime_status` and `logs_read` to diagnose problems, `service_restart` after changing backend code, \
and `ui_snapshot` to see the UI: it returns element refs (e.g. e12) for `ui_click` and `ui_fill`. \
Prefer application capabilities (`capabilities_list`, `app_invoke` / `cap_*` tools) over UI automation when they exist. \
Some actions may require human approval or may not be permitted in the current mode.";

fn rpc_result(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn rpc_error(id: &Value, code: i64, message: impl Into<String>) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message.into() } })
}

/// Convert a tool definition into an MCP tool description.
pub fn mcp_tool(t: &ToolDef) -> Value {
    let read_only = t.permission == Permission::Read;
    let destructive = matches!(t.permission, Permission::Code | Permission::System | Permission::Services);
    json!({
        "name": t.name,
        "description": format!("{} [permission: {}]", t.description, t.permission),
        "inputSchema": t.input_schema,
        "annotations": {
            "readOnlyHint": read_only,
            "destructiveHint": destructive,
            "openWorldHint": false
        }
    })
}

/// Convert a tool outcome into an MCP `CallToolResult`.
pub fn mcp_call_result(outcome: &ToolOutcome) -> Value {
    let text = match &outcome.result {
        Value::String(s) => s.clone(),
        v if !outcome.ok => {
            v.get("error").and_then(|e| e.as_str()).map(str::to_string).unwrap_or_else(|| v.to_string())
        }
        v => serde_json::to_string_pretty(v).unwrap_or_default(),
    };
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": !outcome.ok,
    });
    if outcome.ok && outcome.result.is_object() {
        result["structuredContent"] = outcome.result.clone();
    }
    result
}

/// Handle one JSON-RPC message (or a batch). Returns `None` for notifications.
pub async fn handle_message(provider: &dyn ToolProvider, msg: Value, ctx: CallContext) -> Option<Value> {
    if let Value::Array(items) = msg {
        let mut out = Vec::new();
        for item in items {
            if let Some(r) = Box::pin(handle_message(provider, item, ctx.clone())).await {
                out.push(r);
            }
        }
        return if out.is_empty() { None } else { Some(Value::Array(out)) };
    }
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(json!({}));

    // notifications and responses have no id
    let id = id?;
    if method.is_empty() {
        return Some(rpc_error(&id, -32600, "invalid request"));
    }

    let response = match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(|v| v.as_str()).unwrap_or("");
            let version = if SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
                requested
            } else {
                SUPPORTED_PROTOCOL_VERSIONS[0]
            };
            rpc_result(
                &id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": true }, "resources": {} },
                    "serverInfo": { "name": "openshell", "title": "OpenShell", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": provider.instructions(),
                }),
            )
        }
        "ping" => rpc_result(&id, json!({})),
        "tools/list" => {
            let tools: Vec<Value> = provider.list_tools().iter().map(mcp_tool).collect();
            rpc_result(&id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let Some(name) = params.get("name").and_then(|n| n.as_str()) else {
                return Some(rpc_error(&id, -32602, "missing tool name"));
            };
            if !provider.list_tools().iter().any(|t| t.name == name) {
                return Some(rpc_error(&id, -32602, format!("unknown tool `{name}`")));
            }
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let outcome = provider.call_tool(name, args, ctx).await;
            rpc_result(&id, mcp_call_result(&outcome))
        }
        "resources/list" => rpc_result(&id, json!({ "resources": provider.resources() })),
        "resources/read" => {
            let Some(uri) = params.get("uri").and_then(|u| u.as_str()) else {
                return Some(rpc_error(&id, -32602, "missing resource uri"));
            };
            match provider.read_resource(uri).await {
                Ok(contents) => rpc_result(&id, json!({ "contents": contents })),
                Err(e) => rpc_error(&id, -32002, e),
            }
        }
        "resources/templates/list" => rpc_result(&id, json!({ "resourceTemplates": [] })),
        "prompts/list" => rpc_result(&id, json!({ "prompts": [] })),
        other => rpc_error(&id, -32601, format!("method not found: {other}")),
    };
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::builtin_tools;

    struct Echo;
    impl ToolProvider for Echo {
        fn list_tools(&self) -> Vec<ToolDef> {
            builtin_tools()
        }
        fn call_tool<'a>(
            &'a self,
            name: &'a str,
            args: Value,
            ctx: CallContext,
        ) -> BoxFuture<'a, ToolOutcome> {
            Box::pin(async move {
                if name == "fs_write" {
                    ToolOutcome::error("denied")
                } else {
                    ToolOutcome::ok(json!({ "tool": name, "args": args, "agent": ctx.agent }))
                }
            })
        }
    }

    fn ctx() -> CallContext {
        CallContext { agent: "test".into(), channel: "mcp".into() }
    }

    #[tokio::test]
    async fn protocol_flow() {
        let init = handle_message(
            &Echo,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}),
            ctx(),
        )
        .await
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle_message(&Echo, json!({"jsonrpc":"2.0","method":"notifications/initialized"}), ctx())
            .await
            .is_none());

        let list = handle_message(&Echo, json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}), ctx())
            .await
            .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "ui_snapshot"));

        let call = handle_message(
            &Echo,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"runtime_status","arguments":{}}}),
            ctx(),
        )
        .await
        .unwrap();
        assert_eq!(call["result"]["isError"], false);
        assert_eq!(call["result"]["structuredContent"]["agent"], "test");

        let denied = handle_message(
            &Echo,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"fs_write","arguments":{}}}),
            ctx(),
        )
        .await
        .unwrap();
        assert_eq!(denied["result"]["isError"], true);
        assert_eq!(denied["result"]["content"][0]["text"], "denied");

        let res = handle_message(
            &Echo,
            json!({"jsonrpc":"2.0","id":6,"method":"resources/read","params":{"uri":"x://y"}}),
            ctx(),
        )
        .await
        .unwrap();
        assert_eq!(res["error"]["code"], -32002);
        assert_eq!(init["result"]["capabilities"]["tools"]["listChanged"], true);

        let unknown =
            handle_message(&Echo, json!({"jsonrpc":"2.0","id":5,"method":"nope"}), ctx()).await.unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
    }
}
