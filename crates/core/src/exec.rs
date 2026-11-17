//! Agent tool execution: permission checks, approvals, audit and the actual
//! implementation of every built-in tool.

use crate::{fs, Shell};
use openshell_agent::audit::{AuditEntry, AuditOutcome};
use openshell_agent::mcp::{BoxFuture, CallContext, ToolProvider};
use openshell_agent::policy::{Decision, Policy};
use openshell_agent::tools::{builtin_tools, ToolDef, CAPABILITY_TOOL_PREFIX};
use openshell_agent::{providers, ToolOutcome};
use openshell_config::{CapabilityConfig, Permission};
use openshell_runtime::process::build_command;
use openshell_runtime::LogQuery;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

/// A capability from the config or registered by a page.
#[derive(Debug, Clone)]
pub(crate) enum Capability {
    Config(CapabilityConfig),
    Page {
        page: String,
        action: openshell_agent::bridge::PageAction,
    },
    /// Imported from a service's OpenAPI document.
    OpenApi(crate::openapi::ApiOperation),
}

impl Capability {
    fn name(&self) -> &str {
        match self {
            Capability::Config(c) => &c.name,
            Capability::Page { action, .. } => &action.name,
            Capability::OpenApi(o) => &o.name,
        }
    }
    pub(crate) fn permission(&self) -> Permission {
        match self {
            Capability::Config(c) => c.permission,
            Capability::Page { action, .. } => action.permission,
            Capability::OpenApi(o) => o.permission,
        }
    }
    pub(crate) fn description(&self) -> String {
        match self {
            Capability::Config(c) => c.description.clone(),
            Capability::Page { action, .. } => action.description.clone(),
            Capability::OpenApi(o) => o.description.clone(),
        }
    }
    pub(crate) fn input_schema(&self) -> Value {
        match self {
            Capability::Config(c) => c.input_schema.clone(),
            Capability::Page { action, .. } => action.input_schema.clone(),
            Capability::OpenApi(o) => Some(o.input_schema.clone()),
        }
        .unwrap_or_else(|| json!({ "type": "object", "properties": {} }))
    }
    fn to_json(&self) -> Value {
        match self {
            Capability::Config(c) => json!({
                "name": c.name,
                "description": c.description,
                "permission": c.permission,
                "input_schema": c.input_schema,
                "source": "config",
                "target": if let Some(h) = &c.http {
                    format!("{} {}{}", h.method.to_uppercase(), h.service.clone().map(|s| format!("{s}:")).unwrap_or_default(), if h.path.is_empty() { h.url.clone().unwrap_or_default() } else { h.path.clone() })
                } else {
                    format!("task {}", c.task.clone().unwrap_or_default())
                },
            }),
            Capability::Page { page, action } => json!({
                "name": action.name,
                "description": action.description,
                "permission": action.permission,
                "input_schema": action.input_schema,
                "source": "frontend",
                "page": page,
            }),
            Capability::OpenApi(o) => json!({
                "name": o.name,
                "description": o.description,
                "permission": o.permission,
                "input_schema": o.input_schema,
                "source": "openapi",
                "target": format!("{} {}:{}", o.method, o.service, o.path),
            }),
        }
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

fn arg_u64(args: &Value, key: &str) -> Option<u64> {
    args.get(key).and_then(|v| v.as_u64())
}

fn require<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    arg_str(args, key).ok_or_else(|| format!("missing argument `{key}`"))
}

impl Shell {
    pub(crate) fn capabilities(&self) -> Vec<Capability> {
        let mut caps: Vec<Capability> = self
            .inner
            .project
            .config
            .capabilities
            .iter()
            .cloned()
            .map(Capability::Config)
            .collect();
        for op in self.inner.imported.read().unwrap().iter() {
            if !caps.iter().any(|c| c.name() == op.name) {
                caps.push(Capability::OpenApi(op.clone()));
            }
        }
        for (page, action) in self.inner.hub.actions() {
            if !caps.iter().any(|c| c.name() == action.name) {
                caps.push(Capability::Page { page, action });
            }
        }
        caps
    }

    pub(crate) fn capability(&self, name: &str) -> Option<Capability> {
        self.capabilities().into_iter().find(|c| c.name() == name)
    }

    /// Built-in tools plus one `cap_<name>` tool per application capability.
    pub fn tools(&self) -> Vec<ToolDef> {
        let mut tools = builtin_tools();
        for cap in self.capabilities() {
            tools.push(ToolDef {
                name: format!("{CAPABILITY_TOOL_PREFIX}{}", cap.name()),
                description: format!("Application capability: {}", cap.description()),
                permission: cap.permission(),
                input_schema: cap.input_schema(),
            });
        }
        tools
    }

    /// Permission required for a call (capability calls use the capability's own scope).
    fn required_permission(&self, name: &str, args: &Value) -> Result<Permission, String> {
        if name == "app_invoke" {
            let cap = require(args, "name")?;
            return self
                .capability(cap)
                .map(|c| c.permission())
                .ok_or_else(|| format!("unknown capability `{cap}` (see capabilities_list)"));
        }
        self.tools()
            .into_iter()
            .find(|t| t.name == name)
            .map(|t| t.permission)
            .ok_or_else(|| format!("unknown tool `{name}`"))
    }

    /// Execute a tool on behalf of a caller with the given policy.
    pub async fn call_tool(&self, name: &str, args: Value, ctx: CallContext, policy: &Policy) -> ToolOutcome {
        let started = Instant::now();
        let args = if args.is_null() { json!({}) } else { args };
        let permission = match self.required_permission(name, &args) {
            Ok(p) => p,
            Err(e) => return ToolOutcome::error(e),
        };
        let audit = |outcome: AuditOutcome, message: Option<String>, approved: bool| {
            self.inner.audit.record(AuditEntry {
                ts: chrono::Utc::now(),
                agent: ctx.agent.clone(),
                channel: ctx.channel.clone(),
                tool: name.to_string(),
                permission,
                args: args.clone(),
                outcome,
                message,
                approved_by_human: approved,
                duration_ms: started.elapsed().as_millis() as u64,
            })
        };

        let mut approved = false;
        match policy.check(permission) {
            Decision::Deny => {
                let msg = policy.denial_message(permission);
                audit(AuditOutcome::Denied, Some(msg.clone()), false);
                return ToolOutcome::error(msg);
            }
            Decision::NeedsApproval => {
                let summary = summarize(name, &args);
                let timeout = Duration::from_millis(self.inner.project.config.agents.approval_timeout_ms);
                let ok = self
                    .inner
                    .approvals
                    .request(&ctx.agent, name, permission, summary, args.clone(), timeout)
                    .await;
                if !ok {
                    let msg = "the user did not approve this action".to_string();
                    audit(AuditOutcome::Rejected, Some(msg.clone()), false);
                    return ToolOutcome::error(msg);
                }
                approved = true;
            }
            Decision::Allow => {}
        }

        let result = self.execute(name, &args).await;
        match result {
            Ok(v) => {
                audit(AuditOutcome::Ok, None, approved);
                ToolOutcome::ok(v)
            }
            Err(e) => {
                audit(AuditOutcome::Error, Some(e.clone()), approved);
                ToolOutcome::error(e)
            }
        }
    }

    async fn ui(&self, args: &Value, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        self.inner.hub.call(arg_str(args, "page"), method, params, timeout).await
    }

    async fn execute(&self, name: &str, args: &Value) -> Result<Value, String> {
        let rt = &self.inner.runtime;
        let root = self.inner.project.root.clone();
        let ui_timeout = Duration::from_secs(15);
        match name {
            "app_info" => Ok(self.app_info()),
            "runtime_status" => match arg_str(args, "service") {
                Some(s) => rt
                    .service_status(s)
                    .map(|st| json!(st))
                    .ok_or_else(|| format!("unknown service `{s}`")),
                None => Ok(json!({ "services": rt.status() })),
            },
            "logs_read" => {
                let q = LogQuery {
                    service: arg_str(args, "service").map(str::to_string),
                    since: arg_u64(args, "since"),
                    limit: Some(arg_u64(args, "limit").unwrap_or(100).min(1000) as usize),
                    contains: arg_str(args, "contains").map(str::to_string),
                    errors_only: args.get("errors_only").and_then(|v| v.as_bool()).unwrap_or(false),
                };
                let lines = rt.logs().query(&q);
                Ok(json!({ "lines": lines, "last_seq": rt.logs().last_seq() }))
            }
            "capabilities_list" => Ok(json!({
                "capabilities": self.capabilities().iter().map(|c| c.to_json()).collect::<Vec<_>>()
            })),
            "config_get" => serde_json::to_value(&self.inner.project.config).map_err(|e| e.to_string()),
            "providers_list" => {
                let a = &self.inner.project.config.agents;
                Ok(json!({ "providers": providers::discover(&a.providers, a.detect_local_providers).await }))
            }
            "audit_read" => Ok(json!({
                "entries": self.inner.audit.recent(arg_u64(args, "limit").unwrap_or(50) as usize)
            })),
            "ui_pages" => Ok(json!({ "pages": self.inner.hub.pages() })),
            "ui_snapshot" => self.ui(args, "snapshot", args.clone(), ui_timeout).await,
            "ui_click" => self.ui(args, "click", args.clone(), ui_timeout).await,
            "ui_fill" => self.ui(args, "fill", args.clone(), ui_timeout).await,
            "ui_press" => self.ui(args, "press", args.clone(), ui_timeout).await,
            "ui_read_text" => self.ui(args, "read_text", args.clone(), ui_timeout).await,
            "ui_wait_for" => {
                let t = arg_u64(args, "timeout_ms").unwrap_or(5000);
                self.ui(args, "wait_for", args.clone(), Duration::from_millis(t + 2000)).await
            }
            "ui_navigate" => self.ui(args, "navigate", args.clone(), ui_timeout).await,
            "ui_reload" => {
                if self.inner.hub.pages().is_empty() {
                    // no bridge (e.g. the page failed to load): let the host reload the WebView
                    return self.inner.host.reload_frontend().map(|_| json!({ "reloading": true }));
                }
                self.ui(args, "reload", json!({}), ui_timeout).await
            }
            "ui_eval" => self.ui(args, "eval", args.clone(), ui_timeout).await,
            "window_control" => {
                let action = require(args, "action")?;
                self.inner.host.window(action).map(|_| json!({ "ok": true, "action": action }))
            }
            "notify" => {
                let title = require(args, "title")?;
                let body = arg_str(args, "body").unwrap_or("");
                self.inner.host.notify(title, body).map(|_| json!({ "ok": true }))
            }
            "service_start" | "service_restart" => {
                let svc = require(args, "name")?;
                if name == "service_start" {
                    rt.start(svc).map_err(|e| e.to_string())?;
                } else {
                    rt.restart(svc).map_err(|e| e.to_string())?;
                }
                let since = rt.logs().last_seq();
                // give the supervisor a moment to pick up the change
                tokio::time::sleep(Duration::from_millis(300)).await;
                match rt.wait_ready(svc, Duration::from_secs(45)).await {
                    Ok(st) => Ok(json!({ "ok": true, "status": st })),
                    Err(e) => {
                        let logs = rt.logs().query(&LogQuery {
                            service: Some(svc.to_string()),
                            since: Some(since),
                            limit: Some(30),
                            ..Default::default()
                        });
                        let text: Vec<String> = logs.into_iter().map(|l| l.text).collect();
                        Err(format!("{e}\nrecent logs:\n{}", text.join("\n")))
                    }
                }
            }
            "service_stop" => {
                let svc = require(args, "name")?;
                rt.stop(svc).map_err(|e| e.to_string())?;
                rt.wait_stopped(svc, Duration::from_secs(30)).await.map_err(|e| e.to_string())?;
                Ok(json!({ "ok": true, "status": rt.service_status(svc) }))
            }
            "fs_list" => fs::list(
                &root,
                arg_str(args, "path").unwrap_or(""),
                args.get("recursive").and_then(|v| v.as_bool()).unwrap_or(false),
                arg_u64(args, "max_entries").unwrap_or(500) as usize,
            ),
            "fs_read" => fs::read(&root, require(args, "path")?, arg_u64(args, "max_bytes").unwrap_or(200_000) as usize),
            "fs_search" => fs::search(
                &root,
                arg_str(args, "path").unwrap_or(""),
                require(args, "query")?,
                arg_u64(args, "max_results").unwrap_or(100) as usize,
            ),
            "fs_write" => {
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("missing argument `content`")?;
                fs::write(&root, require(args, "path")?, content)
            }
            "task_run" => self.run_task(require(args, "name")?).await,
            "app_invoke" => {
                let cap = require(args, "name")?;
                self.invoke_capability(cap, args.get("input").cloned().unwrap_or(json!({}))).await
            }
            other if other.starts_with(CAPABILITY_TOOL_PREFIX) => {
                self.invoke_capability(&other[CAPABILITY_TOOL_PREFIX.len()..], args.clone()).await
            }
            other => Err(format!("unknown tool `{other}`")),
        }
    }

    fn app_info(&self) -> Value {
        let cfg = &self.inner.project.config;
        let policy = &self.inner.agent_policy;
        json!({
            "app": {
                "name": cfg.app.name,
                "version": cfg.app.version,
                "description": cfg.app.description,
            },
            "mode": self.inner.mode,
            "host": self.inner.host.kind(),
            "project_root": self.inner.project.root,
            "frontend_url": self.inner.runtime.frontend_url(),
            "services": self.inner.runtime.status().iter().map(|s| json!({
                "name": s.name, "state": s.state, "url": s.url, "category": s.category
            })).collect::<Vec<_>>(),
            "ui_pages_connected": self.inner.hub.pages().len(),
            "permissions": {
                "granted": policy.granted,
                "approval_required": policy.approval_required.iter().filter(|p| policy.granted.contains(p)).collect::<Vec<_>>(),
            },
            "capabilities": self.capabilities().iter().map(|c| c.name().to_string()).collect::<Vec<_>>(),
            "tasks": cfg.tasks.keys().collect::<Vec<_>>(),
            "hints": [
                "Source code lives in the project root; edit it with your own tools or fs_write.",
                "Services with `watch` paths restart automatically when their files change; otherwise call service_restart.",
                "Dev-server frontends hot-reload by themselves; use ui_reload for static frontends.",
                "Use ui_snapshot to see the UI and get element refs for ui_click / ui_fill.",
            ],
        })
    }

    async fn run_task(&self, name: &str) -> Result<Value, String> {
        let spec = self
            .inner
            .project
            .config
            .tasks
            .get(name)
            .ok_or_else(|| format!("unknown task `{name}`"))?
            .normalized();
        let cwd = spec
            .cwd
            .as_ref()
            .map(|c| self.inner.project.resolve_path(c))
            .unwrap_or_else(|| self.inner.project.root.clone());
        let mut cmd = build_command(&spec.command, &[], &cwd, &spec.env).map_err(|e| e.to_string())?;
        let started = Instant::now();
        let child = cmd.spawn().map_err(|e| format!("cannot run task `{name}`: {e}"))?;
        let output = tokio::time::timeout(Duration::from_millis(spec.timeout_ms), child.wait_with_output())
            .await
            .map_err(|_| format!("task `{name}` timed out after {}s", spec.timeout_ms / 1000))?
            .map_err(|e| e.to_string())?;
        let tail = |b: &[u8]| {
            let s = openshell_runtime::logs::strip_ansi(&String::from_utf8_lossy(b));
            let n = s.chars().count();
            if n > 20_000 {
                s.chars().skip(n - 20_000).collect()
            } else {
                s
            }
        };
        Ok(json!({
            "task": name,
            "exit_code": output.status.code(),
            "success": output.status.success(),
            "stdout": tail(&output.stdout),
            "stderr": tail(&output.stderr),
            "duration_ms": started.elapsed().as_millis() as u64,
        }))
    }

    async fn invoke_capability(&self, name: &str, input: Value) -> Result<Value, String> {
        let cap = self
            .capability(name)
            .ok_or_else(|| format!("unknown capability `{name}` (see capabilities_list)"))?;
        match cap {
            Capability::OpenApi(op) => {
                let (url, query, body) = op.request(&input);
                let method = reqwest::Method::from_bytes(op.method.as_bytes()).map_err(|e| e.to_string())?;
                let client = reqwest::Client::builder().no_proxy().build().map_err(|e| e.to_string())?;
                let mut req = client.request(method, &url).query(&query).timeout(Duration::from_secs(60));
                if let Some(b) = body {
                    req = req.json(&b);
                }
                let resp = req.send().await.map_err(|e| format!("{} {url}: {e}", op.method))?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                let body: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
                if status.is_success() {
                    Ok(body)
                } else {
                    Err(format!("{} {url} returned {status}: {body}", op.method))
                }
            }
            Capability::Page { page, .. } => {
                self.inner
                    .hub
                    .call(Some(&page), "invoke_action", json!({ "name": name, "input": input }), Duration::from_secs(30))
                    .await
            }
            Capability::Config(c) => {
                if let Some(task) = &c.task {
                    return self.run_task(task).await;
                }
                let h = c.http.as_ref().ok_or("capability has no target")?;
                let base = match (&h.service, &h.url) {
                    (Some(s), _) => self
                        .inner
                        .runtime
                        .service_url(s)
                        .ok_or_else(|| format!("service `{s}` has no url/port"))?,
                    (None, Some(u)) => u.clone(),
                    (None, None) => return Err("capability has no url".into()),
                };
                let url = format!("{}{}", base.trim_end_matches('/'), h.path);
                let method = reqwest::Method::from_bytes(h.method.to_uppercase().as_bytes()).map_err(|e| e.to_string())?;
                let client = reqwest::Client::builder().no_proxy().build().map_err(|e| e.to_string())?;
                let mut req = client.request(method.clone(), &url).timeout(Duration::from_secs(60));
                if method == reqwest::Method::GET || method == reqwest::Method::DELETE {
                    if let Some(obj) = input.as_object() {
                        let q: Vec<(String, String)> = obj
                            .iter()
                            .map(|(k, v)| (k.clone(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))
                            .collect();
                        req = req.query(&q);
                    }
                } else {
                    req = req.json(&input);
                }
                let resp = req.send().await.map_err(|e| format!("{} {url}: {e}", h.method))?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                let body: Value = serde_json::from_str(&text).unwrap_or(Value::String(text));
                if status.is_success() {
                    Ok(body)
                } else {
                    Err(format!("{} {url} returned {status}: {body}", h.method))
                }
            }
        }
    }
}

/// Human readable summary for approval dialogs.
fn summarize(name: &str, args: &Value) -> String {
    match name {
        "fs_write" => format!(
            "write {} ({} bytes)",
            arg_str(args, "path").unwrap_or("?"),
            args.get("content").and_then(|c| c.as_str()).map(|c| c.len()).unwrap_or(0)
        ),
        "task_run" => format!("run task `{}`", arg_str(args, "name").unwrap_or("?")),
        "ui_eval" => format!(
            "run JavaScript in the app: {}",
            arg_str(args, "script").unwrap_or("").chars().take(120).collect::<String>()
        ),
        "service_start" | "service_stop" | "service_restart" => format!(
            "{} service `{}`",
            name.trim_start_matches("service_"),
            arg_str(args, "name").unwrap_or("?")
        ),
        "app_invoke" => format!("invoke capability `{}`", arg_str(args, "name").unwrap_or("?")),
        _ => {
            let a = args.to_string();
            format!("{name} {}", a.chars().take(160).collect::<String>())
        }
    }
}

/// Exposes the shell's tools to MCP with a fixed policy (agents).
pub struct AgentTools(pub Shell);

/// The "use" profile: only the application's own functions (capabilities from the config,
/// OpenAPI imports and frontend actions) under their plain names, plus `app_status`.
/// Agents that want to *use* the application – not develop it – get a small, domain-specific
/// tool list and never need UI automation.
pub struct AppTools(pub Shell);

impl ToolProvider for AppTools {
    fn list_tools(&self) -> Vec<ToolDef> {
        let mut tools = vec![ToolDef {
            name: "app_status".into(),
            description: "Whether the application and its services are running and healthy.".into(),
            permission: Permission::Read,
            input_schema: json!({ "type": "object", "properties": {} }),
        }];
        for cap in self.0.capabilities() {
            if cap.name() == "app_status" {
                continue;
            }
            tools.push(ToolDef {
                name: cap.name().to_string(),
                description: cap.description(),
                permission: cap.permission(),
                input_schema: cap.input_schema(),
            });
        }
        tools
    }

    fn call_tool<'a>(&'a self, name: &'a str, args: Value, ctx: CallContext) -> BoxFuture<'a, ToolOutcome> {
        Box::pin(async move {
            let policy = self.0.inner.agent_policy.clone();
            if name == "app_status" {
                let services: Vec<Value> = self
                    .0
                    .runtime()
                    .status()
                    .into_iter()
                    .map(|s| json!({ "name": s.name, "state": s.state, "error": s.last_error }))
                    .collect();
                let healthy = services.iter().all(|s| s["state"] == "running");
                return ToolOutcome::ok(json!({ "app": self.0.runtime().app_name(), "healthy": healthy, "services": services }));
            }
            self.0
                .call_tool(&format!("{CAPABILITY_TOOL_PREFIX}{name}"), args, ctx, &policy)
                .await
        })
    }

    fn instructions(&self) -> String {
        let cfg = &self.0.project().config;
        format!(
            "You are connected to the desktop application \"{}\"{}. The tools operate the application directly \
             through its own API, so no UI automation is needed. Read-only tools are safe to call; other tools change \
             data in the application and may require the user's approval.",
            cfg.app.name,
            cfg.app.description.as_ref().map(|d| format!(" ({d})")).unwrap_or_default()
        )
    }
}

impl ToolProvider for AgentTools {
    fn list_tools(&self) -> Vec<ToolDef> {
        self.0.tools()
    }
    fn call_tool<'a>(&'a self, name: &'a str, args: Value, ctx: CallContext) -> BoxFuture<'a, ToolOutcome> {
        Box::pin(async move {
            let policy = self.0.inner.agent_policy.clone();
            self.0.call_tool(name, args, ctx, &policy).await
        })
    }
}
