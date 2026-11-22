//! OpenShell desktop shell (Tauri 2).
//!
//! The binary is generic: it contains no application logic. It loads an
//! `openshell.json`, starts the runtime and control API (`openshell-core`),
//! shows a loading/status page and navigates the main window to the
//! application frontend as soon as it is healthy.
//!
//! ```text
//! openshell-desktop [--project <dir>] [--mode development|production]   window (default)
//! openshell-desktop --headless [...]                                     no window, background runtime
//! openshell-desktop --mcp [--profile app|full] [...]                     MCP server on stdio for AI agents
//! ```
//!
//! Installed apps are a renamed copy of this binary with the application in `app/`
//! next to it, so `MyApp.exe --mcp` gives agents direct access to the installed app.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menus;

use openshell_core::agent::approvals::{ApprovalRequest, Approvals};
use openshell_core::config::{find_config_file, Mode, Project, TitleBar};
use openshell_core::runtime::{LogQuery, RuntimeEvent, ServiceState};
use openshell_core::state::ControlClient;
use openshell_core::stdio::McpProfile;
use openshell_core::{Host, Shell};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_notification::NotificationExt;

#[derive(Default)]
pub(crate) struct AppState {
    shell: Mutex<Option<Shell>>,
    error: Mutex<Option<String>>,
    app_name: Mutex<String>,
    hide_on_close: AtomicBool,
    shutting_down: AtomicBool,
    ready_to_exit: AtomicBool,
    /// Service preselected in the runtime panel (set by "Show logs…").
    pub(crate) panel_filter: Mutex<Option<String>>,
    /// MCP commands for agents: (use the app, develop the app).
    pub(crate) mcp_commands: Mutex<(String, String)>,
}

impl AppState {
    pub(crate) fn shell(&self) -> Option<Shell> {
        self.shell.lock().unwrap().clone()
    }
}

// ----------------------------------------------------------------------------
// Host implementation: what the runtime / agents can do with the desktop
// ----------------------------------------------------------------------------

struct DesktopHost {
    app: AppHandle,
}

pub(crate) fn main_window(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    app.get_webview_window("main").ok_or_else(|| "the main window is closed".to_string())
}

pub(crate) fn show_main(app: &AppHandle) {
    if let Ok(w) = main_window(app) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

impl Host for DesktopHost {
    fn kind(&self) -> &'static str {
        "desktop"
    }

    fn window(&self, action: &str) -> Result<(), String> {
        let w = main_window(&self.app)?;
        match action {
            "show" => w.show(),
            "hide" => w.hide(),
            "focus" => {
                show_main(&self.app);
                Ok(())
            }
            "minimize" => w.minimize(),
            "maximize" => w.maximize(),
            "unmaximize" => w.unmaximize(),
            other => return Err(format!("unknown window action `{other}`")),
        }
        .map_err(|e| e.to_string())
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), String> {
        self.app.notification().builder().title(title).body(body).show().map_err(|e| e.to_string())
    }

    fn reload_frontend(&self) -> Result<(), String> {
        open_frontend(&self.app)
    }

    fn approval_requested(&self, req: ApprovalRequest, approvals: Arc<Approvals>) {
        show_main(&self.app);
        let text = format!(
            "The agent \u{201c}{}\u{201d} wants to:\n\n{}\n\nTool: {}   Permission: {}\n\nThis request is denied automatically if it is not answered.",
            req.agent, req.summary, req.tool, req.permission
        );
        let id = req.id.clone();
        self.app
            .dialog()
            .message(text)
            .title("OpenShell \u{2013} allow agent action?")
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom("Allow".into(), "Deny".into()))
            .show(move |allowed| {
                approvals.resolve(&id, allowed);
            });
    }

    fn shutdown_requested(&self) {
        request_exit(&self.app);
    }
}

// ----------------------------------------------------------------------------
// Navigation helpers
// ----------------------------------------------------------------------------

/// URL of OpenShell's own pages (loading screen) for the current platform.
fn local_page(page: &str) -> String {
    if cfg!(windows) {
        format!("http://tauri.localhost/{page}")
    } else {
        format!("tauri://localhost/{page}")
    }
}

fn is_local_page(url: &tauri::Url) -> bool {
    url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost")
}

fn navigate(app: &AppHandle, url: &str) -> Result<(), String> {
    let w = main_window(app)?;
    let url: tauri::Url = url.parse().map_err(|e| format!("invalid url {url}: {e}"))?;
    w.navigate(url).map_err(|e| e.to_string())
}

fn open_frontend(app: &AppHandle) -> Result<(), String> {
    let shell = app.state::<AppState>().shell().ok_or("the runtime is not running")?;
    let url = shell.runtime().frontend_url().ok_or("no frontend is configured in openshell.json")?;
    navigate(app, &url)
}

/// Follow runtime events: show the app when the frontend is ready, reload on
/// file changes, fall back to the status page when the frontend fails and keep
/// the tray / menu bar service menus current.
fn watch_runtime(app: AppHandle, shell: Shell) {
    tauri::async_runtime::spawn(async move {
        let rt = shell.runtime().clone();
        let mut events = rt.subscribe();
        if rt.has_service("frontend") {
            let app2 = app.clone();
            let rt2 = rt.clone();
            tauri::async_runtime::spawn(async move {
                if rt2.wait_ready("frontend", Duration::from_secs(3600)).await.is_ok() {
                    let _ = open_frontend(&app2);
                }
            });
        }
        loop {
            match events.recv().await {
                Ok(RuntimeEvent::FrontendReload) => {
                    if let Ok(w) = main_window(&app) {
                        let _ = w.eval("location.reload()");
                    }
                }
                Ok(RuntimeEvent::ServiceState { service, state, .. }) => {
                    let a = app.clone();
                    let _ = app.run_on_main_thread(move || menus::refresh(&a));
                    if service != "frontend" {
                        continue;
                    }
                    match state {
                        ServiceState::Running => {
                            let on_status_page = main_window(&app)
                                .ok()
                                .and_then(|w| w.url().ok())
                                .map(|u| is_local_page(&u))
                                .unwrap_or(false);
                            if on_status_page {
                                let _ = open_frontend(&app);
                            }
                        }
                        ServiceState::Failed | ServiceState::Crashed => {
                            let _ = navigate(&app, &local_page("index.html"));
                        }
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
}

// ----------------------------------------------------------------------------
// Shutdown
// ----------------------------------------------------------------------------

/// Stop all services, then exit. Safe to call multiple times.
pub(crate) fn request_exit(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.shutting_down.swap(true, Ordering::SeqCst) {
        return;
    }
    for w in app.webview_windows().values() {
        let _ = w.hide();
    }
    let shell = state.shell();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(shell) = shell {
            shell.shutdown().await;
        }
        app.state::<AppState>().ready_to_exit.store(true, Ordering::SeqCst);
        app.exit(0);
    });
}

// ----------------------------------------------------------------------------
// Commands for OpenShell's own pages (loading screen, runtime panel)
// ----------------------------------------------------------------------------

#[tauri::command]
fn get_state(state: State<'_, AppState>, since: Option<u64>) -> Value {
    let error = state.error.lock().unwrap().clone();
    let name = state.app_name.lock().unwrap().clone();
    let focus = state.panel_filter.lock().unwrap().take();
    let Some(shell) = state.shell() else {
        return json!({ "app": name, "error": error });
    };
    let rt = shell.runtime();
    json!({
        "app": rt.app_name(),
        "mode": shell.mode(),
        "error": error,
        "control_url": shell.control_url(),
        "frontend_url": rt.frontend_url(),
        "project_root": shell.project().root,
        "services": rt.status(),
        "logs": rt.logs().query(&LogQuery { since, limit: Some(300), ..Default::default() }),
        "last_seq": rt.logs().last_seq(),
        "approvals": shell.approvals().pending(),
        "pages": shell.hub().pages().len(),
        "focus_service": focus,
        "mcp_commands": *state.mcp_commands.lock().unwrap(),
        "has_frontend": rt.has_service("frontend"),
        "databases": shell.databases().list(),
        "capabilities": shell.capabilities_summary(),
    })
}

/// Rows of a database table (console data browser).
#[tauri::command]
fn db_rows(
    state: State<'_, AppState>,
    database: String,
    table: String,
    limit: Option<u32>,
) -> Result<Value, String> {
    let shell = state.shell().ok_or("the runtime is not running")?;
    let db = shell.databases().get(Some(&database))?;
    let quoted = format!("\"{}\"", table.replace('"', "\"\""));
    db.query(&format!("SELECT * FROM {quoted} LIMIT {}", limit.unwrap_or(100).min(1000)), &Value::Null)
}

/// Run an app function from the console (as the user: full access, audited as `console`).
#[tauri::command]
async fn call_function(state: State<'_, AppState>, name: String, input: Value) -> Result<Value, String> {
    let shell = state.shell().ok_or("the runtime is not running")?;
    let policy = openshell_core::agent::policy::Policy::admin(shell.mode());
    let ctx = openshell_core::agent::mcp::CallContext { agent: "console".into(), channel: "console".into() };
    let outcome = shell.call_tool(&format!("cap_{name}"), input, ctx, &policy).await;
    if outcome.ok {
        Ok(outcome.result)
    } else {
        Err(outcome.error_message().unwrap_or_else(|| "failed".into()))
    }
}

#[tauri::command]
fn service_action(state: State<'_, AppState>, name: String, action: String) -> Result<(), String> {
    let shell = state.shell().ok_or("the runtime is not running")?;
    let rt = shell.runtime();
    match action.as_str() {
        "start" => rt.start(&name),
        "stop" => rt.stop(&name),
        "restart" => rt.restart(&name),
        other => return Err(format!("unknown action `{other}`")),
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn open_app(app: AppHandle) -> Result<(), String> {
    open_frontend(&app)
}

#[tauri::command]
fn resolve_approval(state: State<'_, AppState>, id: String, approve: bool) -> bool {
    state.shell().map(|s| s.approvals().resolve(&id, approve)).unwrap_or(false)
}

/// The title bar menu, for frontends that draw their own title bar (`window.title_bar: "overlay"`).
#[tauri::command]
fn show_window_menu(app: AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || menus::show_titlebar_menu(&handle));
}

#[tauri::command]
fn open_runtime_panel(app: AppHandle) -> Result<(), String> {
    show_runtime_panel(&app).map_err(|e| e.to_string())
}

pub(crate) fn show_runtime_panel(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("runtime") {
        w.show()?;
        w.unminimize()?;
        w.set_focus()?;
        return Ok(());
    }
    let name = app.state::<AppState>().app_name.lock().unwrap().clone();
    WebviewWindowBuilder::new(app, "runtime", WebviewUrl::App("runtime.html".into()))
        .title(format!("{name} \u{2013} Runtime"))
        .inner_size(1000.0, 680.0)
        .build()?;
    Ok(())
}

// ----------------------------------------------------------------------------
// Startup
// ----------------------------------------------------------------------------

#[derive(Default)]
struct Args {
    project: Option<PathBuf>,
    mode: Option<Mode>,
    headless: bool,
    mcp: bool,
    profile: Option<McpProfile>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--project" | "-p" => args.project = it.next().map(PathBuf::from),
            "--mode" | "-m" => args.mode = Some(it.next().unwrap_or_default().parse()?),
            "--dev" => args.mode = Some(Mode::Development),
            "--headless" => args.headless = true,
            "--mcp" => args.mcp = true,
            "--profile" => {
                args.profile = Some(match it.next().as_deref() {
                    Some("full") => McpProfile::Full,
                    Some("app") => McpProfile::App,
                    other => return Err(format!("unknown profile {other:?} (app|full)")),
                })
            }
            "--help" | "-h" => {
                println!(
                    "openshell-desktop [--project <dir>] [--mode development|production] [--headless] [--mcp [--profile app|full]]"
                );
                std::process::exit(0);
            }
            other if other.starts_with('-') => return Err(format!("unknown argument `{other}`")),
            other => args.project = Some(PathBuf::from(other)),
        }
    }
    Ok(args)
}

/// Find the application: explicit argument, `OPENSHELL_PROJECT`, next to the
/// executable (installed apps: `<exe dir>/app/openshell.json`) or the working
/// directory. Installed apps keep their state in a per-user directory.
fn resolve_project(args: &Args) -> Result<(Project, bool), String> {
    if let Some(p) = &args.project {
        return Project::load(p).map(|p| (p, false)).map_err(|e| e.to_string());
    }
    if let Ok(p) = std::env::var("OPENSHELL_PROJECT") {
        return Project::load(p).map(|p| (p, false)).map_err(|e| e.to_string());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for candidate in [
                dir.join("app"),
                dir.join("../Resources/app"),
                dir.join("../lib").join(exe_stem()).join("app"),
            ] {
                if find_config_file(&candidate).is_some() {
                    let project = Project::load(&candidate).map_err(|e| e.to_string())?;
                    let state = dirs::data_local_dir()
                        .unwrap_or_else(std::env::temp_dir)
                        .join(project.config.identifier());
                    return Ok((project.with_state_dir(state), true));
                }
            }
        }
    }
    Project::discover(".").map(|p| (p, false)).map_err(|e| e.to_string())
}

fn exe_stem() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.file_stem().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_default()
}

/// Commands an agent uses to connect: (use the app, develop the app).
fn mcp_commands(project: &Project, packaged: bool) -> (String, String) {
    let exe = std::env::current_exe().map(|e| e.display().to_string()).unwrap_or_default();
    if packaged {
        (format!("\"{exe}\" --mcp"), format!("\"{exe}\" --mcp --profile full"))
    } else {
        let root = project.root.display();
        (
            format!("openshell mcp --app --start --project \"{root}\""),
            format!("openshell mcp --project \"{root}\""),
        )
    }
}

fn default_mode(args: &Args) -> Mode {
    args.mode.unwrap_or_else(|| {
        std::env::var("OPENSHELL_MODE").ok().and_then(|m| m.parse().ok()).unwrap_or(Mode::Production)
    })
}

/// Another instance of this app is running: hand over or stop it.
/// Returns `true` if this process should exit (a desktop instance was focused).
fn handle_running_instance(project: &Project) -> bool {
    tauri::async_runtime::block_on(async {
        let Ok(client) = ControlClient::connect(&project.state_dir()).await else {
            return false;
        };
        if client.info.host == "desktop" {
            let _ = client.post("/v1/tools/window_control", &json!({ "action": "focus" })).await;
            return true;
        }
        // a headless instance (e.g. started for an agent): replace it with the window
        let _ = client.post("/v1/shutdown", &json!({})).await;
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if client.get("/v1/health").await.is_err() {
                break;
            }
        }
        false
    })
}

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let state = app.state::<AppState>();
    let args = parse_args();
    let resolved = args.as_ref().map_err(|e| e.clone()).and_then(resolve_project);

    let mut builder = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()));
    let (project, packaged) = match resolved {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("{e}");
            *state.error.lock().unwrap() = Some(e);
            *state.app_name.lock().unwrap() = "OpenShell".into();
            builder.title("OpenShell").inner_size(900.0, 600.0).build()?;
            return Ok(());
        }
    };
    if handle_running_instance(&project) {
        std::process::exit(0);
    }
    let mode = args.as_ref().map(default_mode).unwrap_or(Mode::Production);
    let cfg = project.config.clone();
    *state.app_name.lock().unwrap() = cfg.app.name.clone();
    *state.mcp_commands.lock().unwrap() = mcp_commands(&project, packaged);
    // without a window (window.enabled = false) the app lives in the tray; closing only hides
    state.hide_on_close.store(
        cfg.window.close_to_tray || cfg.runtime.background_mode || !cfg.window.enabled,
        Ordering::SeqCst,
    );
    if packaged {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = handle.autolaunch();
        let result = if cfg.runtime.launch_at_login { autolaunch.enable() } else { autolaunch.disable() };
        if let Err(e) = result {
            tracing::debug!("launch at login: {e}");
        }
    }
    let icon_path = cfg.app.icon.as_ref().map(|i| project.resolve_path(i));

    let host = Arc::new(DesktopHost { app: handle.clone() });
    let shell = match tauri::async_runtime::block_on(Shell::launch(project, mode, host)) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("{e:#}");
            tracing::error!("{msg}");
            *state.error.lock().unwrap() = Some(msg);
            builder.title(cfg.window_title()).inner_size(900.0, 600.0).build()?;
            return Ok(());
        }
    };

    let w = &cfg.window;
    builder = builder
        .title(cfg.window_title())
        .inner_size(w.width, w.height)
        .resizable(w.resizable)
        .maximized(w.maximized)
        .devtools(w.devtools.unwrap_or(mode == Mode::Development))
        .visible(w.enabled)
        .initialization_script(shell.webview_init_script())
        // Tauri's own drag and drop handler (file drops as Tauri events) swallows the HTML5 drag events
        // on Windows: `dragover` and `drop` never reach the page, so dragging inside the app and dropping
        // files on it fail. Without it the WebView handles both like a browser does.
        .disable_drag_drop_handler();
    if let (Some(mw), Some(mh)) = (w.min_width, w.min_height) {
        builder = builder.min_inner_size(mw, mh);
    }
    let overlay = w.title_bar == TitleBar::Overlay;
    if overlay {
        // The frontend draws the title bar. macOS keeps its window buttons over the content; on
        // Windows and Linux the system title bar goes (edges still resize, Windows keeps the shadow).
        #[cfg(target_os = "macos")]
        {
            builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay).hidden_title(true);
        }
        #[cfg(not(target_os = "macos"))]
        {
            builder = builder.decorations(false).shadow(true);
        }
        if let Some(url) = shell.runtime().frontend_url() {
            allow_window_controls(&handle, &url);
        }
    }
    let window = builder.build()?;
    if let Some(icon) = icon_path.filter(|p| p.is_file()) {
        match tauri::image::Image::from_path(&icon) {
            Ok(img) => {
                let _ = window.set_icon(img);
            }
            Err(e) => tracing::warn!("cannot load icon {}: {e}", icon.display()),
        }
    }
    #[cfg(windows)]
    if !overlay {
        menus::titlebar::install(&handle, &window);
    }

    // Apps without a frontend (backend, database or functions only) show OpenShell's console.
    if !shell.runtime().has_service("frontend") {
        let _ = navigate(&handle, &local_page("console.html"));
    }

    *state.shell.lock().unwrap() = Some(shell.clone());
    // a window-less app needs the tray to be reachable at all
    if w.tray || !w.enabled {
        if let Err(e) = build_tray(&handle, &cfg.app.name) {
            tracing::warn!("tray icon unavailable: {e}");
        }
    }
    menus::refresh(&handle);
    shell.start();
    watch_runtime(handle, shell);
    Ok(())
}

/// Window permissions an overlay title bar needs, granted to the app's frontend origin only (pages
/// served from elsewhere get no IPC access). OpenShell's own pages have them in capabilities/default.json.
const WINDOW_CONTROLS: &[&str] = &[
    "core:window:allow-start-dragging",
    "core:window:allow-internal-toggle-maximize",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-close",
    "core:window:allow-is-maximized",
    "allow-show-window-menu",
];

fn allow_window_controls(app: &AppHandle, frontend_url: &str) {
    let Ok(url) = tauri::Url::parse(frontend_url) else { return };
    let (Some(host), Some(port)) = (url.host_str(), url.port_or_known_default()) else { return };
    // localhost and 127.0.0.1 are the same server; a frontend may redirect from one to the other.
    let hosts: Vec<&str> = match host {
        "localhost" | "127.0.0.1" => vec!["localhost", "127.0.0.1"],
        other => vec![other],
    };
    let mut capability =
        tauri::ipc::CapabilityBuilder::new("openshell-title-bar").window("main").local(false);
    for host in hosts {
        capability = capability.remote(format!("{}://{host}:{port}/*", url.scheme()));
    }
    for permission in WINDOW_CONTROLS {
        capability = capability.permission(*permission);
    }
    if let Err(e) = app.add_capability(capability) {
        tracing::warn!("overlay title bar: cannot allow the window controls for {frontend_url}: {e}");
    }
}

fn build_tray(app: &AppHandle, name: &str) -> tauri::Result<()> {
    let mut builder = TrayIconBuilder::with_id("openshell")
        .tooltip(name)
        .menu(&menus::tray_menu(app)?)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// `--mcp`: MCP server on stdio. Starts a headless instance of the app on demand.
fn run_mcp(args: &Args) -> ! {
    let code = match resolve_project(args) {
        Err(e) => {
            eprintln!("openshell: {e}");
            1
        }
        Ok((project, packaged)) => {
            let profile = args.profile.unwrap_or(McpProfile::App);
            let mut start_args: Vec<String> = vec!["--headless".into()];
            if !packaged {
                start_args.push("--project".into());
                start_args.push(project.root.display().to_string());
            }
            let mode = args.mode.unwrap_or(if profile == McpProfile::App {
                Mode::Production
            } else {
                Mode::Development
            });
            start_args.push("--mode".into());
            start_args.push(mode.to_string());
            let autostart: openshell_core::stdio::Autostart = Box::new(move || {
                openshell_core::headless::spawn_detached(&std::env::current_exe()?, &start_args).map(|_| ())
            });
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            match rt.block_on(openshell_core::stdio::serve(
                Some(project.state_dir()),
                profile,
                Some(autostart),
            )) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("openshell: {e:#}");
                    1
                }
            }
        }
    };
    std::process::exit(code)
}

/// `--headless`: run the services without a window until stopped.
fn run_headless(args: &Args) -> ! {
    let code = match resolve_project(args) {
        Err(e) => {
            eprintln!("openshell: {e}");
            1
        }
        Ok((project, _)) => {
            let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
            match rt.block_on(openshell_core::headless::run(project, default_mode(args))) {
                Ok(()) => 0,
                Err(e) => {
                    eprintln!("openshell: {e:#}");
                    1
                }
            }
        }
    };
    std::process::exit(code)
}

fn main() {
    let filter = std::env::var("OPENSHELL_LOG").unwrap_or_else(|_| "warn".into());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    if let Ok(args) = parse_args() {
        if args.mcp {
            run_mcp(&args);
        }
        if args.headless {
            run_headless(&args);
        }
    }

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            get_state,
            service_action,
            open_app,
            resolve_approval,
            open_runtime_panel,
            show_window_menu,
            db_rows,
            call_function
        ])
        .on_menu_event(menus::on_menu_event)
        .setup(setup)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    return;
                }
                let app = window.app_handle();
                let state = app.state::<AppState>();
                api.prevent_close();
                if state.hide_on_close.load(Ordering::SeqCst) && !state.shutting_down.load(Ordering::SeqCst) {
                    let _ = window.hide();
                } else {
                    request_exit(app);
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("failed to build the OpenShell desktop app");

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let state = app.state::<AppState>();
            if !state.ready_to_exit.load(Ordering::SeqCst) {
                api.prevent_exit();
                request_exit(app);
            }
        }
    });
}
