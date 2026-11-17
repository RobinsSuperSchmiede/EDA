//! Service monitoring menus: right-click on the window title bar (Windows), the tray
//! icon menu (all platforms) and the menu bar (macOS) all show the same live view of
//! the running services with actions (restart, stop, logs, open) and agent shortcuts.

use crate::{main_window, request_exit, show_main, show_runtime_panel, AppState};
use openshell_core::runtime::{ServiceState, ServiceStatus};
use tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

fn mb(bytes: u64) -> String {
    let m = bytes as f64 / 1_048_576.0;
    if m >= 1024.0 {
        format!("{:.1} GB", m / 1024.0)
    } else {
        format!("{m:.0} MB")
    }
}

fn uptime(st: &ServiceStatus) -> Option<String> {
    let started = st.started_at?;
    if !st.state.is_active() {
        return None;
    }
    let secs = (chrono_now() - started.timestamp()).max(0);
    Some(match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m {}s", s / 60, s % 60),
        s => format!("{}h {}m", s / 3600, (s % 3600) / 60),
    })
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn glyph(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Running => "\u{25cf}",                     // ●
        ServiceState::Unhealthy | ServiceState::Crashed | ServiceState::Failed => "\u{2716}", // ✖
        ServiceState::Stopped | ServiceState::Exited => "\u{25cb}", // ○
        _ => "\u{25d0}",                                          // ◐
    }
}

/// One line per service: `● backend — running · 0.4% · 31 MB`
fn service_label(st: &ServiceStatus) -> String {
    let mut label = format!("{}  {}  \u{2014}  {}", glyph(st.state), st.name, st.state);
    if let Some(m) = &st.metrics {
        label.push_str(&format!("  \u{00b7}  {:.1}% CPU  \u{00b7}  {}", m.cpu_percent, mb(m.memory_bytes)));
    }
    label
}

fn info(app: &AppHandle, text: String) -> tauri::Result<MenuItem<Wry>> {
    MenuItem::new(app, text, false, None::<&str>)
}

fn service_submenu(app: &AppHandle, st: &ServiceStatus) -> tauri::Result<Submenu<Wry>> {
    let sub = Submenu::new(app, service_label(st), true)?;
    let n = &st.name;
    // details
    let mut details = vec![format!("kind: {}", st.category.clone().unwrap_or_else(|| st.kind.clone()))];
    if let Some(pid) = st.pid {
        details.push(format!("pid: {pid}"));
    }
    if let Some(port) = st.port {
        details.push(format!("port: {port}"));
    }
    if let Some(m) = &st.metrics {
        details.push(format!("processes: {}", m.processes));
    }
    if let Some(u) = uptime(st) {
        details.push(format!("uptime: {u}"));
    }
    details.push(format!("restarts: {}", st.restarts));
    for d in details {
        sub.append(&info(app, d)?)?;
    }
    if let Some(e) = &st.last_error {
        sub.append(&info(app, format!("last error: {}", e.chars().take(80).collect::<String>()))?)?;
    }
    sub.append(&PredefinedMenuItem::separator(app)?)?;
    let active = st.state.is_active();
    sub.append(&MenuItem::with_id(app, format!("svc:restart:{n}"), "Restart", true, None::<&str>)?)?;
    if active {
        sub.append(&MenuItem::with_id(app, format!("svc:stop:{n}"), "Stop", true, None::<&str>)?)?;
    } else {
        sub.append(&MenuItem::with_id(app, format!("svc:start:{n}"), "Start", true, None::<&str>)?)?;
    }
    sub.append(&MenuItem::with_id(app, format!("svc:logs:{n}"), "Show logs\u{2026}", true, None::<&str>)?)?;
    if st.url.is_some() {
        sub.append(&MenuItem::with_id(app, format!("svc:open:{n}"), "Open in browser", true, None::<&str>)?)?;
        sub.append(&MenuItem::with_id(app, format!("svc:copyurl:{n}"), "Copy URL", true, None::<&str>)?)?;
    }
    Ok(sub)
}

/// Append the services section (header, one submenu per service, global actions).
fn append_services(app: &AppHandle, menu: &dyn MenuLike) -> tauri::Result<()> {
    let state = app.state::<AppState>();
    let Some(shell) = state.shell() else {
        menu.add(&info(app, "Runtime not running".into())?)?;
        return Ok(());
    };
    let statuses = shell.runtime().status();
    let healthy = statuses.iter().filter(|s| s.state == ServiceState::Running).count();
    let total_mem: u64 = statuses.iter().filter_map(|s| s.metrics.map(|m| m.memory_bytes)).sum();
    let total_cpu: f32 = statuses.iter().filter_map(|s| s.metrics.map(|m| m.cpu_percent)).sum();
    menu.add(&info(
        app,
        format!(
            "{} \u{00b7} {} mode \u{00b7} {healthy}/{} services healthy \u{00b7} {total_cpu:.1}% CPU \u{00b7} {}",
            shell.runtime().app_name(),
            shell.mode(),
            statuses.len(),
            mb(total_mem)
        ),
    )?)?;
    menu.add(&PredefinedMenuItem::separator(app)?)?;
    for st in &statuses {
        menu.add(&service_submenu(app, st)?)?;
    }
    menu.add(&PredefinedMenuItem::separator(app)?)?;
    menu.add(&MenuItem::with_id(app, "rt:restart-all", "Restart all services", true, None::<&str>)?)?;
    menu.add(&MenuItem::with_id(app, "rt:panel", "Runtime panel\u{2026}", true, None::<&str>)?)?;
    menu.add(&MenuItem::with_id(app, "rt:logs-folder", "Open logs folder", true, None::<&str>)?)?;

    let agents = Submenu::new(app, "AI agents", true)?;
    let pending = shell.approvals().pending().len();
    agents.append(&info(app, format!("{} page(s) bridged \u{00b7} {pending} pending approval(s)", shell.hub().pages().len()))?)?;
    agents.append(&MenuItem::with_id(app, "agent:copy-app", "Copy MCP command (use the app)", true, None::<&str>)?)?;
    agents.append(&MenuItem::with_id(app, "agent:copy-full", "Copy MCP command (develop the app)", true, None::<&str>)?)?;
    if pending > 0 {
        agents.append(&MenuItem::with_id(app, "rt:panel", "Review approvals\u{2026}", true, None::<&str>)?)?;
    }
    menu.add(&agents)?;
    Ok(())
}

/// Common interface of `Menu` and `Submenu` for `append`.
trait MenuLike {
    fn add(&self, item: &dyn IsMenuItem<Wry>) -> tauri::Result<()>;
}
impl MenuLike for Menu<Wry> {
    fn add(&self, item: &dyn IsMenuItem<Wry>) -> tauri::Result<()> {
        self.append(item)
    }
}
impl MenuLike for Submenu<Wry> {
    fn add(&self, item: &dyn IsMenuItem<Wry>) -> tauri::Result<()> {
        self.append(item)
    }
}

/// Context menu for a right-click on the title bar: services + window actions.
fn titlebar_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    append_services(app, &menu)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "win:reload", "Reload", true, None::<&str>)?)?;
    if dev_mode(app) {
        menu.append(&MenuItem::with_id(app, "win:devtools", "Developer tools", true, None::<&str>)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "win:minimize", "Minimize", true, None::<&str>)?)?;
    let maximized = main_window(app).and_then(|w| w.is_maximized().map_err(|e| e.to_string())).unwrap_or(false);
    menu.append(&MenuItem::with_id(app, "win:maximize", if maximized { "Restore" } else { "Maximize" }, true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "win:close", "Close", true, None::<&str>)?)?;
    Ok(menu)
}

fn dev_mode(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .shell()
        .map(|s| s.mode() == openshell_core::config::Mode::Development)
        .unwrap_or(false)
}

/// Show the title bar menu at the cursor.
pub fn show_titlebar_menu(app: &AppHandle) {
    match (titlebar_menu(app), main_window(app)) {
        (Ok(menu), Ok(w)) => {
            if let Err(e) = w.popup_menu(&menu) {
                tracing::warn!("cannot show menu: {e}");
            }
        }
        (Err(e), _) => tracing::warn!("cannot build menu: {e}"),
        _ => {}
    }
}

/// Tray icon menu.
pub fn tray_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(app, "win:show", "Show window", true, None::<&str>)?)?;
    menu.append(&MenuItem::with_id(app, "win:reload", "Reload frontend", true, None::<&str>)?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let services = Submenu::new(app, "Services", true)?;
    append_services(app, &services)?;
    menu.append(&services)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(app, "rt:quit", "Quit", true, None::<&str>)?)?;
    Ok(menu)
}

/// Rebuild the tray menu (and on macOS the menu bar) after state changes.
pub fn refresh(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("openshell") {
        if let Ok(menu) = tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(menu) = Menu::default(app) {
            if let Ok(services) = Submenu::new(app, "Services", true) {
                if append_services(app, &services).is_ok() && menu.append(&services).is_ok() {
                    let _ = app.set_menu(menu);
                }
            }
        }
    }
}

/// Handle every menu event (title bar, tray, menu bar).
pub fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let id = event.id().as_ref().to_string();
    let state = app.state::<AppState>();
    let shell = state.shell();
    let parts: Vec<&str> = id.splitn(3, ':').collect();
    match parts.as_slice() {
        ["win", "show"] => show_main(app),
        ["win", "reload"] => {
            if let Ok(w) = main_window(app) {
                let _ = w.eval("location.reload()");
            }
        }
        ["win", "devtools"] => {
            if let Ok(w) = main_window(app) {
                if w.is_devtools_open() {
                    w.close_devtools();
                } else {
                    w.open_devtools();
                }
            }
        }
        ["win", "minimize"] => {
            if let Ok(w) = main_window(app) {
                let _ = w.minimize();
            }
        }
        ["win", "maximize"] => {
            if let Ok(w) = main_window(app) {
                if w.is_maximized().unwrap_or(false) {
                    let _ = w.unmaximize();
                } else {
                    let _ = w.maximize();
                }
            }
        }
        ["win", "close"] => {
            if let Ok(w) = main_window(app) {
                let _ = w.close();
            }
        }
        ["rt", "quit"] => request_exit(app),
        ["rt", "panel"] => {
            let _ = show_runtime_panel(app);
        }
        ["rt", "restart-all"] => {
            if let Some(shell) = shell {
                for name in shell.runtime().service_names() {
                    let _ = shell.runtime().restart(&name);
                }
            }
        }
        ["rt", "logs-folder"] => {
            if let Some(shell) = shell {
                let dir = shell.project().state_dir().join("logs");
                let _ = app.opener().open_path(dir.display().to_string(), None::<&str>);
            }
        }
        ["agent", which] => {
            let cmds = state.mcp_commands.lock().unwrap().clone();
            let text = if *which == "copy-app" { cmds.0 } else { cmds.1 };
            let _ = app.clipboard().write_text(text);
        }
        ["svc", action, name] => {
            let Some(shell) = shell else { return };
            let rt = shell.runtime();
            match *action {
                "restart" => {
                    let _ = rt.restart(name);
                }
                "stop" => {
                    let _ = rt.stop(name);
                }
                "start" => {
                    let _ = rt.start(name);
                }
                "logs" => {
                    *state.panel_filter.lock().unwrap() = Some(name.to_string());
                    let _ = show_runtime_panel(app);
                }
                "open" => {
                    if let Some(url) = rt.service_url(name) {
                        let _ = app.opener().open_url(url, None::<&str>);
                    }
                }
                "copyurl" => {
                    if let Some(url) = rt.service_url(name) {
                        let _ = app.clipboard().write_text(url);
                    }
                }
                _ => {}
            }
        }
        _ => {}
    }
}

/// Windows: replace the system menu that appears on a right-click on the title bar
/// with the OpenShell services menu (Alt+Space still opens the system menu).
#[cfg(windows)]
pub mod titlebar {
    use std::sync::OnceLock;
    use tauri::AppHandle;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};

    const WM_NCRBUTTONUP: u32 = 0x00A5;
    const HTCAPTION: usize = 2;
    const SUBCLASS_ID: usize = 0x05E1_1A11;
    static APP: OnceLock<AppHandle> = OnceLock::new();

    unsafe extern "system" fn proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == WM_NCRBUTTONUP && wparam == HTCAPTION {
            if let Some(app) = APP.get() {
                let handle = app.clone();
                // build and show the menu outside of the window procedure
                let _ = app.run_on_main_thread(move || super::show_titlebar_menu(&handle));
                return 0;
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    pub fn install(app: &AppHandle, window: &tauri::WebviewWindow) {
        let _ = APP.set(app.clone());
        if let Ok(hwnd) = window.hwnd() {
            unsafe {
                SetWindowSubclass(hwnd.0 as HWND, Some(proc), SUBCLASS_ID, 0);
            }
        }
    }
}
