//! `openshell mcp` – MCP server over stdio for AI agents.
//!
//! * default profile: all developer tools (runtime, logs, services, files, UI automation, capabilities)
//! * `--app`: only the application's own functions – for agents that *use* the app
//! * `--start`: start the app in the background (headless) if it is not running

use super::load_project;
use anyhow::Result;
use openshell_core::config::Mode;
use openshell_core::stdio::{serve, Autostart, McpProfile};
use std::path::Path;

pub async fn run(dir: &Path, app: bool, start: bool, token: Option<String>) -> Result<()> {
    // an agent session token limits this connection to the session's scopes
    if let Some(t) = token {
        std::env::set_var("OPENSHELL_AGENT_TOKEN", t);
    }
    let project = load_project(dir).ok();
    if project.is_none() {
        eprintln!("openshell mcp: no openshell.json found from {}", dir.display());
    }
    let state_dir = project.as_ref().map(|p| p.state_dir());
    let profile = if app { McpProfile::App } else { McpProfile::Full };
    let autostart: Option<Autostart> = match (&project, start) {
        (Some(p), true) => {
            let root = p.root.clone();
            // using the app -> production mode; developing it -> development mode
            let mode = if app { Mode::Production } else { Mode::Development };
            Some(Box::new(move || {
                let exe = std::env::current_exe()?;
                let cmd = if mode == Mode::Production { "start" } else { "dev" };
                openshell_core::headless::spawn_detached(
                    &exe,
                    &["--project".into(), root.display().to_string(), cmd.into(), "--headless".into()],
                )
                .map(|_| ())
            }))
        }
        _ => None,
    };
    serve(state_dir, profile, autostart).await
}
