//! `openshell dev` / `openshell start` / `openshell build`.

use super::load_project;
use crate::output::{self, bold, cyan, dim, green, yellow};
use anyhow::{bail, Context, Result};
use openshell_core::agent::approvals::ApprovalEvent;
use openshell_core::config::{validate, Mode, Project};
use openshell_core::runtime::process::build_command;
use openshell_core::state::{self, ControlClient};
use openshell_core::{HeadlessHost, Shell};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub async fn run(dir: &Path, mode: Mode, headless: bool, detach: bool) -> Result<()> {
    let project = load_project(dir)?;
    for issue in validate(&project.config) {
        output::warn(&issue.to_string());
    }
    if let Ok(c) = ControlClient::connect(&project.state_dir()).await {
        bail!(
            "`{}` is already running ({} mode, pid {}). Use `openshell stop` first.",
            c.info.app,
            c.info.mode,
            c.info.pid
        );
    }
    // a previous instance may have crashed
    state::remove_runtime_info(&project.state_dir());

    if !headless {
        match find_desktop_binary() {
            Some(bin) => return run_desktop(&project, mode, &bin, detach).await,
            None => output::warn(
                "the desktop app (openshell-desktop) was not found next to the CLI or in PATH; \
                 running headless – open the frontend in your browser. \
                 Build it with `cargo build -p openshell-desktop` or set OPENSHELL_DESKTOP.",
            ),
        }
    }
    if detach {
        return spawn_detached(&project, mode).await;
    }
    run_headless(project, mode).await
}

/// Locate the `openshell-desktop` binary.
pub fn find_desktop_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("OPENSHELL_DESKTOP") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let name = if cfg!(windows) {
        "openshell-desktop.exe"
    } else {
        "openshell-desktop"
    };
    if let Ok(exe) = std::env::current_exe() {
        let candidate = exe.with_file_name(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    which::which("openshell-desktop").ok()
}

fn banner(app: &str, mode: Mode, host: &str, control: &str) {
    println!();
    println!("  {} {}  {}", bold("OpenShell"), dim(env!("CARGO_PKG_VERSION")), bold(app));
    println!("  {} {mode} · {host}", dim("mode   "));
    println!("  {} {control}", dim("control"));
    println!("  {} openshell mcp   {}", dim("agents "), dim("(MCP over stdio)"));
    println!();
}

fn print_log_line(line: &Value, width: usize) {
    let service = line["service"].as_str().unwrap_or("?");
    let text = line["text"].as_str().unwrap_or("");
    let text = match line["stream"].as_str() {
        Some("stderr") => output::paint("91", text),
        Some("system") => dim(text),
        _ => text.to_string(),
    };
    println!("{} {text}", output::service_tag(service, width));
}

fn tag_width(names: &[String]) -> usize {
    names.iter().map(|n| n.len()).max().unwrap_or(8).max(8)
}

async fn run_headless(project: Project, mode: Mode) -> Result<()> {
    let host = Arc::new(HeadlessHost::default());
    let shutdown = host.shutdown.clone();
    let shell = Shell::launch(project, mode, host).await?;
    let names = shell.runtime().service_names();
    let width = tag_width(&names);
    banner(shell.runtime().app_name(), mode, "headless", shell.control_url());

    // stream logs
    let mut logs = shell.runtime().logs().subscribe();
    tokio::spawn(async move {
        loop {
            match logs.recv().await {
                Ok(line) => print_log_line(&serde_json::to_value(&line).unwrap_or_default(), width),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    });
    // announce approvals
    let mut approvals = shell.approvals().subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = approvals.recv().await {
            if let ApprovalEvent::Requested(r) = ev {
                println!(
                    "\n  {} agent `{}` wants to {}\n  approve: {}   deny: {}\n",
                    yellow("approval needed:"),
                    r.agent,
                    bold(&r.summary),
                    cyan(&format!("openshell approve {}", r.id)),
                    cyan(&format!("openshell deny {}", r.id)),
                );
            }
        }
    });
    shell.start();
    if let Some(url) = shell.runtime().frontend_url() {
        let rt = shell.runtime().clone();
        tokio::spawn(async move {
            if rt.wait_ready("frontend", Duration::from_secs(600)).await.is_ok() {
                println!("\n  {} open {}\n", green("ready:"), bold(&url));
            }
        });
    }
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = shutdown.notified() => {}
    }
    println!("\n{}", dim("stopping services…"));
    shell.shutdown().await;
    output::ok("stopped");
    Ok(())
}

async fn wait_for_shell(project: &Project, child: &mut std::process::Child, timeout: Duration) -> Result<ControlClient> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(c) = ControlClient::connect(&project.state_dir()).await {
            return Ok(c);
        }
        if let Some(status) = child.try_wait()? {
            bail!("the shell exited during startup ({status}). Run `openshell doctor` for diagnostics.");
        }
        if Instant::now() > deadline {
            bail!("the shell did not start within {}s", timeout.as_secs());
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn configure_detached(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // own process group: Ctrl+C in this terminal is handled by the CLI, which shuts the shell down gracefully
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
}

async fn run_desktop(project: &Project, mode: Mode, bin: &Path, detach: bool) -> Result<()> {
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("--project")
        .arg(&project.root)
        .arg("--mode")
        .arg(mode.to_string())
        .stdin(Stdio::null());
    if detach {
        cmd.stdout(Stdio::null()).stderr(Stdio::null());
        openshell_core::headless::disable_stdio_inheritance();
    }
    configure_detached(&mut cmd);
    let mut child = cmd
        .spawn()
        .with_context(|| format!("cannot start {}", bin.display()))?;
    let client = wait_for_shell(project, &mut child, Duration::from_secs(60)).await?;
    banner(&client.info.app, mode, "desktop", &client.info.url);
    if detach {
        output::ok(&format!("running in the background (pid {}). Stop it with `openshell stop`.", child.id()));
        return Ok(());
    }

    let status = client.get("/v1/status").await?;
    let names: Vec<String> = status["services"]
        .as_array()
        .map(|a| a.iter().filter_map(|s| s["name"].as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let width = tag_width(&names);
    let mut since = 0u64;
    let mut seen_approvals: Vec<String> = vec![];
    let ctrl_c = tokio::signal::ctrl_c();
    tokio::pin!(ctrl_c);
    loop {
        if let Ok(v) = client.get(&format!("/v1/logs?since={since}&limit=1000")).await {
            for line in v["lines"].as_array().into_iter().flatten() {
                print_log_line(line, width);
            }
            since = v["last_seq"].as_u64().unwrap_or(since);
        }
        if let Ok(v) = client.get("/v1/approvals").await {
            for r in v["pending"].as_array().into_iter().flatten() {
                let id = r["id"].as_str().unwrap_or_default().to_string();
                if !seen_approvals.contains(&id) {
                    println!(
                        "\n  {} agent `{}` wants to {} (answer in the window or run `openshell approve {id}`)\n",
                        yellow("approval needed:"),
                        r["agent"].as_str().unwrap_or("?"),
                        bold(r["summary"].as_str().unwrap_or("?")),
                    );
                    seen_approvals.push(id);
                }
            }
        }
        if let Some(status) = child.try_wait()? {
            state::remove_runtime_info(&project.state_dir());
            if status.success() {
                output::ok("window closed, services stopped");
                return Ok(());
            }
            bail!("the desktop shell exited with {status}");
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(400)) => {}
            _ = &mut ctrl_c => {
                println!("\n{}", dim("stopping…"));
                let _ = client.post("/v1/shutdown", &serde_json::json!({})).await;
                let deadline = Instant::now() + Duration::from_secs(30);
                while child.try_wait()?.is_none() {
                    if Instant::now() > deadline {
                        let _ = child.kill();
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                state::remove_runtime_info(&project.state_dir());
                output::ok("stopped");
                return Ok(());
            }
        }
    }
}

/// Headless background instance (`openshell start --headless --detach`).
async fn spawn_detached(project: &Project, mode: Mode) -> Result<()> {
    let exe = std::env::current_exe()?;
    let args = vec![
        "--project".to_string(),
        project.root.display().to_string(),
        if mode == Mode::Development { "dev" } else { "start" }.to_string(),
        "--headless".to_string(),
    ];
    let mut child = openshell_core::headless::spawn_detached(&exe, &args)?;
    let client = wait_for_shell(project, &mut child, Duration::from_secs(60)).await?;
    banner(&client.info.app, mode, "headless", &client.info.url);
    output::ok(&format!(
        "running in the background (pid {}). Status: `openshell status`, stop: `openshell stop`.",
        child.id()
    ));
    Ok(())
}

pub async fn build(dir: &Path) -> Result<()> {
    let project = load_project(dir)?;
    build_project(&project).await
}

/// Production build: compiled services (`services[].build`) and the frontend (`frontend.build`).
pub async fn build_project(project: &Project) -> Result<()> {
    let mut did_something = false;
    // service builds (C++, Java, Go, Rust, ...), variables resolved like at runtime
    let defs = openshell_core::runtime::build_plan(project, Mode::Production)?;
    for def in defs.iter().filter(|d| d.build.is_some()) {
        let b = def.build.as_ref().unwrap();
        println!("{} {} {}", dim("build:"), bold(&def.name), b.command.display());
        run_inherit(&b.command, &b.cwd)
            .await
            .with_context(|| format!("building `{}` failed", def.name))?;
        did_something = true;
    }
    if let Some(fe) = &project.config.frontend {
        let cwd = fe
            .cwd
            .as_ref()
            .map(|c| project.resolve_path(c))
            .unwrap_or_else(|| project.root.clone());
        if let Some(build) = &fe.build {
            if let Some(setup) = &fe.setup {
                let needed = setup
                    .creates
                    .as_ref()
                    .map(|c| !project.resolve_path(c).exists())
                    .unwrap_or(true);
                if needed {
                    println!("{} {}", dim("setup:"), setup.command.display());
                    run_inherit(&setup.command, &cwd).await?;
                }
            }
            println!("{} {} {}", dim("build:"), bold("frontend"), build.display());
            run_inherit(build, &cwd).await?;
            did_something = true;
        }
    }
    if did_something {
        output::ok("build finished. Run it with `openshell start` or create an installer with `openshell package`.");
    } else {
        output::ok("nothing to build (no `build` steps configured)");
    }
    Ok(())
}

async fn run_inherit(command: &openshell_core::config::CommandSpec, cwd: &Path) -> Result<()> {
    let mut cmd = build_command(command, &[], cwd, &Default::default())?;
    cmd.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    let status = cmd.status().await?;
    if !status.success() {
        bail!("`{}` failed ({status})", command.display());
    }
    Ok(())
}
