//! Terminal output helpers (colors respect `NO_COLOR`).

use std::sync::atomic::{AtomicBool, Ordering};

static COLOR: AtomicBool = AtomicBool::new(true);

pub fn enable_ansi() {
    let no_color = std::env::var_os("NO_COLOR").is_some();
    #[cfg(windows)]
    let ok = enable_windows_vt();
    #[cfg(not(windows))]
    let ok = true;
    COLOR.store(ok && !no_color, Ordering::Relaxed);
}

#[cfg(windows)]
fn enable_windows_vt() -> bool {
    // Windows 10+ terminals understand ANSI sequences once VT processing is on.
    // Use the console API through a tiny FFI declaration to avoid extra dependencies.
    type Handle = *mut core::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetStdHandle(n: u32) -> Handle;
        fn GetConsoleMode(h: Handle, mode: *mut u32) -> i32;
        fn SetConsoleMode(h: Handle, mode: u32) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    unsafe {
        let h = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;
        if GetConsoleMode(h, &mut mode) == 0 {
            // not a console (pipe / file): no colors
            return false;
        }
        SetConsoleMode(h, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

pub fn paint(code: &str, text: &str) -> String {
    if COLOR.load(Ordering::Relaxed) {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

pub fn bold(t: &str) -> String {
    paint("1", t)
}
pub fn dim(t: &str) -> String {
    paint("2", t)
}
pub fn green(t: &str) -> String {
    paint("32", t)
}
pub fn yellow(t: &str) -> String {
    paint("33", t)
}
pub fn red(t: &str) -> String {
    paint("31", t)
}
pub fn cyan(t: &str) -> String {
    paint("36", t)
}

pub fn error(msg: &str) {
    eprintln!("{} {msg}", red("error:"));
}

pub fn warn(msg: &str) {
    eprintln!("{} {msg}", yellow("warning:"));
}

pub fn ok(msg: &str) {
    println!("{} {msg}", green("✔"));
}

const SERVICE_COLORS: [&str; 6] = ["36", "35", "34", "33", "32", "96"];

/// Stable color per service name.
pub fn service_tag(name: &str, width: usize) -> String {
    let idx = name.bytes().map(|b| b as usize).sum::<usize>() % SERVICE_COLORS.len();
    paint(SERVICE_COLORS[idx], &format!("{name:>width$} │"))
}

/// Colored state, padded to `width` visible characters.
pub fn state_padded(s: &str, width: usize) -> String {
    let pad = " ".repeat(width.saturating_sub(s.chars().count()));
    format!("{}{pad}", state(s))
}

pub fn state(s: &str) -> String {
    match s {
        "running" => green(s),
        "starting" | "pending" | "setup" | "building" | "stopping" | "backoff" => yellow(s),
        "unhealthy" | "crashed" | "failed" => red(s),
        _ => dim(s),
    }
}
