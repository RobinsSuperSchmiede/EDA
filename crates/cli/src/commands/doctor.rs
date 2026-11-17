//! `openshell doctor` – diagnose configuration and environment.

use super::run::find_desktop_binary;
use crate::output::{bold, dim, green, red, yellow};
use anyhow::Result;
use openshell_core::config::{validate, Mode, Project};
use openshell_core::runtime::process::{port_in_use, program_exists};
use openshell_core::runtime::{build_plan, DefKind};
use openshell_core::state::ControlClient;
use std::path::Path;

struct Report {
    errors: usize,
    warnings: usize,
}

impl Report {
    fn ok(&self, msg: &str) {
        println!("  {} {msg}", green("✔"));
    }
    fn warn(&mut self, msg: &str) {
        self.warnings += 1;
        println!("  {} {msg}", yellow("!"));
    }
    fn err(&mut self, msg: &str) {
        self.errors += 1;
        println!("  {} {msg}", red("✘"));
    }
}

fn tool_version(cmd: &str, arg: &str) -> Option<String> {
    let out = std::process::Command::new(which::which(cmd).ok()?).arg(arg).output().ok()?;
    let text = String::from_utf8_lossy(if out.stdout.is_empty() { &out.stderr } else { &out.stdout }).to_string();
    text.lines().next().map(|l| l.trim().to_string())
}

pub async fn run(dir: &Path) -> Result<()> {
    let mut r = Report { errors: 0, warnings: 0 };
    println!("{}", bold("Configuration"));
    let project = match Project::discover(dir) {
        Ok(p) => {
            r.ok(&format!("{} ({})", p.config_path.display(), p.config.app.name));
            for issue in validate(&p.config) {
                r.warn(&issue.to_string());
            }
            Some(p)
        }
        Err(e) => {
            r.err(&format!("{e}"));
            println!("    {}", dim("create one with `openshell init` or `openshell create <name>`"));
            None
        }
    };

    if let Some(p) = &project {
        let running = ControlClient::connect(&p.state_dir()).await.ok();
        println!("\n{}", bold("Services (development mode)"));
        match build_plan(p, Mode::Development) {
            Ok(defs) => {
                for d in defs {
                    let label = format!("{:<10}", d.name);
                    match &d.kind {
                        DefKind::Process { command, .. } => {
                            if !d.cwd.is_dir() {
                                r.err(&format!("{label} working directory {} does not exist", d.cwd.display()));
                            } else if !program_exists(command, &d.cwd) {
                                r.err(&format!(
                                    "{label} `{}` not found in PATH",
                                    command.program().unwrap_or_default()
                                ));
                            } else {
                                r.ok(&format!("{label} {}", command.display()));
                            }
                            if let Some(build) = &d.build {
                                if program_exists(&build.command, &build.cwd) {
                                    println!("    {}", dim(&format!("build: {}", build.command.display())));
                                } else {
                                    r.err(&format!(
                                        "{label} build tool `{}` not found in PATH",
                                        build.command.program().unwrap_or_default()
                                    ));
                                }
                            }
                            if let Some(setup) = &d.setup {
                                let pending = setup.creates.as_ref().map(|c| !c.exists()).unwrap_or(true);
                                if pending {
                                    if program_exists(&setup.command, &setup.cwd) {
                                        println!("    {}", dim(&format!("setup `{}` will run on first start", setup.command.display())));
                                    } else {
                                        r.err(&format!("{label} setup `{}` not runnable (missing in PATH)", setup.command.display()));
                                    }
                                }
                            }
                        }
                        DefKind::Static { dist, .. } => {
                            if dist.is_dir() {
                                r.ok(&format!("{label} static files in {}", dist.display()));
                            } else {
                                r.err(&format!("{label} {} does not exist (run `openshell build`?)", dist.display()));
                            }
                        }
                        DefKind::External => r.ok(&format!("{label} external {}", d.url.clone().unwrap_or_default())),
                    }
                    if running.is_none() && !matches!(d.kind, DefKind::External) {
                        if let Some(port) = d.port {
                            if port_in_use(port) {
                                r.warn(&format!("{label} port {port} is already in use"));
                            }
                        }
                    }
                }
            }
            Err(e) => r.err(&format!("{e:#}")),
        }
        if let Some(prod) = &p.config.frontend {
            if let Some(openshell_core::config::FrontendPlan::Static { dist }) = prod.plan(Mode::Production) {
                if !p.resolve_path(&dist).is_dir() {
                    r.warn(&format!("production frontend `{dist}` is not built yet (`openshell build`)"));
                }
            }
        }

        println!("\n{}", bold("Runtime"));
        match running {
            Some(c) => r.ok(&format!("running: {} mode, pid {}, {}", c.info.mode, c.info.pid, c.info.url)),
            None => println!("  {} not running", dim("·")),
        }
    }

    println!("\n{}", bold("Desktop"));
    match find_desktop_binary() {
        Some(b) => r.ok(&format!("desktop shell: {}", b.display())),
        None => r.warn("desktop shell `openshell-desktop` not found – `openshell dev` will run headless (browser mode)"),
    }
    #[cfg(windows)]
    {
        let key = r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
        let found = std::process::Command::new("reg")
            .args(["query", key, "/v", "pv"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if found {
            r.ok("WebView2 runtime installed");
        } else {
            r.warn("WebView2 runtime not detected (preinstalled on Windows 11; otherwise install it from Microsoft)");
        }
    }
    #[cfg(target_os = "linux")]
    println!("  {}", dim("Linux needs WebKitGTK 4.1 (e.g. `apt install libwebkit2gtk-4.1-0`)"));

    println!("\n{}", bold("Toolchain"));
    for (cmd, arg) in [
        ("node", "--version"),
        ("npm", "--version"),
        ("python", "--version"),
        ("python3", "--version"),
        ("go", "version"),
        ("java", "-version"),
        ("javac", "-version"),
        ("mvn", "-v"),
        ("gradle", "-v"),
        ("cmake", "--version"),
        ("g++", "--version"),
        ("clang++", "--version"),
    ] {
        match tool_version(cmd, arg) {
            Some(v) => println!("  {} {cmd:<8} {}", green("✔"), dim(&v)),
            None => println!("  {} {cmd:<8} {}", dim("·"), dim("not found")),
        }
    }

    println!();
    if r.errors > 0 {
        println!("{} {} error(s), {} warning(s)", red("✘"), r.errors, r.warnings);
        std::process::exit(1);
    } else if r.warnings > 0 {
        println!("{} no errors, {} warning(s)", yellow("!"), r.warnings);
    } else {
        println!("{} everything looks good", green("✔"));
    }
    Ok(())
}
