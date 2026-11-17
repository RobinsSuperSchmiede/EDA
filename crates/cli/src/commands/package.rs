//! `openshell package` – native installers.

use super::load_project;
use super::run::{build_project, find_desktop_binary};
use crate::output::{self, bold, dim};
use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

pub async fn run(dir: &Path, targets: Vec<String>, out: Option<PathBuf>, skip_build: bool) -> Result<()> {
    let project = load_project(dir)?;
    let Some(desktop) = find_desktop_binary() else {
        bail!("the desktop shell (openshell-desktop) was not found; build it with `cargo build --release -p openshell-desktop`");
    };
    if desktop.components().any(|c| c.as_os_str() == "debug") {
        output::warn(&format!(
            "{} is a debug build (larger, opens a console window on Windows). Use a release build for distribution.",
            desktop.display()
        ));
    }
    if !skip_build {
        build_project(&project).await?;
    }
    println!("{} {} {}", dim("package:"), bold(&project.config.app.name), project.config.identifier());
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
