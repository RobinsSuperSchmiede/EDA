mod add;
mod control;
mod create;
mod doctor;
mod init;
mod mcp;
mod package;
mod run;

pub use add::Lang;

use crate::{AddBlock, Cmd};
use anyhow::Result;
use openshell_core::config::Project;
use openshell_core::state::ControlClient;
use std::path::{Path, PathBuf};

/// The AI app-development guide (docs/ai/app-development.md), embedded so agents can read it from the binary.
const GUIDE: &str = include_str!("../../../../docs/ai/app-development.md");

pub async fn run(project: Option<PathBuf>, cmd: Cmd) -> Result<()> {
    let dir = project.clone().unwrap_or_else(|| PathBuf::from("."));
    match cmd {
        Cmd::Init { force, name, toml } => init::run(&dir, force, name, toml),
        Cmd::Create { name, template, with, lang, list } => {
            create::run_command(name.as_deref().unwrap_or_default(), template, with, lang, list)
        }
        Cmd::Add { block } => add::run(
            &dir,
            match block {
                AddBlock::Frontend => add::Block::Frontend,
                AddBlock::Backend { lang } => add::Block::Backend(lang),
                AddBlock::Database { name } => add::Block::Database(name),
                AddBlock::Function { name, lang } => add::Block::Function(name, lang),
            },
        ),
        Cmd::Guide => {
            print!("{}", GUIDE);
            Ok(())
        }
        Cmd::Schema => {
            println!("{}", serde_json::to_string_pretty(&openshell_core::config::json_schema())?);
            Ok(())
        }
        Cmd::Dev { headless } => {
            run::run(&dir, openshell_core::config::Mode::Development, headless, false).await
        }
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
        Cmd::Logs { service, follow, lines, errors, grep } => {
            control::logs(&dir, service, follow, lines, errors, grep).await
        }
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
