mod control;
mod create;
mod doctor;
mod init;
mod mcp;
mod package;
mod run;

use crate::Cmd;
use anyhow::Result;
use openshell_core::config::Project;
use openshell_core::state::ControlClient;
use std::path::{Path, PathBuf};

pub async fn run(project: Option<PathBuf>, cmd: Cmd) -> Result<()> {
    let dir = project.clone().unwrap_or_else(|| PathBuf::from("."));
    match cmd {
        Cmd::Init { force, name, toml } => init::run(&dir, force, name, toml),
        Cmd::Create { name, template, list } => create::run(name.as_deref().unwrap_or_default(), &template, list),
        Cmd::Schema => {
            println!("{}", serde_json::to_string_pretty(&openshell_core::config::json_schema())?);
            Ok(())
        }
        Cmd::Dev { headless } => run::run(&dir, openshell_core::config::Mode::Development, headless, false).await,
        Cmd::Start { headless, detach } => {
            run::run(&dir, openshell_core::config::Mode::Production, headless, detach).await
        }
        Cmd::Build => run::build(&dir).await,
        Cmd::Package { target, out, skip_build } => package::run(&dir, target, out, skip_build).await,
        Cmd::Doctor => doctor::run(&dir).await,
        Cmd::Mcp { app, start } => mcp::run(&dir, app, start).await,
        Cmd::Stop { service } => control::stop(&dir, service).await,
        Cmd::Restart { service } => control::service(&dir, &service, "restart").await,
        Cmd::Run { service } => control::service(&dir, &service, "start").await,
        Cmd::Status { json } => control::status(&dir, json).await,
        Cmd::Logs {
            service,
            follow,
            lines,
            errors,
            grep,
        } => control::logs(&dir, service, follow, lines, errors, grep).await,
        Cmd::Tools { json } => control::tools(&dir, json).await,
        Cmd::Call { tool, args } => control::call(&dir, &tool, args).await,
        Cmd::Approvals => control::approvals(&dir).await,
        Cmd::Approve { id } => control::decide(&dir, &id, true).await,
        Cmd::Deny { id } => control::decide(&dir, &id, false).await,
        Cmd::Audit { lines } => control::audit(&dir, lines).await,
    }
}

/// Load the project configuration (walking up from `dir`).
pub fn load_project(dir: &Path) -> Result<Project> {
    Ok(Project::discover(dir)?)
}

/// Connect to the running shell of the project in `dir`.
pub async fn connect(dir: &Path) -> Result<ControlClient> {
    let project = load_project(dir)?;
    ControlClient::connect(&project.state_dir()).await
}
