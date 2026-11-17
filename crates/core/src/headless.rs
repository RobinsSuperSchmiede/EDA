//! Run a shell without a window until it is asked to stop (Ctrl+C or `POST /v1/shutdown`).

use crate::{HeadlessHost, Shell};
use anyhow::Result;
use openshell_config::{Mode, Project};
use std::sync::Arc;

/// Launch, start all services and block until shutdown. Returns after all services stopped.
pub async fn run(project: Project, mode: Mode) -> Result<()> {
    let host = Arc::new(HeadlessHost::default());
    let shutdown = host.shutdown.clone();
    let shell = Shell::launch(project, mode, host).await?;
    shell.start();
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = shutdown.notified() => {}
    }
    shell.shutdown().await;
    Ok(())
}

/// Spawn `exe args...` fully detached (no console, own process group), e.g. to start a
/// headless instance on demand when an agent connects.
pub fn spawn_detached(exe: &std::path::Path, args: &[String]) -> Result<std::process::Child> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    disable_stdio_inheritance();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    Ok(cmd.spawn()?)
}

/// Windows hands all inheritable handles to child processes. Our stdio may be the pipes of an
/// MCP client or a shell pipeline; a long-running background child holding them would keep the
/// reader from ever seeing EOF. Call before spawning such children. No-op elsewhere.
pub fn disable_stdio_inheritance() {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
        use windows_sys::Win32::System::Console::{
            GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };
        unsafe {
            for h in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                let handle = GetStdHandle(h);
                if !handle.is_null() {
                    SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }
}
