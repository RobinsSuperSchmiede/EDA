mod add;
mod agents;
mod control;
mod create;
mod doctor;
mod init;
mod mcp;
mod package;
mod release;
mod remote;
mod run;
mod runtime;

pub use add::Lang;

use crate::{AddBlock, AgentsCmd, Cmd, ReleaseCmd, RemoteCmd, RuntimeCmd};
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
        Cmd::Mcp { app, start, token } => mcp::run(&dir, app, start, token).await,
        Cmd::Agents { action } => {
            agents::run(
                &dir,
                match action.unwrap_or(AgentsCmd::Show) {
                    AgentsCmd::Show => agents::Action::Show,
                    AgentsCmd::Enable => agents::Action::Enable,
                    AgentsCmd::Disable => agents::Action::Disable,
                    AgentsCmd::Token { name, scopes, full } => agents::Action::Token { name, scopes, full },
                    AgentsCmd::Tokens => agents::Action::Tokens,
                    AgentsCmd::Revoke { name } => agents::Action::Revoke { name },
                },
            )
            .await
        }
        Cmd::Runtime { action } => {
            runtime::run(
                &dir,
                match action.unwrap_or(RuntimeCmd::List) {
                    RuntimeCmd::List => runtime::Action::List,
                    RuntimeCmd::Add { name, version, path, no_install } => {
                        runtime::Action::Add { name, version, path, install: !no_install }
                    }
                    RuntimeCmd::Install { name, force } => runtime::Action::Install { name, force },
                    RuntimeCmd::Remove { name, keep_files } => runtime::Action::Remove { name, keep_files },
                },
            )
            .await
        }
        Cmd::Languages => {
            create::print_languages();
            Ok(())
        }
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
        Cmd::Remote { action } => {
            let action = match action {
                None => remote::Action::Show,
                Some(RemoteCmd::Pair { device, no_qr, scopes }) => {
                    remote::Action::Pair { device, qr: !no_qr, scopes }
                }
                Some(RemoteCmd::Devices) => remote::Action::Devices,
                Some(RemoteCmd::Revoke { device }) => remote::Action::Revoke { device },
                Some(RemoteCmd::Unblock { address }) => remote::Action::Unblock { address },
            };
            remote::run(&dir, action).await
        }
        Cmd::Lifecycle { event } => control::lifecycle(&dir, &event).await,
        Cmd::Release { action } => match action {
            ReleaseCmd::Workflow { force } => release::workflow(&dir, force),
            ReleaseCmd::Manifest { dir: files, base_url } => release::manifest(&dir, &files, base_url),
        },
        Cmd::Install { file, start } => release::install(&file, start).await,
        Cmd::Apps => release::apps(),
        Cmd::Update => release::update(&dir).await,
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

/// `p` relative to `root` with forward slashes (`.` for the root itself).
pub(crate) fn rel_path(root: &Path, p: &Path) -> String {
    let r = p.strip_prefix(root).unwrap_or(p).to_string_lossy().replace('\\', "/");
    if r.is_empty() {
        ".".into()
    } else {
        r
    }
}
