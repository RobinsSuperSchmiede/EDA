//! Catalogue of built-in agent tools. Every tool declares the permission scope
//! it needs and a JSON Schema for its input, so it can be exposed 1:1 over MCP
//! and the HTTP API.

use openshell_config::Permission;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub permission: Permission,
    pub input_schema: Value,
}

fn object(props: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    })
}

fn tool(name: &str, permission: Permission, description: &str, schema: Value) -> ToolDef {
    ToolDef { name: name.into(), description: description.into(), permission, input_schema: schema }
}

fn page_prop() -> Value {
    json!({ "type": "string", "description": "Target page id from ui_pages (default: the main window)" })
}

fn target_props() -> Value {
    json!({
        "ref": { "type": "string", "description": "Element ref from ui_snapshot, e.g. \"e12\"" },
        "selector": { "type": "string", "description": "CSS selector" },
        "text": { "type": "string", "description": "Visible text / accessible name of the element" },
        "page": page_prop(),
    })
}

fn merge(mut a: Value, b: Value) -> Value {
    if let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) {
        for (k, v) in b {
            a.insert(k.clone(), v.clone());
        }
    }
    a
}

/// All built-in tools.
pub fn builtin_tools() -> Vec<ToolDef> {
    use Permission::*;
    let service_name = || {
        object(
            json!({ "name": { "type": "string", "description": "Service name (see runtime_status)" } }),
            &["name"],
        )
    };
    vec![
        // --- read ----------------------------------------------------------
        tool("app_info", Read,
            "Describe the application: name, mode, frontend URL, services, granted permissions, available capabilities and how to work with it. Call this first.",
            object(json!({}), &[])),
        tool("runtime_status", Read,
            "Status of all managed services (frontend, backends, databases, workers): state, pid, port, url, restarts, last error.",
            object(json!({ "service": { "type": "string", "description": "Only this service" } }), &[])),
        tool("logs_read", Read,
            "Read recent log lines (stdout/stderr of services and runtime messages). Use `since` with the last seq you saw to get only new lines.",
            object(json!({
                "service": { "type": "string" },
                "since": { "type": "integer", "description": "Only lines with seq greater than this" },
                "limit": { "type": "integer", "description": "Max lines (default 100, max 1000)" },
                "contains": { "type": "string", "description": "Case-insensitive filter" },
                "errors_only": { "type": "boolean", "description": "Only stderr and runtime messages" }
            }), &[])),
        tool("capabilities_list", Read,
            "List the functions this application exposes to agents (declared in openshell.json or registered by the frontend). Invoke them with app_invoke.",
            object(json!({}), &[])),
        tool("config_get", Read,
            "The effective openshell.json configuration of the application.",
            object(json!({}), &[])),
        tool("providers_list", Read,
            "AI providers known to the application and local providers detected on this machine (Ollama, LM Studio) with their models.",
            object(json!({}), &[])),
        tool("audit_read", Read,
            "Recent agent actions from the audit log.",
            object(json!({ "limit": { "type": "integer" } }), &[])),
        // --- ui ------------------------------------------------------------
        tool("ui_pages", Ui,
            "List application pages connected to the UI bridge (the desktop window and browser tabs).",
            object(json!({}), &[])),
        tool("ui_snapshot", Ui,
            "Semantic snapshot of the current page: title, URL, headings, visible text and all interactive elements with refs (e.g. \"e12\") that can be used with ui_click / ui_fill.",
            object(json!({
                "selector": { "type": "string", "description": "Only snapshot inside this element" },
                "max_text": { "type": "integer", "description": "Max characters of page text (default 3000)" },
                "page": page_prop()
            }), &[])),
        tool("ui_click", Ui,
            "Click an element identified by ref, CSS selector or visible text.",
            object(target_props(), &[])),
        tool("ui_fill", Ui,
            "Set the value of an input, textarea, select, checkbox or contenteditable element (fires input/change events, works with React/Vue/Svelte).",
            object(merge(target_props(), json!({
                "label": { "type": "string", "description": "Find the field by its label text" },
                "value": { "description": "New value (string, number or boolean for checkboxes)" },
                "submit": { "type": "boolean", "description": "Submit the surrounding form afterwards" }
            })), &["value"])),
        tool("ui_press", Ui,
            "Press a key (e.g. \"Enter\", \"Escape\", \"ArrowDown\") on an element or the focused element.",
            object(merge(target_props(), json!({ "key": { "type": "string" } })), &["key"])),
        tool("ui_read_text", Ui,
            "Read the visible text of the page or of one element.",
            object(target_props(), &[])),
        tool("ui_wait_for", Ui,
            "Wait until an element or text appears (or disappears with gone=true).",
            object(json!({
                "selector": { "type": "string" },
                "text": { "type": "string" },
                "gone": { "type": "boolean" },
                "timeout_ms": { "type": "integer", "description": "Default 5000" },
                "page": page_prop()
            }), &[])),
        tool("ui_navigate", Ui,
            "Navigate the application to a path (e.g. \"/settings\") or URL.",
            object(json!({ "path": { "type": "string" }, "url": { "type": "string" }, "page": page_prop() }), &[])),
        tool("ui_reload", Ui,
            "Reload the application frontend (e.g. after changing frontend code).",
            object(json!({ "page": page_prop() }), &[])),
        tool("window_control", Ui,
            "Control the native desktop window.",
            object(json!({ "action": { "type": "string", "enum": ["show", "hide", "focus", "minimize", "maximize", "unmaximize"] } }), &["action"])),
        tool("notify", Ui,
            "Show a native desktop notification to the user.",
            object(json!({ "title": { "type": "string" }, "body": { "type": "string" } }), &["title"])),
        // --- actions -------------------------------------------------------
        tool("app_invoke", Actions,
            "Invoke an application capability by name (see capabilities_list). The capability's own permission is checked as well.",
            object(json!({ "name": { "type": "string" }, "input": { "type": "object", "description": "Input matching the capability's input_schema" } }), &["name"])),
        // --- services ------------------------------------------------------
        tool("service_start", Services, "Start a service (and its dependencies).", service_name()),
        tool("service_stop", Services, "Stop a service.", service_name()),
        tool("service_restart", Services, "Restart a service, e.g. after changing backend code.", service_name()),
        // --- files ---------------------------------------------------------
        tool("fs_list", Files,
            "List files of the project (paths are relative to the project root).",
            object(json!({
                "path": { "type": "string", "description": "Directory (default: project root)" },
                "recursive": { "type": "boolean" },
                "max_entries": { "type": "integer", "description": "Default 500" }
            }), &[])),
        tool("fs_read", Files,
            "Read a text file of the project.",
            object(json!({ "path": { "type": "string" }, "max_bytes": { "type": "integer", "description": "Default 200000" } }), &["path"])),
        tool("fs_search", Files,
            "Search project files for a text (case-insensitive). Returns file, line number and line.",
            object(json!({
                "query": { "type": "string" },
                "path": { "type": "string", "description": "Directory to search (default: project root)" },
                "max_results": { "type": "integer", "description": "Default 100" }
            }), &["query"])),
        // --- code ----------------------------------------------------------
        tool("fs_write", Code,
            "Create or overwrite a project file. Changes to watched paths restart services / reload the frontend automatically.",
            object(json!({ "path": { "type": "string" }, "content": { "type": "string" } }), &["path", "content"])),
        // --- system --------------------------------------------------------
        tool("task_run", System,
            "Run a task declared in openshell.json (e.g. tests, linting, migrations) and return its output.",
            object(json!({ "name": { "type": "string" } }), &["name"])),
        tool("ui_eval", System,
            "Evaluate JavaScript in the application page and return the (JSON-serialisable) result.",
            object(json!({ "script": { "type": "string" }, "page": page_prop() }), &["script"])),
    ]
}

/// Look up a built-in tool by name.
pub fn find_builtin(name: &str) -> Option<ToolDef> {
    builtin_tools().into_iter().find(|t| t.name == name)
}

/// Prefix of the dynamic tools generated for application capabilities.
pub const CAPABILITY_TOOL_PREFIX: &str = "cap_";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_are_unique_and_valid() {
        let tools = builtin_tools();
        let mut names: Vec<_> = tools.iter().map(|t| t.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), tools.len());
        for t in &tools {
            assert!(t.name.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
            assert_eq!(t.input_schema["type"], "object");
        }
    }
}
