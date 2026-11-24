//! Platform adapters.
//!
//! The runtime, the agent layer and the control API are the same on every platform. What
//! differs is *how* things are started and how long they may live:
//!
//! * **Desktop / server** – any program can be started, process trees are killed as a whole,
//!   services run as long as the shell runs.
//! * **Android app** – apps may only execute binaries shipped inside the APK (the native library
//!   directory, "W^X" since Android 10), there is no `PATH` with toolchains, and the system
//!   stops background work unless a foreground service keeps the app alive.
//! * **Android in Termux** – a terminal environment with its own package manager: Python,
//!   Node.js, Java, Rust, Go, C/C++ and databases install with `pkg install` and run like on
//!   Linux. The phone becomes a development machine and a portable host.
//!
//! A [`RuntimeAdapter`] encapsulates these differences. The runtime asks the adapter to build
//! every command (services, setup and build steps, functions, tasks) and reports the
//! adapter's [`Capabilities`] to hosts, the CLI and agents, so an agent building an app on a
//! phone knows which building blocks are available there.

use crate::process;
use anyhow::{anyhow, bail, Result};
use openshell_config::{CommandSpec, Platform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;

/// Which programs a platform can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessSupport {
    /// Any program found in `PATH` or the project (desktop, server).
    Full,
    /// Only executables shipped with the app (Android: interpreters and binaries in the APK).
    Restricted,
    /// No child processes at all.
    None,
}

/// How long services may run when nobody looks at the app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundModel {
    /// Services run as long as the shell runs (desktop, server).
    Always,
    /// The OS stops background work unless the host keeps a foreground service (Android).
    ForegroundService,
}

/// What the current platform offers. Exposed in `/v1/status` and to agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    pub platform: Platform,
    /// `windows`, `macos`, `linux`, `android`, ...
    pub os: String,
    pub arch: String,
    pub processes: ProcessSupport,
    pub background: BackgroundModel,
    /// Hot reload / restart on file changes.
    pub file_watching: bool,
    /// The frontend is shown in an app window (WebView).
    pub app_window: bool,
    /// Tray icon and title-bar service menu.
    pub tray: bool,
    /// `openshell package` can build installers on this platform.
    pub installers: bool,
    /// Language toolchains and package managers can run (build steps, dev servers, `npm`,
    /// `pip`, `cargo` …): desktop, server and Termux, not the Android app.
    #[serde(default)]
    pub toolchains: bool,
    /// The host can keep the device awake while services must run (Termux wake lock,
    /// Android foreground service).
    #[serde(default)]
    pub wake_lock: bool,
}

/// Platform specific part of the runtime. Implemented for desktop/server and Android; a host
/// can provide its own implementation (for example with more bundled interpreters).
pub trait RuntimeAdapter: Send + Sync + 'static {
    fn capabilities(&self) -> Capabilities;

    fn platform(&self) -> Platform {
        self.capabilities().platform
    }

    /// Build a command ready to spawn (stdin closed, stdout/stderr piped, killed on drop).
    fn command(
        &self,
        spec: &CommandSpec,
        extra_args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Command>;

    /// Whether the program of a command can be started here (`openshell doctor`, validation).
    fn program_available(&self, spec: &CommandSpec, cwd: &Path) -> bool;

    /// How to install a missing program on this platform (`openshell doctor`).
    fn install_hint(&self, _program: &str) -> Option<String> {
        None
    }

    /// Keep the device awake while services must run (`true`) or release it (`false`).
    /// Hosts call this when the runtime starts and stops; a no-op where not needed.
    fn keep_awake(&self, _on: bool) {}
}

/// The adapter for the platform this binary was built for. On Android: the Termux adapter
/// inside Termux, the app adapter otherwise; `OPENSHELL_ANDROID_MODE=app|termux` overrides.
pub fn default_adapter() -> Arc<dyn RuntimeAdapter> {
    #[cfg(target_os = "android")]
    {
        let mode = std::env::var("OPENSHELL_ANDROID_MODE").unwrap_or_default();
        if mode != "app" {
            if let Some(t) = TermuxAdapter::detect(
                std::env::var("PREFIX").ok().as_deref(),
                std::env::var("TERMUX_VERSION").ok().as_deref(),
                mode == "termux",
            ) {
                return Arc::new(t);
            }
        }
        Arc::new(AndroidAdapter::from_env())
    }
    #[cfg(not(target_os = "android"))]
    {
        Arc::new(DesktopAdapter::desktop())
    }
}

// --------------------------------------------------------------------------- desktop / server

/// Windows, macOS and Linux: every program can be started, process trees are managed with
/// job objects (Windows) or process groups (Unix).
pub struct DesktopAdapter {
    platform: Platform,
}

impl DesktopAdapter {
    pub fn desktop() -> Self {
        Self { platform: Platform::Desktop }
    }
    /// A machine without a user session (no window, no tray, always on).
    pub fn server() -> Self {
        Self { platform: Platform::Server }
    }
}

impl Default for DesktopAdapter {
    fn default() -> Self {
        Self::desktop()
    }
}

impl RuntimeAdapter for DesktopAdapter {
    fn capabilities(&self) -> Capabilities {
        let desktop = self.platform == Platform::Desktop;
        Capabilities {
            platform: self.platform,
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
            processes: ProcessSupport::Full,
            background: BackgroundModel::Always,
            file_watching: true,
            app_window: desktop,
            tray: desktop,
            installers: desktop,
            toolchains: true,
            wake_lock: false,
        }
    }

    fn command(
        &self,
        spec: &CommandSpec,
        extra_args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Command> {
        process::build_command(spec, extra_args, cwd, env)
    }

    fn program_available(&self, spec: &CommandSpec, cwd: &Path) -> bool {
        process::program_exists(spec, cwd)
    }
}

// --------------------------------------------------------------------------- android

/// Android: only executables shipped with the app may run.
///
/// The Android host registers what the APK contains – directories with executables (the
/// native library directory) and named programs such as `node` → `.../libnode.so` – and the
/// adapter maps commands from `openshell.json` onto them. Everything else is rejected with an
/// error that tells the developer (or the agent) which building blocks work instead: static
/// frontends, the built-in database and functions run by a bundled interpreter.
#[derive(Debug, Clone)]
pub struct AndroidAdapter {
    executable_dirs: Vec<PathBuf>,
    programs: BTreeMap<String, PathBuf>,
    shell: PathBuf,
}

impl Default for AndroidAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl AndroidAdapter {
    /// An adapter that allows the system tools in `/system/bin` (`sh`, `cat`, `echo`, ...),
    /// which every Android app may execute.
    pub fn new() -> Self {
        Self {
            executable_dirs: vec![PathBuf::from("/system/bin")],
            programs: BTreeMap::new(),
            shell: PathBuf::from("/system/bin/sh"),
        }
    }

    /// Configuration passed by the Android host through the environment:
    ///
    /// * `OPENSHELL_ANDROID_EXEC_DIRS` – directories with executables, separated by `:`
    /// * `OPENSHELL_ANDROID_PROGRAMS` – named programs, `node=/path/libnode.so;python=/path/libpython.so`
    ///
    /// Inside Termux (`PREFIX=/data/data/com.termux/files/usr`) its `bin` directory is allowed
    /// as well, so `pkg install nodejs python` makes those runtimes available.
    pub fn from_env() -> Self {
        let mut a = Self::new();
        if let Ok(prefix) = std::env::var("PREFIX") {
            if prefix.contains("com.termux") {
                a = a.with_executable_dir(Path::new(&prefix).join("bin"));
            }
        }
        if let Ok(dirs) = std::env::var("OPENSHELL_ANDROID_EXEC_DIRS") {
            for d in dirs.split(':').filter(|d| !d.is_empty()) {
                a = a.with_executable_dir(d);
            }
        }
        if let Ok(programs) = std::env::var("OPENSHELL_ANDROID_PROGRAMS") {
            for entry in programs.split(';') {
                if let Some((name, path)) = entry.split_once('=') {
                    a = a.with_program(name.trim(), path.trim());
                }
            }
        }
        a
    }

    /// Allow every executable inside `dir` (typically `ApplicationInfo.nativeLibraryDir`).
    pub fn with_executable_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.executable_dirs.push(dir.into());
        self
    }

    /// Make a bundled program available under a name, e.g. `node` → `.../libnode.so`.
    pub fn with_program(mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.programs.insert(name.into(), path.into());
        self
    }

    /// Programs available by name (for diagnostics and agents).
    pub fn programs(&self) -> impl Iterator<Item = &str> {
        self.programs.keys().map(String::as_str)
    }

    fn allowed(&self, path: &Path) -> bool {
        self.executable_dirs.iter().any(|d| path.starts_with(d)) || self.programs.values().any(|p| p == path)
    }

    /// Map a program name from the config onto something the app may execute.
    fn resolve(&self, program: &str, cwd: &Path) -> Result<PathBuf> {
        // `python3` and `python` are interchangeable, as on the desktop
        let alias = match program {
            "python" => Some("python3"),
            "python3" => Some("python"),
            _ => None,
        };
        for name in std::iter::once(program).chain(alias) {
            if let Some(p) = self.programs.get(name) {
                return Ok(p.clone());
            }
        }
        let candidate = if program.contains('/') {
            let p = Path::new(program);
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            }
        } else {
            match self.executable_dirs.iter().map(|d| d.join(program)).find(|p| p.is_file()) {
                Some(p) => p,
                None => bail!("{}", self.unavailable(program)),
            }
        };
        if self.allowed(&candidate) {
            Ok(candidate)
        } else {
            bail!("{}", self.unavailable(program))
        }
    }

    fn unavailable(&self, program: &str) -> String {
        let bundled = if self.programs.is_empty() {
            "none".to_string()
        } else {
            self.programs.keys().cloned().collect::<Vec<_>>().join(", ")
        };
        format!(
            "`{program}` cannot run on Android: apps may only start system tools and programs shipped \
             with the app (bundled: {bundled}). Use a static frontend, the built-in database or functions for a \
             bundled interpreter, or limit this service with `\"platforms\": [\"desktop\", \"server\"]`"
        )
    }
}

// --------------------------------------------------------------------------- termux

/// Android inside Termux: a Linux-like environment where packages from `pkg install` (Python,
/// Node.js, OpenJDK, Rust, Go, Clang, PostgreSQL, Redis …) live in `$PREFIX/bin` and may run.
/// Projects are created, developed and run on the phone like on a desktop; only installers and
/// app windows are missing (the frontend opens in the phone's browser).
#[derive(Debug, Clone)]
pub struct TermuxAdapter {
    prefix: PathBuf,
}

/// Termux packages that provide a program (for `openshell doctor` hints).
const TERMUX_PACKAGES: &[(&str, &str)] = &[
    ("python", "python"),
    ("python3", "python"),
    ("pip", "python"),
    ("node", "nodejs"),
    ("npm", "nodejs"),
    ("npx", "nodejs"),
    ("java", "openjdk-17"),
    ("javac", "openjdk-17"),
    ("mvn", "maven"),
    ("gradle", "gradle"),
    ("cargo", "rust"),
    ("rustc", "rust"),
    ("go", "golang"),
    ("gcc", "clang"),
    ("cc", "clang"),
    ("clang", "clang"),
    ("g++", "clang"),
    ("make", "make"),
    ("cmake", "cmake"),
    ("git", "git"),
    ("php", "php"),
    ("ruby", "ruby"),
    ("deno", "deno"),
    ("bun", "bun"),
    ("dotnet", "dotnet8.0"),
    ("postgres", "postgresql"),
    ("psql", "postgresql"),
    ("redis-server", "redis"),
    ("mariadbd", "mariadb"),
    ("mysqld", "mariadb"),
    ("mongod", "mongodb"),
    ("nginx", "nginx"),
    ("sqlite3", "sqlite"),
];

impl TermuxAdapter {
    pub fn new(prefix: impl Into<PathBuf>) -> Self {
        Self { prefix: prefix.into() }
    }

    /// Detect Termux from `$PREFIX` (`/data/data/com.termux/files/usr`) or `$TERMUX_VERSION`;
    /// `force` accepts any existing prefix (other Termux-like environments).
    pub fn detect(prefix: Option<&str>, termux_version: Option<&str>, force: bool) -> Option<Self> {
        let prefix = prefix.filter(|p| !p.is_empty())?;
        let termux = prefix.contains("com.termux") || termux_version.is_some_and(|v| !v.is_empty());
        (termux || force).then(|| Self::new(prefix))
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    fn bin(&self) -> PathBuf {
        self.prefix.join("bin")
    }

    /// `$PREFIX/bin` first, then the inherited `PATH`.
    fn path_env(&self, env: &BTreeMap<String, String>) -> String {
        let inherited = env.get("PATH").cloned().or_else(|| std::env::var("PATH").ok()).unwrap_or_default();
        let bin = self.bin().display().to_string();
        if inherited.split(':').any(|p| p == bin) {
            inherited
        } else if inherited.is_empty() {
            format!("{bin}:/system/bin")
        } else {
            format!("{bin}:{inherited}")
        }
    }

    fn find(&self, program: &str, cwd: &Path, path: &str) -> Option<PathBuf> {
        if program.contains('/') {
            let p = Path::new(program);
            let p = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
            return p.is_file().then_some(p);
        }
        let names: &[&str] = match program {
            "python" => &["python", "python3"],
            "python3" => &["python3", "python"],
            _ => &[],
        };
        let names: Vec<&str> = if names.is_empty() { vec![program] } else { names.to_vec() };
        for name in names {
            for dir in path.split(':').filter(|d| !d.is_empty()) {
                let candidate = Path::new(dir).join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn missing(&self, program: &str) -> String {
        match self.install_hint(program) {
            Some(hint) => format!("`{program}` is not installed in Termux – install it with `{hint}`"),
            None => format!("`{program}` was not found in Termux ($PREFIX/bin or PATH)"),
        }
    }
}

impl RuntimeAdapter for TermuxAdapter {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            platform: Platform::Android,
            os: "android".into(),
            arch: std::env::consts::ARCH.into(),
            processes: ProcessSupport::Full,
            background: BackgroundModel::ForegroundService,
            file_watching: true,
            app_window: false,
            tray: false,
            installers: false,
            toolchains: true,
            wake_lock: true,
        }
    }

    fn command(
        &self,
        spec: &CommandSpec,
        extra_args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Command> {
        let path = self.path_env(env);
        let mut cmd = match spec {
            CommandSpec::Shell(line) => {
                let mut full = line.clone();
                for a in extra_args {
                    full.push(' ');
                    full.push_str(&posix_quote(a));
                }
                let sh = self.bin().join("sh");
                let mut c = Command::new(if sh.is_file() { sh } else { PathBuf::from("/system/bin/sh") });
                c.arg("-c").arg(full);
                c
            }
            CommandSpec::Argv(argv) => {
                let program = argv.first().ok_or_else(|| anyhow!("empty command"))?;
                let resolved =
                    self.find(program, cwd, &path).ok_or_else(|| anyhow!("{}", self.missing(program)))?;
                let mut c = Command::new(resolved);
                c.args(&argv[1..]).args(extra_args);
                c
            }
        };
        cmd.current_dir(cwd)
            .envs(env)
            .env("PATH", path)
            .env("PREFIX", &self.prefix)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        Ok(cmd)
    }

    fn program_available(&self, spec: &CommandSpec, cwd: &Path) -> bool {
        match spec {
            CommandSpec::Shell(line) if plain_words(line).is_none() => true,
            _ => spec
                .program()
                .map(|p| self.find(&p, cwd, &self.path_env(&BTreeMap::new())).is_some())
                .unwrap_or(false),
        }
    }

    fn install_hint(&self, program: &str) -> Option<String> {
        TERMUX_PACKAGES.iter().find(|(p, _)| *p == program).map(|(_, pkg)| format!("pkg install {pkg}"))
    }

    fn keep_awake(&self, on: bool) {
        let tool = self.bin().join(if on { "termux-wake-lock" } else { "termux-wake-unlock" });
        if tool.is_file() {
            let _ = std::process::Command::new(tool)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// Split a shell line into words if it is a plain command without shell syntax.
fn plain_words(line: &str) -> Option<Vec<String>> {
    if line.chars().any(|c| "|&;<>()$`\"'\\*?~{}[]#=\n".contains(c)) {
        return None;
    }
    let words: Vec<String> = line.split_whitespace().map(str::to_string).collect();
    (!words.is_empty()).then_some(words)
}

fn posix_quote(arg: &str) -> String {
    if !arg.is_empty() && arg.chars().all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c)) {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

impl RuntimeAdapter for AndroidAdapter {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            platform: Platform::Android,
            os: "android".into(),
            arch: std::env::consts::ARCH.into(),
            processes: ProcessSupport::Restricted,
            background: BackgroundModel::ForegroundService,
            file_watching: true,
            app_window: true,
            tray: false,
            installers: false,
            toolchains: false,
            wake_lock: true,
        }
    }

    fn command(
        &self,
        spec: &CommandSpec,
        extra_args: &[String],
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) -> Result<Command> {
        let argv: Option<Vec<String>> = match spec {
            CommandSpec::Argv(v) => Some(v.clone()),
            CommandSpec::Shell(line) => plain_words(line),
        };
        let mut cmd = match argv {
            Some(argv) => {
                let program = argv.first().ok_or_else(|| anyhow!("empty command"))?;
                let mut c = Command::new(self.resolve(program, cwd)?);
                c.args(&argv[1..]).args(extra_args);
                c
            }
            None => {
                // real shell syntax: run through the system shell; Android itself still refuses
                // to execute files outside the app's executable directories
                let CommandSpec::Shell(line) = spec else { unreachable!() };
                let mut full = line.clone();
                for a in extra_args {
                    full.push(' ');
                    full.push_str(&posix_quote(a));
                }
                let mut c = Command::new(&self.shell);
                c.arg("-c").arg(full);
                c
            }
        };
        let mut path: Vec<String> = self.executable_dirs.iter().map(|d| d.display().to_string()).collect();
        path.push(env.get("PATH").cloned().unwrap_or_else(|| "/system/bin".into()));
        cmd.current_dir(cwd)
            .envs(env)
            .env("PATH", path.join(":"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        Ok(cmd)
    }

    fn program_available(&self, spec: &CommandSpec, cwd: &Path) -> bool {
        match spec {
            CommandSpec::Shell(line) if plain_words(line).is_none() => true,
            _ => spec.program().map(|p| self.resolve(&p, cwd).is_ok()).unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> CommandSpec {
        CommandSpec::Argv(v.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn android_maps_bundled_programs() {
        let a = AndroidAdapter::new().with_program("node", "/data/app/x/lib/arm64/libnode.so");
        let cwd = Path::new("/data/user/0/x/files/app");
        assert_eq!(a.resolve("node", cwd).unwrap(), PathBuf::from("/data/app/x/lib/arm64/libnode.so"));
        assert!(a.program_available(&argv(&["node", "server.js"]), cwd));
        assert!(a.program_available(&CommandSpec::Shell("node server.js".into()), cwd));
    }

    #[test]
    fn android_rejects_programs_outside_the_app() {
        let a = AndroidAdapter::new().with_program("node", "/apk/lib/libnode.so");
        let cwd = Path::new("/data/user/0/x/files/app");
        let err = a.resolve("cargo", cwd).unwrap_err().to_string();
        assert!(err.contains("cannot run on Android"), "{err}");
        assert!(err.contains("bundled: node"), "{err}");
        assert!(a.resolve("./build/server", cwd).is_err());
        assert!(!a.program_available(&argv(&["python", "app.py"]), cwd));
    }

    #[test]
    fn android_python_alias_and_exec_dirs() {
        let a = AndroidAdapter::new()
            .with_program("python3", "/apk/lib/libpython.so")
            .with_executable_dir("/apk/lib");
        let cwd = Path::new("/files");
        assert_eq!(a.resolve("python", cwd).unwrap(), PathBuf::from("/apk/lib/libpython.so"));
        assert!(a.resolve("/apk/lib/libtool.so", cwd).is_ok());
    }

    #[test]
    fn plain_words_detects_shell_syntax() {
        assert_eq!(
            plain_words("node server.js --port 3"),
            Some(vec!["node".into(), "server.js".into(), "--port".into(), "3".into()])
        );
        assert_eq!(plain_words("npm run build && node x"), None);
        assert_eq!(plain_words("PORT=1 node x"), None);
    }

    #[test]
    fn termux_detection() {
        let p = "/data/data/com.termux/files/usr";
        assert!(TermuxAdapter::detect(Some(p), None, false).is_some());
        assert!(TermuxAdapter::detect(Some("/usr"), Some("0.118"), false).is_some());
        assert!(TermuxAdapter::detect(Some("/usr"), None, false).is_none());
        assert!(TermuxAdapter::detect(Some("/opt/env"), None, true).is_some(), "forced");
        assert!(TermuxAdapter::detect(None, Some("0.118"), true).is_none(), "needs a prefix");
    }

    #[test]
    fn termux_runs_installed_toolchains() {
        let prefix = tempfile::tempdir().unwrap();
        let bin = prefix.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("python3"), "#!/bin/sh\n").unwrap();
        let t = TermuxAdapter::new(prefix.path());
        let c = t.capabilities();
        assert_eq!(c.platform, Platform::Android);
        assert_eq!(c.processes, ProcessSupport::Full);
        assert!(c.toolchains && c.wake_lock && !c.installers && !c.app_window);

        let cwd = prefix.path();
        // `python` falls back to `python3`, like on the desktop
        let path = t.path_env(&BTreeMap::from([("PATH".to_string(), "/system/bin".to_string())]));
        assert_eq!(path, format!("{}:/system/bin", bin.display()));
        // (Termux paths are `:`-separated; on a Windows test host the drive letter splits off)
        let found = t.find("python", cwd, &path).expect("python3 found for `python`");
        assert_eq!(found.file_name().unwrap(), "python3");
        assert!(t.find("cargo", cwd, &path).is_none());
        assert!(t.program_available(&CommandSpec::Shell("npm run build && node x".into()), cwd));
        let env = BTreeMap::from([("PATH".to_string(), bin.display().to_string())]);
        let err = t.command(&argv(&["cargo", "run"]), &[], cwd, &env).unwrap_err().to_string();
        assert!(err.contains("pkg install rust"), "{err}");
        assert!(t.command(&argv(&["python", "app.py"]), &[], cwd, &env).is_ok());
        assert_eq!(t.install_hint("java").as_deref(), Some("pkg install openjdk-17"));
        assert_eq!(t.install_hint("postgres").as_deref(), Some("pkg install postgresql"));
        assert!(t.install_hint("frobnicate").is_none());
    }

    #[test]
    fn desktop_capabilities() {
        let c = DesktopAdapter::desktop().capabilities();
        assert_eq!(c.platform, Platform::Desktop);
        assert_eq!(c.processes, ProcessSupport::Full);
        assert!(c.tray && c.app_window);
        let s = DesktopAdapter::server().capabilities();
        assert_eq!(s.platform, Platform::Server);
        assert!(!s.tray && !s.app_window);
        assert!(c.toolchains && !AndroidAdapter::new().capabilities().toolchains);
    }
}
