//! # openshell-runtime
//!
//! The Runtime Manager of OpenShell. It owns every external component of an
//! application (frontend, backends, databases, workers, local AI servers) and
//! takes care of:
//!
//! * starting them in dependency order (with one-time `setup` steps),
//! * health checks (HTTP / TCP) and readiness,
//! * restart policies with exponential backoff and crash-loop protection,
//! * collecting stdout/stderr into a queryable log store,
//! * dynamic port allocation and `${...}` variable resolution,
//! * serving static frontend builds (with reverse proxy),
//! * restarting services / reloading the frontend when files change (dev mode),
//! * killing whole process trees on stop and when the shell exits,
//! * platform differences (desktop, server, Android) through a [`RuntimeAdapter`],
//! * the app lifecycle on mobile platforms ([`LifecycleEvent`]).
//!
//! The runtime is UI-agnostic: the desktop app, the headless CLI mode and the
//! agent layer all drive the same [`Runtime`].

mod health;
pub mod logs;
mod metrics;
mod plan;
pub mod platform;
pub mod process;
mod static_server;
mod supervisor;
mod watch;

pub use health::{Probe, ProbeConfig};
pub use logs::{LogLine, LogQuery, LogStore, LogStream};
pub use metrics::Metrics;
pub use openshell_config::{BackgroundPolicy, Mode, Platform, Project};
pub use plan::{build_plan, build_plan_for, BuildDef, DefKind, ServiceDef};
pub use platform::{
    default_adapter, AndroidAdapter, BackgroundModel, Capabilities, DesktopAdapter, ProcessSupport,
    RuntimeAdapter,
};

use anyhow::{anyhow, bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch as tokio_watch};

/// Lifecycle state of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    /// Not running and not supposed to run.
    Stopped,
    /// Waiting for dependencies to become healthy.
    Pending,
    /// Running its one-time setup command.
    Setup,
    /// Running its build step (compiled services).
    Building,
    /// Process started, waiting for the health check.
    Starting,
    /// Running and healthy.
    Running,
    /// Running but the health check fails.
    Unhealthy,
    /// Being stopped.
    Stopping,
    /// Crashed; waiting before the next restart attempt.
    Backoff,
    /// Exited with code 0 and is not restarted.
    Exited,
    /// Exited with an error and is not restarted.
    Crashed,
    /// Could not be started or crashed too often.
    Failed,
}

impl ServiceState {
    /// The service has a live process / server (or is about to).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Pending
                | Self::Setup
                | Self::Building
                | Self::Starting
                | Self::Running
                | Self::Unhealthy
                | Self::Stopping
                | Self::Backoff
        )
    }
    /// The service ended and needs a manual start.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Stopped | Self::Exited | Self::Crashed | Self::Failed)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Pending => "pending",
            Self::Setup => "setup",
            Self::Building => "building",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Unhealthy => "unhealthy",
            Self::Stopping => "stopping",
            Self::Backoff => "backoff",
            Self::Exited => "exited",
            Self::Crashed => "crashed",
            Self::Failed => "failed",
        }
    }
}

impl std::fmt::Display for ServiceState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Public, serialisable status of one service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceStatus {
    pub name: String,
    /// `process`, `external` or `static`.
    pub kind: String,
    /// Informational category from the config (`backend`, `database`, `frontend`, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub state: ServiceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub restarts: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    pub depends_on: Vec<String>,
    pub autostart: bool,
    /// CPU and memory of the service's process tree (process services while running).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Metrics>,
}

/// Events emitted by the runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuntimeEvent {
    ServiceState {
        service: String,
        state: ServiceState,
        previous: ServiceState,
    },
    /// Watched frontend files changed; the WebView should reload.
    FrontendReload,
    /// Watched files of a service changed (it is being restarted).
    FilesChanged {
        service: String,
        paths: Vec<String>,
    },
    /// The host reported a lifecycle change (mobile platforms).
    Lifecycle {
        event: LifecycleEvent,
        /// Services paused or resumed because of it.
        services: Vec<String>,
    },
}

/// App lifecycle reported by the host. On the desktop the app is always in the foreground;
/// on Android the host forwards `onStart`/`onStop`, `onTrimMemory`, battery saver and
/// connectivity changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifecycleEvent {
    /// The app is visible again: paused services are resumed.
    Foreground,
    /// The app went to the background: services with `"background": "pause"` are stopped.
    Background,
    /// The system is low on memory: pausable services are stopped.
    LowMemory,
    /// Battery saver switched on or off.
    PowerSave { enabled: bool },
    /// Network connectivity changed.
    Network { connected: bool, metered: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Control {
    pub run: bool,
    pub generation: u64,
}

pub(crate) struct Slot {
    pub def: ServiceDef,
    pub status: Mutex<ServiceStatus>,
    pub control: tokio_watch::Sender<Control>,
}

impl Slot {
    pub fn state(&self) -> ServiceState {
        self.status.lock().unwrap().state
    }

    /// Update the status; emits an event if the state changed.
    pub fn update(&self, rt: &Inner, f: impl FnOnce(&mut ServiceStatus)) {
        let (prev, next) = {
            let mut st = self.status.lock().unwrap();
            let prev = st.state;
            f(&mut st);
            (prev, st.state)
        };
        if prev != next {
            tracing::debug!(service = %self.def.name, %prev, %next, "state change");
            let _ = rt.events.send(RuntimeEvent::ServiceState {
                service: self.def.name.clone(),
                state: next,
                previous: prev,
            });
        }
    }

    pub fn set_state(&self, rt: &Inner, state: ServiceState) {
        self.update(rt, |s| s.state = state);
    }

    /// Change the desired state; only notifies the supervisor on a real change.
    pub fn set_run(&self, run: bool) {
        self.control.send_if_modified(|c| {
            let changed = c.run != run;
            c.run = run;
            changed
        });
    }
}

pub(crate) struct Inner {
    pub mode: Mode,
    pub root: PathBuf,
    pub app_name: String,
    pub slots: Vec<Arc<Slot>>,
    pub logs: Arc<LogStore>,
    pub events: broadcast::Sender<RuntimeEvent>,
    pub http: reqwest::Client,
    pub job: Option<process::ProcessJob>,
    pub extra_env: RwLock<BTreeMap<String, String>>,
    pub html_injection: Arc<RwLock<Option<String>>>,
    pub watchers: Mutex<Vec<watch::WatchHandle>>,
    pub adapter: Arc<dyn RuntimeAdapter>,
    /// Services stopped because the app went to the background; resumed on `Foreground`.
    pub paused: Mutex<BTreeSet<String>>,
    pub in_background: AtomicBool,
}

impl Inner {
    pub fn slot(&self, name: &str) -> Option<&Arc<Slot>> {
        self.slots.iter().find(|s| s.def.name == name)
    }
}

/// Handle to the runtime manager. Cheap to clone.
#[derive(Clone)]
pub struct Runtime {
    inner: Arc<Inner>,
}

impl Runtime {
    /// Create the runtime for a project on the platform this binary was built for.
    /// Must be called inside a Tokio runtime. Services are not started until
    /// [`Runtime::start_all`] or [`Runtime::start`].
    pub fn new(project: &Project, mode: Mode) -> Result<Self> {
        Self::with_adapter(project, mode, default_adapter())
    }

    /// Create the runtime with a specific platform adapter (Android host, server, tests).
    pub fn with_adapter(project: &Project, mode: Mode, adapter: Arc<dyn RuntimeAdapter>) -> Result<Self> {
        let defs = build_plan_for(project, mode, adapter.platform())?;
        let state_dir = project.state_dir();
        let logs =
            Arc::new(LogStore::new(project.config.runtime.log_buffer_lines, Some(state_dir.join("logs"))));
        let (events, _) = broadcast::channel(256);
        let slots = defs
            .into_iter()
            .map(|def| {
                let status = ServiceStatus {
                    name: def.name.clone(),
                    kind: def.kind_name().into(),
                    category: def.category.clone(),
                    description: def.description.clone(),
                    state: ServiceState::Stopped,
                    pid: None,
                    port: def.port,
                    url: def.url.clone(),
                    command: def.command_display(),
                    restarts: 0,
                    exit_code: None,
                    last_error: None,
                    started_at: None,
                    depends_on: def.depends_on.clone(),
                    autostart: def.autostart,
                    metrics: None,
                };
                let (control, _) = tokio_watch::channel(Control { run: false, generation: 0 });
                Arc::new(Slot { def, status: Mutex::new(status), control })
            })
            .collect();
        let http =
            reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build()?;
        let inner = Arc::new(Inner {
            mode,
            root: project.root.clone(),
            app_name: project.config.app.name.clone(),
            slots,
            logs,
            events,
            http,
            job: process::ProcessJob::new(),
            extra_env: RwLock::new(BTreeMap::new()),
            html_injection: Arc::new(RwLock::new(None)),
            watchers: Mutex::new(Vec::new()),
            adapter,
            paused: Mutex::new(BTreeSet::new()),
            in_background: AtomicBool::new(false),
        });
        let handle = tokio::runtime::Handle::try_current()
            .map_err(|_| anyhow!("Runtime::new must be called inside a Tokio runtime"))?;
        for slot in &inner.slots {
            handle.spawn(supervisor::supervise(inner.clone(), slot.clone()));
        }
        // application data directory (survives updates of installed apps)
        let data_dir = project.data_dir();
        let _ = std::fs::create_dir_all(&data_dir);
        inner.extra_env.write().unwrap().insert("OPENSHELL_DATA_DIR".into(), data_dir.display().to_string());
        if inner.slots.iter().any(|s| matches!(s.def.kind, DefKind::Process { .. })) {
            handle.spawn(metrics::sample(Arc::downgrade(&inner)));
        }
        Ok(Self { inner })
    }

    pub fn mode(&self) -> Mode {
        self.inner.mode
    }

    pub fn platform(&self) -> Platform {
        self.inner.adapter.platform()
    }

    /// What the platform offers (processes, background model, window, tray, installers).
    pub fn capabilities(&self) -> Capabilities {
        self.inner.adapter.capabilities()
    }

    pub fn adapter(&self) -> &Arc<dyn RuntimeAdapter> {
        &self.inner.adapter
    }

    /// Build a command for this platform (functions and tasks run outside the supervisor).
    pub fn command(
        &self,
        spec: &openshell_config::CommandSpec,
        extra_args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<tokio::process::Command> {
        self.inner.adapter.command(spec, extra_args, cwd, env)
    }

    /// Apply a lifecycle change reported by the host. Returns the services that were paused
    /// (`Background`, `LowMemory`) or resumed (`Foreground`).
    pub fn lifecycle(&self, event: LifecycleEvent) -> Vec<String> {
        let mut affected = Vec::new();
        match event {
            LifecycleEvent::Background | LifecycleEvent::LowMemory => {
                if event == LifecycleEvent::Background {
                    self.inner.in_background.store(true, Ordering::SeqCst);
                }
                let mut paused = self.inner.paused.lock().unwrap();
                for slot in &self.inner.slots {
                    let wants_run = slot.control.borrow().run;
                    if wants_run
                        && slot.def.background == BackgroundPolicy::Pause
                        && !matches!(slot.def.kind, DefKind::External)
                    {
                        slot.set_run(false);
                        paused.insert(slot.def.name.clone());
                        self.inner.logs.system(&slot.def.name, "paused (app in the background)");
                        affected.push(slot.def.name.clone());
                    }
                }
            }
            LifecycleEvent::Foreground => {
                self.inner.in_background.store(false, Ordering::SeqCst);
                let names = std::mem::take(&mut *self.inner.paused.lock().unwrap());
                for name in names {
                    if self.start(&name).is_ok() {
                        self.inner.logs.system(&name, "resumed (app in the foreground)");
                        affected.push(name);
                    }
                }
            }
            LifecycleEvent::PowerSave { .. } | LifecycleEvent::Network { .. } => {}
        }
        tracing::info!(?event, ?affected, "lifecycle");
        self.emit(RuntimeEvent::Lifecycle { event, services: affected.clone() });
        affected
    }

    /// Whether the host reported that the app is in the background.
    pub fn in_background(&self) -> bool {
        self.inner.in_background.load(Ordering::SeqCst)
    }

    /// Active services that keep running in the background. On Android the host keeps a
    /// foreground service (with a notification) alive while this list is not empty.
    pub fn keep_alive(&self) -> Vec<String> {
        self.inner
            .slots
            .iter()
            .filter(|s| {
                s.def.background == BackgroundPolicy::Keep
                    && !matches!(s.def.kind, DefKind::External)
                    && s.state().is_active()
            })
            .map(|s| s.def.name.clone())
            .collect()
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn app_name(&self) -> &str {
        &self.inner.app_name
    }

    /// Names of all managed services (startup order, frontend last).
    pub fn service_names(&self) -> Vec<String> {
        self.inner.slots.iter().map(|s| s.def.name.clone()).collect()
    }

    pub fn has_service(&self, name: &str) -> bool {
        self.inner.slot(name).is_some()
    }

    pub fn status(&self) -> Vec<ServiceStatus> {
        self.inner.slots.iter().map(|s| s.status.lock().unwrap().clone()).collect()
    }

    pub fn service_status(&self, name: &str) -> Option<ServiceStatus> {
        self.inner.slot(name).map(|s| s.status.lock().unwrap().clone())
    }

    /// URL that the WebView should load.
    pub fn frontend_url(&self) -> Option<String> {
        self.inner.slot(openshell_config::FRONTEND_SERVICE).and_then(|s| s.def.url.clone())
    }

    pub fn service_url(&self, name: &str) -> Option<String> {
        self.inner.slot(name).and_then(|s| s.def.url.clone())
    }

    pub fn logs(&self) -> &Arc<LogStore> {
        &self.inner.logs
    }

    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.inner.events.subscribe()
    }

    /// Emit an event to all subscribers (used e.g. to request a frontend reload).
    pub fn emit(&self, event: RuntimeEvent) {
        let _ = self.inner.events.send(event);
    }

    /// Environment variables added to every process started from now on.
    pub fn set_extra_env(&self, key: impl Into<String>, value: impl Into<String>) {
        self.inner.extra_env.write().unwrap().insert(key.into(), value.into());
    }

    /// Environment every process started by OpenShell gets (without service-specific variables):
    /// `OPENSHELL_*`, `OPENSHELL_DATA_DIR`, database paths, control URL, `PYTHONUNBUFFERED`.
    pub fn base_env(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        env.insert("OPENSHELL".into(), "1".into());
        env.insert("OPENSHELL_MODE".into(), self.inner.mode.to_string());
        env.insert("OPENSHELL_PROJECT_ROOT".into(), self.inner.root.display().to_string());
        if std::env::var_os("PYTHONUNBUFFERED").is_none() {
            env.insert("PYTHONUNBUFFERED".into(), "1".into());
        }
        if std::env::var_os("PYTHONIOENCODING").is_none() {
            env.insert("PYTHONIOENCODING".into(), "utf-8".into());
        }
        for (k, v) in self.inner.extra_env.read().unwrap().iter() {
            env.insert(k.clone(), v.clone());
        }
        env
    }

    /// HTML snippet injected into pages served by the static frontend server.
    pub fn set_html_injection(&self, snippet: Option<String>) {
        *self.inner.html_injection.write().unwrap() = snippet;
    }

    /// Service definitions (resolved commands, build steps, URLs).
    pub fn definitions(&self) -> Vec<ServiceDef> {
        self.inner.slots.iter().map(|s| s.def.clone()).collect()
    }

    /// Start every service with `autostart` (dependencies are respected).
    pub fn start_all(&self) {
        for slot in &self.inner.slots {
            if slot.def.autostart && !slot.state().is_active() {
                slot.set_run(true);
            }
        }
    }

    /// Start a service and (recursively) its dependencies.
    pub fn start(&self, name: &str) -> Result<()> {
        let slot = self.slot(name)?;
        for dep in &slot.def.depends_on {
            if let Some(d) = self.inner.slot(dep) {
                if !d.state().is_active() {
                    self.start(dep)?;
                }
            }
        }
        slot.set_run(true);
        Ok(())
    }

    pub fn stop(&self, name: &str) -> Result<()> {
        self.slot(name)?.set_run(false);
        Ok(())
    }

    /// Restart a service (starts it, and stopped dependencies, if it is not running).
    pub fn restart(&self, name: &str) -> Result<()> {
        let slot = self.slot(name)?;
        for dep in &slot.def.depends_on {
            if let Some(d) = self.inner.slot(dep) {
                if !d.state().is_active() {
                    self.start(dep)?;
                }
            }
        }
        slot.control.send_modify(|c| {
            c.run = true;
            c.generation += 1;
        });
        Ok(())
    }

    /// Wait until a service is running and healthy.
    pub async fn wait_ready(&self, name: &str, timeout: Duration) -> Result<ServiceStatus> {
        let slot = self.slot(name)?.clone();
        let deadline = Instant::now() + timeout;
        loop {
            let st = slot.status.lock().unwrap().clone();
            match st.state {
                ServiceState::Running => return Ok(st),
                ServiceState::Failed | ServiceState::Crashed | ServiceState::Exited => {
                    bail!(
                        "`{name}` is {}{}",
                        st.state,
                        st.last_error.map(|e| format!(": {e}")).unwrap_or_default()
                    )
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                bail!(
                    "`{name}` did not become ready within {}s (state: {}{})",
                    timeout.as_secs(),
                    st.state,
                    st.last_error.map(|e| format!(", {e}")).unwrap_or_default()
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Wait until a service has stopped (any terminal state).
    pub async fn wait_stopped(&self, name: &str, timeout: Duration) -> Result<()> {
        let slot = self.slot(name)?.clone();
        let deadline = Instant::now() + timeout;
        while !slot.state().is_terminal() {
            if Instant::now() >= deadline {
                bail!("`{name}` did not stop within {}s", timeout.as_secs());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(())
    }

    /// Stop all services in reverse dependency order and wait for them.
    pub async fn shutdown(&self) {
        self.inner.watchers.lock().unwrap().clear();
        for slot in self.inner.slots.iter().rev() {
            if slot.state().is_terminal() && !slot.control.borrow().run {
                continue;
            }
            slot.set_run(false);
            let timeout = slot.def.stop_timeout + Duration::from_secs(5);
            if let Err(e) = self.wait_stopped(&slot.def.name, timeout).await {
                tracing::warn!("{e}");
            }
        }
    }

    /// Watch configured paths: restart services / reload the frontend on change.
    pub fn start_watchers(&self) -> Result<()> {
        let mut handles = Vec::new();
        for slot in &self.inner.slots {
            if slot.def.watch.is_empty() || matches!(slot.def.kind, DefKind::External) {
                continue;
            }
            let rt = self.clone();
            let name = slot.def.name.clone();
            let is_frontend = slot.def.is_frontend;
            let h = watch::watch_paths(&slot.def.watch, move |paths| {
                let rel: Vec<String> = paths
                    .iter()
                    .map(|p| p.strip_prefix(rt.root()).unwrap_or(p).display().to_string())
                    .collect();
                rt.logs().system(&name, format!("files changed: {}", rel.join(", ")));
                rt.emit(RuntimeEvent::FilesChanged { service: name.clone(), paths: rel });
                if is_frontend {
                    rt.emit(RuntimeEvent::FrontendReload);
                } else if let Some(st) = rt.service_status(&name) {
                    // restart running services; retry services whose build failed
                    let build_failed = st.state == ServiceState::Failed
                        && st.last_error.as_deref().map(|e| e.starts_with("build failed")).unwrap_or(false);
                    if st.state.is_active() || build_failed {
                        let _ = rt.restart(&name);
                    }
                }
            })?;
            handles.push(h);
        }
        self.inner.watchers.lock().unwrap().extend(handles);
        Ok(())
    }

    fn slot(&self, name: &str) -> Result<&Arc<Slot>> {
        self.inner.slot(name).ok_or_else(|| {
            anyhow!("unknown service `{name}` (available: {})", self.service_names().join(", "))
        })
    }
}

#[cfg(test)]
mod tests;
