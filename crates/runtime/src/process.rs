//! Platform specific process handling: command construction, process trees,
//! termination and (on Windows) job objects that kill all children when the
//! shell itself exits.

use anyhow::{anyhow, Context, Result};
use openshell_config::CommandSpec;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::{Child, Command};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Build a command ready to spawn.
///
/// * `CommandSpec::Shell` runs through the platform shell (`cmd /C` or `sh -c`),
///   so `npm run dev`, pipes and `&&` work as in a terminal.
/// * `CommandSpec::Argv` is executed directly; the program is resolved via `PATH`
///   (including `.cmd`/`.exe` on Windows) or relative to the working directory.
pub fn build_command(
    spec: &CommandSpec,
    extra_args: &[String],
    cwd: &Path,
    env: &BTreeMap<String, String>,
) -> Result<Command> {
    let mut cmd = match spec {
        CommandSpec::Shell(line) => {
            let mut full = line.clone();
            for a in extra_args {
                full.push(' ');
                full.push_str(&shell_quote(a));
            }
            shell_command(&full)
        }
        CommandSpec::Argv(argv) => {
            let program = argv.first().ok_or_else(|| anyhow!("empty command"))?;
            let resolved = resolve_program(program, cwd)?;
            let mut c = Command::new(resolved);
            c.args(&argv[1..]);
            c.args(extra_args);
            c
        }
    };
    cmd.current_dir(cwd)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    configure_platform(&mut cmd);
    Ok(cmd)
}

/// A shell command line executed through the platform shell.
pub fn shell_command(line: &str) -> Command {
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.raw_arg("/D /S /C \"").raw_arg(line).raw_arg("\"");
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.arg("-c").arg(line);
        c
    }
}

fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@,+%".contains(c))
    {
        return arg.to_string();
    }
    #[cfg(windows)]
    {
        format!("\"{}\"", arg.replace('"', "\\\""))
    }
    #[cfg(not(windows))]
    {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// Resolve a program name: paths are taken relative to `cwd`, bare names are looked up in `PATH`.
pub fn resolve_program(program: &str, cwd: &Path) -> Result<std::path::PathBuf> {
    if program.contains('/') || program.contains('\\') {
        let p = Path::new(program);
        let p = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
        // `build/bin/server` should also find `server.exe` on Windows
        if cfg!(windows) && !p.exists() && p.extension().is_none() {
            let exe = p.with_extension("exe");
            if exe.exists() {
                return Ok(exe);
            }
        }
        return Ok(p);
    }
    if let Ok(p) = which::which(program) {
        return Ok(p);
    }
    // portable aliases: configs written on Windows use `python`, Linux/macOS often only have `python3`
    let alias = match program {
        "python" => Some("python3"),
        "python3" => Some("python"),
        "pip" => Some("pip3"),
        "pip3" => Some("pip"),
        _ => None,
    };
    if let Some(p) = alias.and_then(|a| which::which(a).ok()) {
        return Ok(p);
    }
    // project-local wrappers such as `gradlew` / `mvnw` (as cmd.exe would find them)
    let exts: &[&str] = if cfg!(windows) { &[".bat", ".cmd", ".exe", ""] } else { &[""] };
    for ext in exts {
        let candidate = cwd.join(format!("{program}{ext}"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    which::which(program).with_context(|| format!("`{program}` was not found in PATH"))
}

/// Whether a command's program can be found (used by `openshell doctor`).
pub fn program_exists(spec: &CommandSpec, cwd: &Path) -> bool {
    match spec.program() {
        Some(p) => {
            // shell built-ins and env assignments cannot be checked reliably
            if matches!(p.as_str(), "cd" | "echo" | "set" | "export" | "exit") || p.contains('=') {
                return true;
            }
            resolve_program(&p, cwd).map(|p| p.exists()).unwrap_or(false)
        }
        None => false,
    }
}

#[cfg(windows)]
fn configure_platform(cmd: &mut Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn configure_platform(cmd: &mut Command) {
    // own process group so the whole tree can be signalled
    cmd.process_group(0);
    #[cfg(target_os = "linux")]
    unsafe {
        cmd.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
}

/// Terminate a child and all of its descendants.
///
/// Unix: SIGTERM to the process group, SIGKILL after `grace`.
/// Windows: `taskkill /T /F` on the process tree.
pub async fn terminate(child: &mut Child, grace: Duration) {
    let Some(pid) = child.id() else {
        let _ = child.wait().await;
        return;
    };
    #[cfg(unix)]
    {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            // also clean up remaining group members
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            return;
        }
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = grace;
        let mut tk = std::process::Command::new("taskkill");
        tk.args(["/PID", &pid.to_string(), "/T", "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        {
            use std::os::windows::process::CommandExt;
            tk.creation_flags(CREATE_NO_WINDOW);
        }
        let _ = tokio::task::spawn_blocking(move || tk.status()).await;
    }
    if tokio::time::timeout(Duration::from_secs(3), child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

/// Find a free TCP port on the loopback interface.
pub fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// Whether something is listening on `127.0.0.1:port`.
pub fn port_in_use(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        Duration::from_millis(200),
    )
    .is_ok()
}

/// Windows job object: every child assigned to it is killed when the shell
/// process exits, even if it crashes. A no-op on other platforms.
pub struct ProcessJob {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

unsafe impl Send for ProcessJob {}
unsafe impl Sync for ProcessJob {}

impl ProcessJob {
    pub fn new() -> Option<Self> {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::JobObjects::*;
            let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if handle.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                windows_sys::Win32::Foundation::CloseHandle(handle);
                return None;
            }
            Some(Self { handle })
        }
        #[cfg(not(windows))]
        {
            Some(Self {})
        }
    }

    pub fn assign(&self, child: &Child) {
        #[cfg(windows)]
        unsafe {
            if let Some(h) = child.raw_handle() {
                windows_sys::Win32::System::JobObjects::AssignProcessToJobObject(
                    self.handle,
                    h as _,
                );
            }
        }
        #[cfg(not(windows))]
        let _ = child;
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}
