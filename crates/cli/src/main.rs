//! `openshell` – command line interface of OpenShell.

mod commands;
mod output;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "openshell",
    version,
    about = "OpenShell – run any web app as a native desktop app, managed by a runtime that humans and AI agents can control.",
    after_help = "Quick start:\n  openshell create my-app && cd my-app && openshell dev\n  openshell init        # in an existing project\n\nDocs: https://github.com/RobinsSuperSchmiede/EDA"
)]
struct Cli {
    /// Project directory (default: current directory or the nearest parent with an openshell.json)
    #[arg(short, long, global = true)]
    project: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create openshell.json for an existing project (detects frontend and backend)
    Init {
        /// Overwrite an existing configuration
        #[arg(long)]
        force: bool,
        /// Application name (default: directory name)
        #[arg(long)]
        name: Option<String>,
        /// Write openshell.toml instead of openshell.json
        #[arg(long)]
        toml: bool,
    },
    /// Create a new project from a template
    ///
    /// Either from a template (`--template react-python`) or composed from building blocks
    /// (`--with frontend,database,functions`). Without both, the terminal asks which blocks you want.
    Create {
        /// Directory / name of the new project
        #[arg(required_unless_present = "list")]
        name: Option<String>,
        /// Template to use (see --list)
        #[arg(short, long, conflicts_with = "with")]
        template: Option<String>,
        /// Compose the app from building blocks: frontend, backend, database, functions (comma-separated)
        #[arg(short, long, value_delimiter = ',')]
        with: Vec<String>,
        /// Language of the backend and functions when using --with
        #[arg(long, value_enum, default_value = "python")]
        lang: commands::Lang,
        /// List available templates
        #[arg(long)]
        list: bool,
    },
    /// Add a building block to the app: frontend, backend, database or function
    Add {
        #[command(subcommand)]
        block: AddBlock,
    },
    /// Run the app in development mode (hot reload, file watching, broad agent access)
    Dev {
        /// Do not open a window; serve the frontend for the browser instead
        #[arg(long)]
        headless: bool,
    },
    /// Run the app in production mode
    Start {
        /// Do not open a window (background services, browser access)
        #[arg(long)]
        headless: bool,
        /// Return immediately instead of following the logs
        #[arg(short, long)]
        detach: bool,
    },
    /// Stop a service, or the whole shell when no service is given
    Stop { service: Option<String> },
    /// Restart a service
    Restart { service: String },
    /// Start a stopped service
    Run { service: String },
    /// Show the status of the shell and all services
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Show service logs
    Logs {
        /// Only this service
        service: Option<String>,
        /// Keep following new lines
        #[arg(short, long)]
        follow: bool,
        /// Number of lines
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
        /// Only stderr and runtime messages
        #[arg(long)]
        errors: bool,
        /// Filter (case-insensitive)
        #[arg(long)]
        grep: Option<String>,
    },
    /// Check configuration, toolchain and environment
    Doctor,
    /// Build for production: frontend (`frontend.build`) and compiled services (`services[].build`)
    Build,
    /// Build a native installer (NSIS/MSI, DMG, DEB/AppImage) for the app
    Package {
        /// Installer format(s): nsis, msi, app, dmg, deb, rpm, appimage
        #[arg(short, long)]
        target: Vec<String>,
        /// Copy the installers into this directory
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Do not run `openshell build` first
        #[arg(long)]
        skip_build: bool,
    },
    /// MCP server on stdin/stdout for AI agents (proxies to the running shell)
    Mcp {
        /// Only expose the application's own functions (for agents that use the app, not develop it)
        #[arg(long)]
        app: bool,
        /// Start the app in the background if it is not running
        #[arg(long)]
        start: bool,
    },
    /// List agent tools and whether agents may use them
    Tools {
        #[arg(long)]
        json: bool,
    },
    /// Call an agent tool, e.g. `openshell call ui_snapshot` or `openshell call service_restart '{"name":"backend"}'`
    Call {
        tool: String,
        /// JSON arguments
        args: Option<String>,
    },
    /// List pending approval requests of agents
    Approvals,
    /// Approve a pending agent action
    Approve { id: String },
    /// Deny a pending agent action
    Deny { id: String },
    /// Show recent agent actions from the audit log
    Audit {
        #[arg(short = 'n', long, default_value_t = 30)]
        lines: usize,
    },
    /// Print the JSON Schema of openshell.json
    Schema,
    /// Print the guide for AI coding agents that build apps on OpenShell
    Guide,
}

#[derive(Subcommand)]
enum AddBlock {
    /// A web UI in web/ (static HTML/JS, served by OpenShell)
    Frontend,
    /// An HTTP service in backend/ (port, health check, restart on change)
    Backend {
        #[arg(long, value_enum, default_value = "python")]
        lang: commands::Lang,
    },
    /// A built-in SQLite database with migrations (agents and the frontend can use it)
    Database {
        /// Database name
        #[arg(default_value = "main")]
        name: String,
    },
    /// A function for AI agents (MCP tool) implemented as a script – no server needed
    #[command(alias = "tool")]
    Function {
        /// Function name (lowercase, digits, `_`)
        name: String,
        #[arg(long, value_enum, default_value = "python")]
        lang: commands::Lang,
    },
}

fn main() {
    let cli = Cli::parse();
    let filter = std::env::var("OPENSHELL_LOG").unwrap_or_else(|_| "warn".into());
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(filter))
        .with_writer(std::io::stderr)
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
        .with_target(false)
        .init();
    output::enable_ansi();

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("tokio runtime");
    let result = rt.block_on(commands::run(cli.project, cli.command));
    if let Err(e) = result {
        output::error(&format!("{e:#}"));
        std::process::exit(1);
    }
    // don't wait for background tasks (stdin readers etc.)
    rt.shutdown_timeout(std::time::Duration::from_millis(200));
}

pub(crate) use Command as Cmd;
