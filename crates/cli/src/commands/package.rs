//! `openshell package` – native installers and the portable app bundle (`.osapp`).

use super::load_project;
use super::run::build_project;
use crate::output::{self, bold, dim};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub async fn run(dir: &Path, targets: Vec<String>, out: Option<PathBuf>, skip_build: bool) -> Result<()> {
    let project = load_project(dir)?;
    let targets = if targets.is_empty() { project.config.package.targets.clone() } else { targets };
    let wants_bundle = targets.iter().any(|t| t == "bundle");
    let installers: Vec<String> = targets.iter().filter(|t| *t != "bundle").cloned().collect();
    // only `bundle` requested: no desktop shell needed (works on Android / Termux too)
    let only_bundle = wants_bundle && installers.is_empty();
    if !skip_build {
        build_project(&project).await?;
    }
    println!("{} {} {}", dim("package:"), bold(&project.config.app.name), project.config.identifier());
    if wants_bundle || cfg!(target_os = "android") {
        let out_dir = out.clone().unwrap_or_else(|| project.root.join("dist"));
        let bundle = openshell_core::bundle::create(&project, &out_dir)?;
        output::ok(&format!("app bundle: {}", bold(&bundle.display().to_string())));
        println!(
            "  {}",
            dim("runs unchanged on any OpenShell host: `openshell install <file>` (desktop, server, Termux) or the Android app")
        );
    }
    if only_bundle {
        return Ok(());
    }
    installers_for(project, installers, out).await
}

#[cfg(target_os = "android")]
async fn installers_for(
    _project: openshell_core::config::Project,
    _targets: Vec<String>,
    _out: Option<PathBuf>,
) -> Result<()> {
    println!(
        "  {}",
        dim("installers for Windows, macOS and Linux are built on those systems – or by CI: `openshell release workflow`")
    );
    Ok(())
}

#[cfg(not(target_os = "android"))]
async fn installers_for(
    project: openshell_core::config::Project,
    targets: Vec<String>,
    out: Option<PathBuf>,
) -> Result<()> {
    use super::run::find_desktop_binary;
    let Some(desktop) = find_desktop_binary() else {
        anyhow::bail!("the desktop shell (openshell-desktop) was not found; build it with `cargo build --release -p openshell-desktop`");
    };
    if desktop.components().any(|c| c.as_os_str() == "debug") {
        output::warn(&format!(
            "{} is a debug build (larger, opens a console window on Windows). Use a release build for distribution.",
            desktop.display()
        ));
    }
    let opts = openshell_package::Options { desktop_binary: desktop, targets, out_dir: out };
    let result = tokio::task::spawn_blocking(move || {
        openshell_package::package(&project, &opts, &|msg| println!("  {} {msg}", dim("·")))
    })
    .await??;
    println!();
    for i in &result.installers {
        output::ok(&format!("installer: {}", bold(&i.display().to_string())));
    }
    println!("  {} {}", dim("portable build (runs without installing):"), result.staging.display());
    Ok(())
}
