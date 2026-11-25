//! # openshell-core
//!
//! Wires the [`Runtime`](openshell_runtime::Runtime) and the Agent Interaction
//! Layer together and serves the local **control API** (HTTP + WebSocket + MCP)
//! that the CLI, the frontend SDK and AI agents talk to.
//!
//! Every front-end of OpenShell uses this crate:
//! * the desktop app (Tauri) launches a [`Shell`] with a desktop [`Host`],
//! * `openshell start --headless` launches it with a [`HeadlessHost`],
//! * a mobile host (Android) launches it with [`Shell::launch_with`] and an
//!   [`AndroidAdapter`](openshell_runtime::AndroidAdapter) and forwards lifecycle events.

pub mod bundle;
pub mod db;
mod exec;
mod fs;
pub mod headless;
pub mod openapi;
pub mod release;
pub mod remote;
pub mod runtimes;
mod server;
pub mod sessions;
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
use openshell_runtime::{LifecycleEvent, Runtime, RuntimeAdapter};
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
    /// Built-in databases.
    pub dbs: db::Databases,
    /// Remote access gateway (`runtime.remote`), if enabled.
    pub remote: Option<Arc<remote::Gateway>>,
    /// Agent sessions with their own tokens and scopes (`openshell agents token`).
    pub sessions: sessions::Sessions,
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
    /// Services are started with [`Shell::start`]. Uses the adapter of the platform this
    /// binary was built for.
    pub async fn launch(project: Project, mode: Mode, host: Arc<dyn Host>) -> Result<Shell> {
        Self::launch_with(project, mode, host, openshell_runtime::default_adapter()).await
    }

    /// Like [`Shell::launch`] with an explicit platform adapter (Android host, server, tests).
    pub async fn launch_with(
        project: Project,
        mode: Mode,
        host: Arc<dyn Host>,
        adapter: Arc<dyn RuntimeAdapter>,
    ) -> Result<Shell> {
        let runtime = Runtime::with_adapter(&project, mode, adapter)?;
        let dbs = db::Databases::open(&project, runtime.logs())?;
        for (k, v) in dbs.env() {
            runtime.set_extra_env(k, v);
        }
        let state_dir = project.state_dir();
        std::fs::create_dir_all(&state_dir)
            .with_context(|| format!("cannot create {}", state_dir.display()))?;
        let port = project.config.runtime.control_port.unwrap_or(0);
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
            .await
            .with_context(|| format!("cannot bind control API on 127.0.0.1:{port}"))?;
        let port = listener.local_addr()?.port();
        let control_url = format!("http://127.0.0.1:{port}");
        let remote = remote::bind(&project).await?;

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
            remote: remote.as_ref().map(|(_, gw)| gw.clone()),
            sessions: sessions::Sessions::load(&project.data_dir()),
            dbs,
            mode,
            runtime,
            project,
        });
        let shell = Shell { inner };

        shell.inner.runtime.set_extra_env("OPENSHELL_CONTROL_URL", control_url.clone());
        // the UI bridge (UI automation for agents) is only injected when agents are on
        shell.inner.runtime.set_html_injection(Some(format!(
            "<script>window.__OPENSHELL__ = window.__OPENSHELL__ || {};</script><script>{}</script>",
            shell.client_config("browser"),
            shell.bridge_js()
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
        if let Some((bound, gw)) = remote {
            tracing::info!("remote gateway on {}", gw.info.urls.join(", "));
            remote::spawn(shell.clone(), gw, bound);
        }
        state::write_runtime_info(
            &state_dir,
            &state::RuntimeInfo {
                pid: std::process::id(),
                url: control_url,
                token: shell.inner.admin_token.clone(),
                mode,
                host: shell.inner.host.kind().to_string(),
                platform: Some(shell.inner.runtime.platform()),
                app: shell.inner.project.config.app.name.clone(),
                started_at: chrono::Utc::now(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        )?;
        Ok(shell)
    }

    /// Start services (autostart) and, in development mode, file watchers.
    pub fn start(&self) {
        self.check_runtimes();
        if self.inner.project.config.runtime.autostart {
            self.inner.runtime.start_all();
        }
        if self.inner.mode == Mode::Development {
            if let Err(e) = self.inner.runtime.start_watchers() {
                tracing::warn!("file watching disabled: {e:#}");
            }
        }
    }

    /// Report bundled runtimes that are not installed and unmet `requires` in the services' logs.
    /// Runs in the background: probing versions starts programs and must not delay the start.
    fn check_runtimes(&self) {
        let cfg = &self.inner.project.config;
        let has_requires = cfg.services.iter().any(|s| !s.requires.is_empty())
            || cfg.frontend.as_ref().map(|f| !f.requires.is_empty()).unwrap_or(false);
        if cfg.runtimes.is_empty() && !has_requires {
            return;
        }
        if self.inner.runtime.platform() == openshell_config::Platform::Android {
            return;
        }
        let project = self.inner.project.clone();
        let logs = self.inner.runtime.logs().clone();
        tokio::task::spawn_blocking(move || {
            for b in openshell_runtime::runtimes::bundled(&project) {
                if !b.installed() {
                    logs.system(
                        "runtime",
                        format!(
                            "bundled runtime `{}` is declared but not installed in {} – run `openshell runtime install`; using the system's copy",
                            b.name,
                            b.dir.display()
                        ),
                    );
                }
            }
            for c in openshell_runtime::runtimes::check_requirements(&project) {
                let line = c.message();
                logs.system(
                    &c.owner,
                    if c.ok { line } else { format!("warning: {line} (`requires` in openshell.json)") },
                );
            }
        });
    }

    /// Forward an app lifecycle change from the host (Android: `onStart`/`onStop`,
    /// `onTrimMemory`, battery saver, connectivity). Returns the paused/resumed services.
    pub fn lifecycle(&self, event: LifecycleEvent) -> Vec<String> {
        self.inner.runtime.lifecycle(event)
    }

    /// Remote access gateway, if enabled (URLs and pairing token).
    pub fn remote(&self) -> Option<&remote::RemoteInfo> {
        self.inner.remote.as_ref().map(|g| &g.info)
    }

    /// The running remote gateway (device pairing, blocked addresses).
    pub fn gateway(&self) -> Option<&Arc<remote::Gateway>> {
        self.inner.remote.as_ref()
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
    /// Built-in databases of the app.
    pub fn databases(&self) -> &db::Databases {
        &self.inner.dbs
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
            // Who draws the title bar; only the desktop window can have an overlay title bar.
            "window": {
                "title_bar": if host == "desktop" { self.inner.project.config.window.title_bar.to_string() } else { "native".into() },
                "os": std::env::consts::OS,
            },
        })
        .to_string()
    }

    /// Initialization script for the desktop WebView (config + window functions + bridge).
    pub fn webview_init_script(&self) -> String {
        format!(
            "window.__OPENSHELL__ = {};\n{}\n{}",
            self.client_config("desktop"),
            WINDOW_JS,
            self.bridge_js()
        )
    }

    /// Whether AI agents may connect (`agents.enabled`).
    pub fn agents_enabled(&self) -> bool {
        self.inner.project.config.agents.enabled
    }

    /// Effective agent permissions for `GET /v1/agents`, `openshell agents` and the runtime panel:
    /// on/off, what agents get now and per mode, agent sessions and paired devices with the scopes
    /// they effectively have.
    pub fn agents_overview(&self) -> serde_json::Value {
        let i = &self.inner;
        let cfg = &i.project.config.agents;
        let policy = &i.agent_policy;
        let by_mode = |m: Mode| if cfg.enabled { cfg.permissions_for(m) } else { Default::default() };
        let sessions: Vec<serde_json::Value> = i
            .sessions
            .list()
            .into_iter()
            .map(|s| {
                json!({ "name": s.name, "profile": s.profile, "scopes": s.scopes,
                        "effective": policy.restricted(&s.scopes).granted,
                        "created_at": s.created_at, "last_seen": s.last_seen })
            })
            .collect();
        let devices: Option<Vec<serde_json::Value>> = i.remote.as_ref().map(|g| {
            g.devices()
                .into_iter()
                .map(|d| {
                    json!({ "name": d.name, "scopes": d.scopes,
                            "effective": policy.restricted(&d.scopes).granted,
                            "paired_at": d.paired_at, "last_seen": d.last_seen })
                })
                .collect()
        });
        json!({
            "enabled": cfg.enabled,
            "mode": i.mode,
            "granted": policy.granted,
            "approval_required": policy.approval_required.iter().filter(|p| policy.granted.contains(p)).collect::<Vec<_>>(),
            "by_mode": { "development": by_mode(Mode::Development), "production": by_mode(Mode::Production) },
            "approval_scopes": cfg.approval_required(),
            "sdk_permissions": i.sdk_policy.granted,
            "sessions": sessions,
            "remote": i.remote.is_some(),
            "devices": devices,
            "pending_approvals": i.approvals.pending().len(),
        })
    }

    /// The UI bridge script, or nothing when the agent layer is off.
    fn bridge_js(&self) -> &'static str {
        if self.agents_enabled() {
            openshell_agent::BRIDGE_JS
        } else {
            ""
        }
    }
}

/// `window.__OPENSHELL__.window` in the desktop window: functions for a frontend that draws its own
/// title bar (`window.title_bar: "overlay"`). Elements with `data-tauri-drag-region` (`="deep"` for a
/// whole bar) move the window; a double click maximizes it. The desktop shell allows these calls only
/// for its own pages and the app's frontend URL.
const WINDOW_JS: &str = r#"(function () {
  var o = window.__OPENSHELL__;
  if (!o || !o.window) return;
  function call(cmd) {
    var t = window.__TAURI_INTERNALS__;
    return t ? t.invoke(cmd, {}) : Promise.reject(new Error("only available in the OpenShell desktop window"));
  }
  o.window.minimize = function () { return call("plugin:window|minimize"); };
  o.window.toggleMaximize = function () { return call("plugin:window|toggle_maximize"); };
  o.window.close = function () { return call("plugin:window|close"); };
  o.window.isMaximized = function () { return call("plugin:window|is_maximized"); };
  // The runtime menu (services, logs, agents) that the native title bar shows on a right click.
  o.window.showMenu = function () { return call("show_window_menu"); };
})();"#;

#[cfg(test)]
mod tests;
