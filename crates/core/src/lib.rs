//! # openshell-core
//!
//! Wires the [`Runtime`](openshell_runtime::Runtime) and the Agent Interaction
//! Layer together and serves the local **control API** (HTTP + WebSocket + MCP)
//! that the CLI, the frontend SDK and AI agents talk to.
//!
//! Both front-ends of OpenShell use this crate:
//! * the desktop app (Tauri) launches a [`Shell`] with a desktop [`Host`],
//! * `openshell start --headless` launches it with a [`HeadlessHost`].

mod exec;
mod fs;
pub mod headless;
pub mod openapi;
mod server;
pub mod state;
pub mod stdio;

pub use exec::{AgentTools, AppTools};

pub use openshell_agent as agent;
pub use openshell_config as config;
pub use openshell_runtime as runtime;

use anyhow::{Context, Result};
use openshell_agent::approvals::{ApprovalEvent, ApprovalRequest, Approvals};
use openshell_agent::audit::AuditLog;
use openshell_agent::bridge::UiHub;
use openshell_agent::policy::Policy;
use openshell_config::{Mode, Project};
use openshell_runtime::Runtime;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Notify;

/// The environment the shell runs in (desktop window or headless).
/// Implemented by the desktop app; [`HeadlessHost`] is used by the CLI.
pub trait Host: Send + Sync + 'static {
    /// `desktop` or `headless`.
    fn kind(&self) -> &'static str;
    /// `show`, `hide`, `focus`, `minimize`, `maximize`, `unmaximize`.
    fn window(&self, action: &str) -> Result<(), String> {
        Err(format!("window action `{action}` is not available in {} mode", self.kind()))
    }
    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        tracing::info!("notification: {title}: {body}");
        Ok(())
    }
    /// Reload the frontend without the UI bridge (e.g. when the page is broken).
    fn reload_frontend(&self) -> Result<(), String> {
        Err(format!("not available in {} mode", self.kind()))
    }
    /// A critical agent action waits for a human decision.
    fn approval_requested(&self, _request: ApprovalRequest, _approvals: Arc<Approvals>) {}
    /// Someone asked the shell to exit (`openshell stop`).
    fn shutdown_requested(&self) {}
}

/// Host without a window (headless mode / browser mode).
#[derive(Default)]
pub struct HeadlessHost {
    pub shutdown: Arc<Notify>,
}

impl Host for HeadlessHost {
    fn kind(&self) -> &'static str {
        "headless"
    }
    fn shutdown_requested(&self) {
        self.shutdown.notify_waiters();
        self.shutdown.notify_one();
    }
}

pub(crate) struct ShellInner {
    pub project: Project,
    pub mode: Mode,
    pub runtime: Runtime,
    pub hub: UiHub,
    pub approvals: Arc<Approvals>,
    pub audit: AuditLog,
    pub agent_policy: Policy,
    pub sdk_policy: Policy,
    pub admin_token: String,
    pub app_token: String,
    pub host: Arc<dyn Host>,
    pub control_url: String,
    pub allowed_origins: Vec<String>,
    /// Operations imported from services' OpenAPI documents.
    pub imported: std::sync::RwLock<Vec<openapi::ApiOperation>>,
}

/// A running OpenShell instance: runtime + control API + agent layer.
#[derive(Clone)]
pub struct Shell {
    pub(crate) inner: Arc<ShellInner>,
}

fn token() -> String {
    format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple())
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    Some(format!("{scheme}://{host}"))
}

impl Shell {
    /// Create the runtime, bind the control API and write `.openshell/runtime.json`.
    /// Services are started with [`Shell::start`].
    pub async fn launch(project: Project, mode: Mode, host: Arc<dyn Host>) -> Result<Shell> {
        let runtime = Runtime::new(&project, mode)?;
        let state_dir = project.state_dir();
        std::fs::create_dir_all(&state_dir)
            .with_context(|| format!("cannot create {}", state_dir.display()))?;
        let port = project.config.runtime.control_port.unwrap_or(0);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("cannot bind control API on 127.0.0.1:{port}"))?;
        let port = listener.local_addr()?.port();
        let control_url = format!("http://127.0.0.1:{port}");

        let mut allowed_origins = vec![
            "tauri://localhost".to_string(),
            "http://tauri.localhost".to_string(),
            "https://tauri.localhost".to_string(),
            control_url.clone(),
            format!("http://localhost:{port}"),
        ];
        if let Some(o) = runtime.frontend_url().as_deref().and_then(origin_of) {
            allowed_origins.push(o.replace("://localhost:", "://127.0.0.1:"));
            allowed_origins.push(o.replace("://127.0.0.1:", "://localhost:"));
            allowed_origins.push(o);
        }

        let agents = &project.config.agents;
        let inner = Arc::new(ShellInner {
            agent_policy: Policy::for_agents(agents, mode),
            sdk_policy: Policy::for_sdk(agents, mode),
            audit: AuditLog::new(Some(state_dir.join("audit.jsonl"))),
            approvals: Arc::new(Approvals::new()),
            hub: UiHub::new(),
            admin_token: token(),
            app_token: token(),
            host,
            control_url: control_url.clone(),
            allowed_origins,
            imported: std::sync::RwLock::new(Vec::new()),
            mode,
            runtime,
            project,
        });
        let shell = Shell { inner };

        shell.inner.runtime.set_extra_env("OPENSHELL_CONTROL_URL", control_url.clone());
        shell.inner.runtime.set_html_injection(Some(format!(
            "<script>window.__OPENSHELL__ = window.__OPENSHELL__ || {};</script><script>{}</script>",
            shell.client_config("browser"),
            openshell_agent::BRIDGE_JS
        )));

        // forward approval requests to the host (desktop dialog)
        let mut rx = shell.inner.approvals.subscribe();
        let s = shell.clone();
        tokio::spawn(async move {
            while let Ok(ev) = rx.recv().await {
                if let ApprovalEvent::Requested(req) = ev {
                    s.inner.host.approval_requested(req, s.inner.approvals.clone());
                }
            }
        });

        shell.watch_openapi();
        server::spawn(shell.clone(), listener);
        state::write_runtime_info(
            &state_dir,
            &state::RuntimeInfo {
                pid: std::process::id(),
                url: control_url,
                token: shell.inner.admin_token.clone(),
                mode,
                host: shell.inner.host.kind().to_string(),
                app: shell.inner.project.config.app.name.clone(),
                started_at: chrono::Utc::now(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        )?;
        Ok(shell)
    }

    /// Start services (autostart) and, in development mode, file watchers.
    pub fn start(&self) {
        if self.inner.project.config.runtime.autostart {
            self.inner.runtime.start_all();
        }
        if self.inner.mode == Mode::Development {
            if let Err(e) = self.inner.runtime.start_watchers() {
                tracing::warn!("file watching disabled: {e:#}");
            }
        }
    }

    /// Stop all services and remove the runtime state file.
    pub async fn shutdown(&self) {
        self.inner.runtime.shutdown().await;
        state::remove_runtime_info(&self.inner.project.state_dir());
    }

    pub fn runtime(&self) -> &Runtime {
        &self.inner.runtime
    }
    pub fn project(&self) -> &Project {
        &self.inner.project
    }
    pub fn mode(&self) -> Mode {
        self.inner.mode
    }
    pub fn control_url(&self) -> &str {
        &self.inner.control_url
    }
    pub fn approvals(&self) -> &Arc<Approvals> {
        &self.inner.approvals
    }
    pub fn hub(&self) -> &UiHub {
        &self.inner.hub
    }

    /// Import OpenAPI documents whenever a service that declares `openapi` becomes healthy.
    fn watch_openapi(&self) {
        let with_openapi: Vec<(String, openshell_config::OpenApiConfig)> = self
            .inner
            .project
            .config
            .services
            .iter()
            .filter_map(|s| s.openapi.as_ref().map(|o| (s.name.clone(), o.normalized())))
            .collect();
        if with_openapi.is_empty() {
            return;
        }
        let shell = self.clone();
        let mut events = self.inner.runtime.subscribe();
        tokio::spawn(async move {
            loop {
                let ev = match events.recv().await {
                    Ok(ev) => ev,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                };
                let openshell_runtime::RuntimeEvent::ServiceState { service, state, .. } = ev else {
                    continue;
                };
                if state != openshell_runtime::ServiceState::Running {
                    continue;
                }
                let Some((_, cfg)) = with_openapi.iter().find(|(n, _)| *n == service) else { continue };
                match shell.load_openapi(&service, cfg).await {
                    Ok(n) => shell.inner.runtime.logs().system(
                        &service,
                        format!("imported {n} API operation(s) for agents from {}", cfg.location),
                    ),
                    Err(e) => {
                        shell.inner.runtime.logs().system(&service, format!("OpenAPI import failed: {e}"))
                    }
                }
            }
        });
    }

    async fn load_openapi(&self, service: &str, cfg: &openshell_config::OpenApiConfig) -> Result<usize> {
        let base = self.inner.runtime.service_url(service).unwrap_or_default();
        let loc = cfg.location.trim();
        let doc: serde_json::Value =
            if loc.starts_with("http://") || loc.starts_with("https://") || loc.starts_with('/') {
                let url = if loc.starts_with('/') {
                    format!("{}{loc}", base.trim_end_matches('/'))
                } else {
                    loc.to_string()
                };
                let client = reqwest::Client::builder().no_proxy().build()?;
                client
                    .get(&url)
                    .timeout(std::time::Duration::from_secs(15))
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?
            } else {
                serde_json::from_str(&std::fs::read_to_string(self.inner.project.resolve_path(loc))?)?
            };
        // servers[0].url may carry a base path (e.g. "/api/v1")
        let server_path = doc
            .pointer("/servers/0/url")
            .and_then(|u| u.as_str())
            .filter(|u| u.starts_with('/'))
            .unwrap_or("");
        let ops =
            openapi::operations(&doc, service, &format!("{}{server_path}", base.trim_end_matches('/')), cfg);
        let n = ops.len();
        let mut imported = self.inner.imported.write().unwrap();
        imported.retain(|o| o.service != service);
        imported.extend(ops);
        Ok(n)
    }

    /// JSON object assigned to `window.__OPENSHELL__` in application pages.
    pub fn client_config(&self, host: &str) -> String {
        let ws = self.inner.control_url.replacen("http://", "ws://", 1);
        json!({
            "api": self.inner.control_url,
            "bridge": format!("{ws}/v1/ui/bridge?token={}", self.inner.app_token),
            "token": self.inner.app_token,
            "host": host,
            "mode": self.inner.mode.to_string(),
            "app": self.inner.project.config.app.name,
        })
        .to_string()
    }

    /// Initialization script for the desktop WebView (config + bridge).
    pub fn webview_init_script(&self) -> String {
        format!("window.__OPENSHELL__ = {};\n{}", self.client_config("desktop"), openshell_agent::BRIDGE_JS)
    }
}

#[cfg(test)]
mod tests;
