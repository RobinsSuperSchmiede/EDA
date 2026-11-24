//! Configuration data model (schema version 1).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Operating mode of the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Project files are used directly, hot reload, broad agent access.
    #[serde(alias = "dev")]
    Development,
    /// Stable runtime for end users, restricted agent access.
    #[serde(alias = "prod")]
    Production,
}

/// Platform an OpenShell runtime runs on. The core is the same everywhere; a platform
/// adapter decides how processes are started and how the app lifecycle is handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// Windows, macOS and Linux with a native window (also `--headless` on a workstation).
    Desktop,
    /// Android phones and tablets: the app runs as a personal runtime and mini server.
    Android,
    /// A machine without a user session (headless runtime, always on).
    Server,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Android => "android",
            Self::Server => "server",
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What happens to a service while the app is in the background (mobile platforms;
/// on the desktop the app never goes to the background).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundPolicy {
    /// Keep running (on Android this keeps a foreground service with a notification alive).
    #[default]
    Keep,
    /// Stop while the app is in the background and start again when it returns.
    Pause,
}

/// Root of an `openshell.json` / `openshell.toml` file.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Optional reference to the JSON schema (ignored by OpenShell).
    #[serde(rename = "$schema", default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Schema version of this file. Currently `1`.
    #[serde(default = "default_version")]
    pub version: u32,
    /// Application metadata.
    pub app: AppConfig,
    /// Native window settings.
    #[serde(default)]
    pub window: WindowConfig,
    /// The web frontend shown in the embedded WebView.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontend: Option<FrontendConfig>,
    /// Backends, databases, workers, local AI servers, ...
    #[serde(default)]
    pub services: Vec<ServiceConfig>,
    /// Runtime manager behaviour.
    #[serde(default)]
    pub runtime: RuntimeConfig,
    /// Agent Interaction Layer settings (permissions, approvals, providers).
    #[serde(default)]
    pub agents: AgentsConfig,
    /// Functions the application exposes to agents.
    #[serde(default)]
    pub capabilities: Vec<CapabilityConfig>,
    /// Named, pre-declared commands (e.g. `test`, `lint`, `migrate`).
    #[serde(default)]
    pub tasks: BTreeMap<String, TaskSpec>,
    /// Installer settings (`openshell package`).
    #[serde(default)]
    pub package: PackageConfig,
    /// Built-in databases (SQLite), managed by OpenShell: created, migrated, exposed to services
    /// (`OPENSHELL_DB_<NAME>`, `DATABASE_URL`), to the frontend SDK and – if allowed – to agents.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub databases: Vec<DatabaseConfig>,
}

fn default_version() -> u32 {
    crate::CONFIG_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    /// Human readable application name (used as window title by default).
    pub name: String,
    /// Reverse-DNS identifier, e.g. `com.example.myapp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Application icon (PNG, ideally 512x512) used for the window and installers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowConfig {
    /// Window title. Defaults to `app.name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default = "default_width")]
    pub width: f64,
    #[serde(default = "default_height")]
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_height: Option<f64>,
    #[serde(default = "default_true")]
    pub resizable: bool,
    #[serde(default)]
    pub maximized: bool,
    /// Hide the window instead of quitting when it is closed (the runtime keeps running in the tray).
    #[serde(default)]
    pub close_to_tray: bool,
    /// Show a system tray icon.
    #[serde(default = "default_true")]
    pub tray: bool,
    /// Enable WebView devtools. Defaults to `true` in development mode only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub devtools: Option<bool>,
    /// `native` (default): the system's title bar. `overlay`: the frontend draws its own title bar –
    /// no system title bar on Windows and Linux, the window buttons over the content on macOS. The
    /// frontend gets `window.__OPENSHELL__.window` (drag, minimize, maximize, close, runtime menu).
    #[serde(default)]
    pub title_bar: TitleBar,
    /// Open a window. `false`: no window – the app runs in the tray / background (services,
    /// databases, agent tools). Apps without a frontend show OpenShell's console in the window.
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Who draws the window's title bar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TitleBar {
    #[default]
    Native,
    Overlay,
}

impl std::fmt::Display for TitleBar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            TitleBar::Native => "native",
            TitleBar::Overlay => "overlay",
        })
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: None,
            width: default_width(),
            height: default_height(),
            min_width: None,
            min_height: None,
            resizable: true,
            maximized: false,
            close_to_tray: false,
            tray: true,
            devtools: None,
            title_bar: TitleBar::Native,
            enabled: true,
        }
    }
}

fn default_width() -> f64 {
    1280.0
}
fn default_height() -> f64 {
    800.0
}
fn default_true() -> bool {
    true
}

/// A command, either as a shell string (`"npm run dev"`, executed through the
/// platform shell) or as an argument vector (`["python", "main.py"]`, executed directly).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum CommandSpec {
    Shell(String),
    Argv(Vec<String>),
}

impl CommandSpec {
    pub fn is_empty(&self) -> bool {
        match self {
            CommandSpec::Shell(s) => s.trim().is_empty(),
            CommandSpec::Argv(v) => v.is_empty() || v[0].trim().is_empty(),
        }
    }

    /// The program that will be looked up in `PATH` (first word for shell strings).
    pub fn program(&self) -> Option<String> {
        match self {
            CommandSpec::Shell(s) => s.split_whitespace().next().map(str::to_string),
            CommandSpec::Argv(v) => v.first().cloned(),
        }
    }

    pub fn display(&self) -> String {
        match self {
            CommandSpec::Shell(s) => s.clone(),
            CommandSpec::Argv(v) => v.join(" "),
        }
    }
}

/// A port: a fixed number or `"auto"` to let OpenShell pick a free port.
/// The chosen port is exposed to the process as `PORT` and as `${port}` /
/// `${services.<name>.port}` in the configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PortSpec {
    Fixed(u16),
    Named(String),
}

impl PortSpec {
    pub fn is_auto(&self) -> bool {
        matches!(self, PortSpec::Named(s) if s == "auto")
    }
    pub fn fixed(&self) -> Option<u16> {
        match self {
            PortSpec::Fixed(p) => Some(*p),
            _ => None,
        }
    }
}

/// A one-time preparation step, e.g. `npm install`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetupConfig {
    pub command: CommandSpec,
    /// Skip the setup if this path (relative to the project root) exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creates: Option<String>,
    /// Working directory (defaults to the service working directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

/// Health check: either a URL string (HTTP GET, 2xx/3xx is healthy) or a detailed object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum HealthCheckSpec {
    Url(String),
    Detailed(HealthCheck),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HealthCheck {
    /// HTTP URL that must answer with 2xx/3xx.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,
    /// `host:port` that must accept TCP connections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp: Option<String>,
    #[serde(default = "default_interval")]
    pub interval_ms: u64,
    #[serde(default = "default_hc_timeout")]
    pub timeout_ms: u64,
    /// Consecutive failures before a running service is reported as unhealthy.
    #[serde(default = "default_failure_threshold")]
    pub failure_threshold: u32,
    /// How long a starting service may take to become healthy.
    #[serde(default = "default_ready_timeout")]
    pub ready_timeout_ms: u64,
}

impl HealthCheck {
    pub fn http(url: impl Into<String>) -> Self {
        Self {
            http: Some(url.into()),
            tcp: None,
            interval_ms: default_interval(),
            timeout_ms: default_hc_timeout(),
            failure_threshold: default_failure_threshold(),
            ready_timeout_ms: default_ready_timeout(),
        }
    }
    pub fn tcp(addr: impl Into<String>) -> Self {
        Self { http: None, tcp: Some(addr.into()), ..Self::http("") }
    }
}

impl HealthCheckSpec {
    pub fn normalized(&self) -> HealthCheck {
        match self {
            HealthCheckSpec::Url(u) => HealthCheck::http(u.clone()),
            HealthCheckSpec::Detailed(h) => h.clone(),
        }
    }
}

fn default_interval() -> u64 {
    5_000
}
fn default_hc_timeout() -> u64 {
    2_000
}
fn default_failure_threshold() -> u32 {
    3
}
fn default_ready_timeout() -> u64 {
    60_000
}

/// When a crashed process is restarted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    Never,
    OnFailure,
    Always,
}

/// `true` (= on-failure), `false` (= never) or an explicit policy.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RestartSpec {
    Bool(bool),
    Policy(RestartPolicy),
}

impl RestartSpec {
    pub fn policy(&self) -> RestartPolicy {
        match self {
            RestartSpec::Bool(true) => RestartPolicy::OnFailure,
            RestartSpec::Bool(false) => RestartPolicy::Never,
            RestartSpec::Policy(p) => *p,
        }
    }
}

/// An external component managed (or monitored) by the runtime.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServiceConfig {
    /// Unique name (`[a-z0-9_-]`), used by CLI, API and agents.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Informational category: `backend`, `database`, `worker`, `ai`, ...
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Command to run. Omit for external services.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandSpec>,
    /// Extra arguments appended to `command`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Working directory relative to the project root.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<PortSpec>,
    /// Base URL of the service. Defaults to `http://127.0.0.1:<port>` when a port is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// The service is already running elsewhere: do not spawn it, only monitor it.
    #[serde(default)]
    pub external: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub healthcheck: Option<HealthCheckSpec>,
    /// Restart policy (default: on-failure).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<RestartSpec>,
    /// Maximum restarts within one minute before the service is marked as failed.
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
    /// Services that must be healthy before this one starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Start automatically with the runtime.
    #[serde(default = "default_true")]
    pub autostart: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<SetupConfig>,
    /// Build step for compiled services (C++, Java, Go, Rust, ...). Runs before every start in
    /// development mode and during `openshell build` / `openshell package`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildSpec>,
    /// Import the service's OpenAPI document as agent capabilities (path on the service, URL or file).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openapi: Option<OpenApiSpec>,
    /// Paths (relative to the project root) that trigger a restart when changed (development mode).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch: Vec<String>,
    /// Only run in these modes (default: all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<Vec<Mode>>,
    /// Only run on these platforms (default: all). Example: `["desktop", "server"]` for a service
    /// that needs a toolchain which is not available on Android.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platforms: Option<Vec<Platform>>,
    /// Behaviour while the app is in the background on mobile platforms (default: `keep`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<BackgroundPolicy>,
    /// Grace period between the stop signal and a forced kill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_timeout_ms: Option<u64>,
}

fn default_max_restarts() -> u32 {
    5
}

impl ServiceConfig {
    /// A minimal service definition (useful for tests and generators).
    pub fn new(name: impl Into<String>, command: CommandSpec) -> Self {
        Self {
            name: name.into(),
            description: None,
            kind: None,
            command: Some(command),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
            port: None,
            url: None,
            external: false,
            healthcheck: None,
            restart: None,
            max_restarts: default_max_restarts(),
            depends_on: vec![],
            autostart: true,
            setup: None,
            build: None,
            openapi: None,
            watch: vec![],
            modes: None,
            platforms: None,
            background: None,
            stop_timeout_ms: None,
        }
    }

    pub fn restart_policy(&self) -> RestartPolicy {
        self.restart.as_ref().map(RestartSpec::policy).unwrap_or(RestartPolicy::OnFailure)
    }

    pub fn enabled_in(&self, mode: Mode) -> bool {
        self.modes.as_ref().map(|m| m.contains(&mode)).unwrap_or(true)
    }

    /// Whether the service runs on `platform` (no `platforms` list: everywhere).
    pub fn runs_on(&self, platform: Platform) -> bool {
        self.platforms.as_ref().map(|p| p.contains(&platform)).unwrap_or(true)
    }
}

/// The web frontend. Framework-agnostic: OpenShell only needs a URL to load,
/// a command that serves it (dev server) or a directory of built files.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrontendConfig {
    /// Dev server command, e.g. `npm run dev`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<PortSpec>,
    /// URL loaded into the WebView (dev server or external URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Directory with built static files (served by OpenShell itself).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dist: Option<String>,
    /// Production build command (`openshell build`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build: Option<CommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<SetupConfig>,
    /// Reverse proxy rules for the static server, e.g. `{"/api": "${services.backend.url}"}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub proxy: BTreeMap<String, String>,
    /// Paths that reload the WebView when changed (development mode, static frontends).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub healthcheck: Option<HealthCheckSpec>,
    /// Services that must be healthy before the frontend starts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    /// Overrides used in production mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production: Option<FrontendOverride>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrontendOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dist: Option<String>,
}

/// How the frontend is provided in a given mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontendPlan {
    /// A process serves the frontend at `url` (dev server, local web server).
    Process { command: CommandSpec, url: String },
    /// Static files served by OpenShell's built-in server.
    Static { dist: String },
    /// An already running / remote URL.
    External { url: String },
}

impl FrontendConfig {
    /// Decide how the frontend is provided in `mode`.
    ///
    /// Development: dev server (`command` + `url`) > external `url` > static `dist`.
    /// Production: `production` overrides > static `dist` > development rules.
    pub fn plan(&self, mode: Mode) -> Option<FrontendPlan> {
        let has_port = self.port.is_some();
        if mode == Mode::Production {
            if let Some(o) = &self.production {
                let plan = Self::plan_from(o.command.clone(), o.url.clone(), o.dist.clone(), has_port);
                if plan.is_some() {
                    return plan;
                }
            }
            if let Some(dist) = &self.dist {
                return Some(FrontendPlan::Static { dist: dist.clone() });
            }
        }
        Self::plan_from(self.command.clone(), self.url.clone(), self.dist.clone(), has_port)
    }

    fn plan_from(
        command: Option<CommandSpec>,
        url: Option<String>,
        dist: Option<String>,
        has_port: bool,
    ) -> Option<FrontendPlan> {
        match (command, url, dist) {
            (Some(command), Some(url), _) => Some(FrontendPlan::Process { command, url }),
            (Some(command), None, _) if has_port => {
                Some(FrontendPlan::Process { command, url: "http://127.0.0.1:${port}".into() })
            }
            (None, Some(url), _) => Some(FrontendPlan::External { url }),
            (_, _, Some(dist)) => Some(FrontendPlan::Static { dist }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    /// Start all services automatically when the shell starts.
    #[serde(default = "default_true")]
    pub autostart: bool,
    /// Keep the runtime running in the background when the window is closed.
    #[serde(default)]
    pub background_mode: bool,
    /// Fixed port for the local control API (default: random free port).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_port: Option<u16>,
    /// Number of log lines kept in memory (all services).
    #[serde(default = "default_log_lines")]
    pub log_buffer_lines: usize,
    /// Grace period for stopping services before they are killed.
    #[serde(default = "default_shutdown_timeout")]
    pub shutdown_timeout_ms: u64,
    /// Start the installed app when the user logs in (applies to installed apps only).
    #[serde(default)]
    pub launch_at_login: bool,
    /// Make the app usable from other devices (for example a phone that hosts the app and a
    /// laptop that uses it). Off by default.
    #[serde(default, skip_serializing_if = "RemoteConfig::is_disabled")]
    pub remote: RemoteConfig,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            autostart: true,
            background_mode: false,
            control_port: None,
            log_buffer_lines: default_log_lines(),
            shutdown_timeout_ms: default_shutdown_timeout(),
            launch_at_login: false,
            remote: RemoteConfig::default(),
        }
    }
}

/// Remote access gateway: serves the app profile (the app's own functions over MCP) to
/// other devices in the network, protected by a pairing token.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteConfig {
    /// Start the gateway.
    #[serde(default)]
    pub enabled: bool,
    /// Address to listen on. `0.0.0.0` makes the gateway reachable in the local network; use a
    /// VPN address (for example a Tailscale IP) to limit it to your own devices.
    #[serde(default = "default_remote_bind")]
    pub bind: String,
    /// Fixed port (default: 7420).
    #[serde(default = "default_remote_port")]
    pub port: u16,
    /// IP addresses or CIDR ranges that may connect (`192.168.1.0/24`, `100.64.0.0/10`,
    /// `fd7a:115c:a1e0::/48`). Applies to the gateway and every exposed service. Empty: everyone
    /// who can reach the port.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    /// Accept the shared pairing token (`.openshell/data/remote-token`). Set to `false` to accept
    /// only per-device tokens from `openshell remote pair <device>`.
    #[serde(default = "default_true")]
    pub shared_token: bool,
    /// Requests per minute per client address and a lockout after failed logins.
    #[serde(default, skip_serializing_if = "RateLimitConfig::is_default")]
    pub rate_limit: RateLimitConfig,
    /// HTTPS for the gateway and exposed HTTP services.
    #[serde(default, skip_serializing_if = "TlsConfig::is_disabled")]
    pub tls: TlsConfig,
    /// Services reachable from other devices on their own port (opt-in per service).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expose: Vec<ExposeConfig>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_remote_bind(),
            port: default_remote_port(),
            allow: Vec::new(),
            shared_token: true,
            rate_limit: RateLimitConfig::default(),
            tls: TlsConfig::default(),
            expose: Vec::new(),
        }
    }
}

impl RemoteConfig {
    fn is_disabled(&self) -> bool {
        !self.enabled
            && self.bind == default_remote_bind()
            && self.port == default_remote_port()
            && self.allow.is_empty()
            && self.shared_token
            && self.rate_limit.is_default()
            && self.tls.is_disabled()
            && self.expose.is_empty()
    }
}

/// Limits per client address. `0` switches a limit off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RateLimitConfig {
    /// Requests (gateway) and new connections (exposed services) per minute and address.
    #[serde(default = "default_requests_per_minute")]
    pub requests_per_minute: u32,
    /// Failed logins (wrong or missing token) before the address is blocked.
    #[serde(default = "default_max_failed_auth")]
    pub max_failed_auth: u32,
    /// How long a blocked address stays blocked.
    #[serde(default = "default_block_minutes")]
    pub block_minutes: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: default_requests_per_minute(),
            max_failed_auth: default_max_failed_auth(),
            block_minutes: default_block_minutes(),
        }
    }
}

impl RateLimitConfig {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_requests_per_minute() -> u32 {
    120
}
fn default_max_failed_auth() -> u32 {
    10
}
fn default_block_minutes() -> u64 {
    5
}

/// TLS for the remote gateway. Without `cert` and `key` OpenShell creates a self-signed
/// certificate in the app's data directory; clients pin its SHA-256 fingerprint
/// (`openshell remote` shows it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TlsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// PEM certificate chain (relative to the project).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert: Option<String>,
    /// PEM private key (relative to the project).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl TlsConfig {
    fn is_disabled(&self) -> bool {
        *self == Self::default()
    }
}

/// A service made reachable from other devices on its own port.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExposeConfig {
    /// Service name, or `frontend` for the app's frontend.
    pub service: String,
    /// Port other devices connect to.
    pub port: u16,
    /// `http` (websites and APIs; token login, TLS) or `tcp` (any protocol, e.g. a database;
    /// protected by the address allowlist only).
    #[serde(default)]
    pub protocol: ExposeProtocol,
    /// `token` (default for `http`): a pairing or device token is required – as
    /// `Authorization: Bearer`, cookie, or once as `?openshell_token=` in the browser.
    /// `none`: no login (rely on `allow`). Always `none` for `tcp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<ExposeAuth>,
    /// Addresses / CIDR ranges for this port; replaces `runtime.remote.allow`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<String>>,
}

impl ExposeConfig {
    pub fn effective_auth(&self) -> ExposeAuth {
        match self.protocol {
            ExposeProtocol::Tcp => ExposeAuth::None,
            ExposeProtocol::Http => self.auth.unwrap_or(ExposeAuth::Token),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExposeProtocol {
    #[default]
    Http,
    Tcp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExposeAuth {
    Token,
    None,
}

/// Parse an IP address or CIDR range (`10.0.0.0/8`, `192.168.1.7`, `::1`).
pub fn parse_cidr(s: &str) -> Option<(std::net::IpAddr, u8)> {
    let (ip, len) = match s.trim().split_once('/') {
        Some((ip, len)) => (ip.parse::<std::net::IpAddr>().ok()?, Some(len.parse::<u8>().ok()?)),
        None => (s.trim().parse::<std::net::IpAddr>().ok()?, None),
    };
    let max = if ip.is_ipv4() { 32 } else { 128 };
    let len = len.unwrap_or(max);
    (len <= max).then_some((ip, len))
}

/// Whether `addr` lies inside the range from [`parse_cidr`]. IPv4-mapped IPv6 addresses match
/// IPv4 ranges.
pub fn cidr_contains(range: (std::net::IpAddr, u8), addr: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    let addr = match addr {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(addr),
        a => a,
    };
    match (range.0, addr) {
        (IpAddr::V4(net), IpAddr::V4(a)) => {
            let mask = if range.1 == 0 { 0 } else { u32::MAX << (32 - range.1) };
            u32::from(net) & mask == u32::from(a) & mask
        }
        (IpAddr::V6(net), IpAddr::V6(a)) => {
            let mask = if range.1 == 0 { 0 } else { u128::MAX << (128 - range.1) };
            u128::from(net) & mask == u128::from(a) & mask
        }
        _ => false,
    }
}

fn default_remote_bind() -> String {
    "0.0.0.0".into()
}
fn default_remote_port() -> u16 {
    7420
}

fn default_log_lines() -> usize {
    10_000
}
fn default_shutdown_timeout() -> u64 {
    5_000
}

/// Permission scopes for agents. Every agent tool requires exactly one scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Read state, status, logs, capabilities.
    #[serde(alias = "read-only", alias = "readonly")]
    Read,
    /// Inspect and operate the user interface.
    #[serde(alias = "ui-interaction")]
    Ui,
    /// Invoke application capabilities that change data.
    #[serde(alias = "write")]
    Actions,
    /// Start, stop and restart services.
    #[serde(alias = "service-management")]
    Services,
    /// Read project files.
    #[serde(alias = "file-access")]
    Files,
    /// Modify project files.
    #[serde(alias = "code-modification")]
    Code,
    /// Native system access: notifications, declared tasks, script evaluation.
    #[serde(alias = "native", alias = "native-system")]
    System,
}

impl Permission {
    pub const ALL: [Permission; 7] = [
        Permission::Read,
        Permission::Ui,
        Permission::Actions,
        Permission::Services,
        Permission::Files,
        Permission::Code,
        Permission::System,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            Permission::Read => "read",
            Permission::Ui => "ui",
            Permission::Actions => "actions",
            Permission::Services => "services",
            Permission::Files => "files",
            Permission::Code => "code",
            Permission::System => "system",
        }
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Either one permission list for all modes or one per mode.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PermissionSpec {
    All(Vec<Permission>),
    ByMode {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        development: Option<Vec<Permission>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        production: Option<Vec<Permission>>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgentsConfig {
    /// Enable the Agent Interaction Layer (tools, MCP, UI bridge).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Scopes granted to agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionSpec>,
    /// Scopes whose actions need explicit human approval. Default: `["system"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_required: Option<Vec<Permission>>,
    /// How long to wait for a human decision before denying.
    #[serde(default = "default_approval_timeout")]
    pub approval_timeout_ms: u64,
    /// Scopes granted to the frontend through the SDK. Default: `["read", "ui"]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_permissions: Option<Vec<Permission>>,
    /// AI providers known to this application (for discovery by agents).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<ProviderConfig>,
    /// Probe well-known local providers (Ollama, LM Studio).
    #[serde(default = "default_true")]
    pub detect_local_providers: bool,
}

impl Default for AgentsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            permissions: None,
            approval_required: None,
            approval_timeout_ms: default_approval_timeout(),
            sdk_permissions: None,
            providers: vec![],
            detect_local_providers: true,
        }
    }
}

fn default_approval_timeout() -> u64 {
    120_000
}

impl AgentsConfig {
    /// Effective agent permissions in `mode`.
    pub fn permissions_for(&self, mode: Mode) -> BTreeSet<Permission> {
        use Permission::*;
        let default_dev = vec![Read, Ui, Actions, Services, Files, Code];
        let default_prod = vec![Read, Ui, Actions];
        let list = match (&self.permissions, mode) {
            (Some(PermissionSpec::All(v)), _) => v.clone(),
            (Some(PermissionSpec::ByMode { development, .. }), Mode::Development) => {
                development.clone().unwrap_or(default_dev)
            }
            (Some(PermissionSpec::ByMode { production, .. }), Mode::Production) => {
                production.clone().unwrap_or(default_prod)
            }
            (None, Mode::Development) => default_dev,
            (None, Mode::Production) => default_prod,
        };
        list.into_iter().collect()
    }

    pub fn approval_required(&self) -> BTreeSet<Permission> {
        self.approval_required.clone().unwrap_or_else(|| vec![Permission::System]).into_iter().collect()
    }

    pub fn sdk_permissions(&self) -> BTreeSet<Permission> {
        self.sdk_permissions
            .clone()
            .unwrap_or_else(|| vec![Permission::Read, Permission::Ui])
            .into_iter()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    Ollama,
    LmStudio,
    OpenaiCompatible,
    Openai,
    Anthropic,
    Mcp,
    Custom,
}

/// An AI provider connection. OpenShell does not run models itself; it only
/// makes providers discoverable to agents and the application.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub name: String,
    pub kind: ProviderKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Name of the environment variable holding the API key (the key itself never goes into the config).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key_env: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
}

/// A function of the application that agents can discover and invoke.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityConfig {
    /// Unique name (`[a-z0-9_]`).
    pub name: String,
    pub description: String,
    /// Required scope. `read` for queries, `actions` (alias `write`) for changes.
    #[serde(default = "default_cap_permission")]
    pub permission: Permission,
    /// JSON Schema of the input object.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// Invoke by calling an HTTP endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<HttpTarget>,
    /// Invoke by running a declared task.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Invoke by running a script/program: it receives the input as JSON on stdin (and in
    /// `OPENSHELL_INPUT`) and prints its result (JSON or text) on stdout. No server needed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunSpec>,
}

fn default_cap_permission() -> Permission {
    Permission::Read
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HttpTarget {
    /// Service whose base URL is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// Absolute URL (alternative to `service` + `path`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default)]
    pub path: String,
}

fn default_method() -> String {
    "POST".into()
}

/// A task: a shell string / argv, or a detailed object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TaskSpec {
    Command(CommandSpec),
    Detailed(TaskConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TaskConfig {
    pub command: CommandSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_task_timeout")]
    pub timeout_ms: u64,
}

fn default_task_timeout() -> u64 {
    300_000
}

impl TaskSpec {
    pub fn normalized(&self) -> TaskConfig {
        match self {
            TaskSpec::Command(c) => TaskConfig {
                command: c.clone(),
                description: None,
                cwd: None,
                env: BTreeMap::new(),
                timeout_ms: default_task_timeout(),
            },
            TaskSpec::Detailed(t) => t.clone(),
        }
    }
}

impl Config {
    /// A minimal configuration with just an application name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            schema: None,
            version: crate::CONFIG_VERSION,
            app: AppConfig { name: name.into(), id: None, version: None, description: None, icon: None },
            window: WindowConfig::default(),
            frontend: None,
            services: vec![],
            runtime: RuntimeConfig::default(),
            agents: AgentsConfig::default(),
            capabilities: vec![],
            tasks: BTreeMap::new(),
            package: PackageConfig::default(),
            databases: vec![],
        }
    }

    pub fn service(&self, name: &str) -> Option<&ServiceConfig> {
        self.services.iter().find(|s| s.name == name)
    }

    pub fn window_title(&self) -> String {
        self.window.title.clone().unwrap_or_else(|| self.app.name.clone())
    }
}

/// When a service build step runs automatically before the service starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum BuildWhen {
    /// Before every start in development mode (default).
    #[default]
    Development,
    /// Before every start in all modes.
    Always,
    /// Only via `openshell build` / `openshell package`.
    Never,
}

/// A build command (`"cmake --build build"`) or a detailed object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BuildSpec {
    Command(CommandSpec),
    Detailed(BuildConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BuildConfig {
    pub command: CommandSpec,
    /// Working directory (defaults to the service working directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub on_start: BuildWhen,
}

impl BuildSpec {
    pub fn normalized(&self) -> BuildConfig {
        match self {
            BuildSpec::Command(c) => {
                BuildConfig { command: c.clone(), cwd: None, on_start: BuildWhen::Development }
            }
            BuildSpec::Detailed(b) => b.clone(),
        }
    }
}

/// Where to find an OpenAPI 3 document: a path on the service (`"/openapi.json"`),
/// an absolute URL, a file in the project, or a detailed object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OpenApiSpec {
    Location(String),
    Detailed(OpenApiConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenApiConfig {
    /// Path on the service, absolute URL or project file.
    pub location: String,
    /// Only import operations whose operationId or path contains one of these strings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Skip operations whose operationId or path contains one of these strings.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Prefix for the generated capability names.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
}

impl OpenApiSpec {
    pub fn normalized(&self) -> OpenApiConfig {
        match self {
            OpenApiSpec::Location(l) => {
                OpenApiConfig { location: l.clone(), include: vec![], exclude: vec![], prefix: None }
            }
            OpenApiSpec::Detailed(c) => c.clone(),
        }
    }
}

/// Settings for `openshell package` (native installers).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageConfig {
    /// Bundle identifier (default: `app.id`, else `dev.openshell.<name>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    /// License file shown by the installer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_file: Option<String>,
    /// Files/directories (glob patterns, relative to the project) to ship. Default: the whole project
    /// except `.git`, `.openshell`, `node_modules`, `__pycache__` and virtualenvs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Glob patterns to leave out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
    /// Installer formats: `nsis`, `msi` (Windows), `app`, `dmg` (macOS), `deb`, `rpm`, `appimage` (Linux).
    /// Default: `nsis` / `dmg` / `deb` + `appimage`. `bundle` builds the portable app bundle
    /// (`.osapp`) on every platform, including Android.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    /// Code signing of installers. Secrets come from the environment, never from this file.
    #[serde(default, skip_serializing_if = "SigningConfig::is_empty")]
    pub signing: SigningConfig,
    /// URL of the `downloads.json` of the newest release (`openshell release manifest`). The
    /// installed app compares versions and reports updates (`GET /v1/update`, `openshell update`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub update_url: Option<String>,
}

/// Code signing for `openshell package`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SigningConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub windows: Option<WindowsSigning>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos: Option<MacosSigning>,
}

impl SigningConfig {
    fn is_empty(&self) -> bool {
        self.windows.is_none() && self.macos.is_none()
    }
}

/// Windows Authenticode signing: a certificate in the certificate store (thumbprint) or a
/// custom command (Azure Trusted Signing, a cloud HSM, `osslsigncode` …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowsSigning {
    /// SHA-1 thumbprint of a code-signing certificate in the user's or machine's store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate_thumbprint: Option<String>,
    /// RFC 3161 / Authenticode timestamp server, e.g. `http://timestamp.digicert.com`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_url: Option<String>,
    /// Custom sign command; `%1` is replaced with the file to sign,
    /// e.g. `["trusted-signing-cli", "-e", "https://…", "%1"]`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sign_command: Vec<String>,
}

/// macOS code signing and notarisation (credentials from `APPLE_ID`, `APPLE_PASSWORD`,
/// `APPLE_TEAM_ID` or `APPLE_API_KEY` / `APPLE_API_ISSUER` in the environment).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MacosSigning {
    /// `Developer ID Application: Example Inc. (TEAMID)`, or `-` for ad-hoc signing.
    pub signing_identity: String,
    /// Entitlements plist (relative to the project).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entitlements: Option<String>,
    /// Team short name for notarisation when the Apple ID belongs to several teams.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_short_name: Option<String>,
}

impl Config {
    /// Bundle identifier used for installers and per-user data directories.
    pub fn identifier(&self) -> String {
        self.package
            .identifier
            .clone()
            .or_else(|| self.app.id.clone())
            .unwrap_or_else(|| format!("dev.openshell.{}", slug(&self.app.name).replace('-', "")))
    }

    /// File-system friendly name (`My App` -> `my-app`).
    pub fn slug(&self) -> String {
        slug(&self.app.name)
    }
}

/// `My Cool App!` -> `my-cool-app`
pub fn slug(name: &str) -> String {
    let s: String =
        name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() {
        "app".into()
    } else {
        s
    }
}

/// A function implemented as a program: a command (`["python", "tools/greet.py"]`) or a detailed object.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RunSpec {
    Command(CommandSpec),
    Detailed(RunConfig),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub command: CommandSpec,
    /// Working directory relative to the project root (default: project root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default = "default_run_timeout")]
    pub timeout_ms: u64,
}

fn default_run_timeout() -> u64 {
    60_000
}

impl RunSpec {
    pub fn normalized(&self) -> RunConfig {
        match self {
            RunSpec::Command(c) => RunConfig {
                command: c.clone(),
                cwd: None,
                env: BTreeMap::new(),
                timeout_ms: default_run_timeout(),
            },
            RunSpec::Detailed(r) => r.clone(),
        }
    }
}

/// Database engines OpenShell manages itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseKind {
    #[default]
    Sqlite,
}

/// What agents may do with a database (the app's own frontend always has access via the SDK
/// within `agents.sdk_permissions`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseAccess {
    None,
    /// `db_list`, `db_schema`, `db_query` (SELECT only).
    #[default]
    Read,
    /// Additionally `db_execute` (INSERT/UPDATE/DELETE/DDL) – needs the `actions` scope.
    Write,
}

/// A database managed by OpenShell.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// Unique name (`[a-z0-9_]`), e.g. `main`.
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: DatabaseKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Database file. Relative paths are inside the app's data directory (`OPENSHELL_DATA_DIR`),
    /// so installed apps keep their data per user. Default: `<name>.db`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Directory (relative to the project) with `*.sql` migrations, applied once each, in file-name order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrations: Option<String>,
    /// Agent access: `none`, `read` (default) or `write`.
    #[serde(default)]
    pub agent_access: DatabaseAccess,
}

impl DatabaseConfig {
    pub fn file_name(&self) -> String {
        self.path.clone().unwrap_or_else(|| format!("{}.db", self.name))
    }

    /// Environment variable that carries the database path to services and functions.
    pub fn env_var(&self) -> String {
        format!("OPENSHELL_DB_{}", self.name.to_uppercase())
    }
}
