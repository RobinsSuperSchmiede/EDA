//! `openshell release …`, `openshell install`, `openshell apps`, `openshell update` – getting an
//! app to users on every system.

use super::load_project;
use crate::output::{self, bold, cyan, dim, green, yellow};
use anyhow::{bail, Context, Result};
use openshell_core::{bundle, release};
use std::path::Path;

const WORKFLOW: &str = include_str!("../../assets/github-release.yml");
pub const WORKFLOW_PATH: &str = ".github/workflows/openshell-release.yml";

/// The release workflow for the app's repository, pinned to this OpenShell version.
pub fn workflow_text() -> String {
    WORKFLOW.replace("__OPENSHELL_VERSION__", env!("CARGO_PKG_VERSION"))
}

pub fn workflow(dir: &Path, force: bool) -> Result<()> {
    let project = load_project(dir)?;
    let path = project.root.join(WORKFLOW_PATH);
    if path.exists() && !force {
        bail!("{} exists (use --force to overwrite)", path.display());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, workflow_text())?;
    output::ok(&format!("wrote {}", bold(WORKFLOW_PATH)));
    println!(
        "\n  {} push a tag to build and publish installers for Windows, macOS, Linux and the .osapp bundle:",
        dim("next:")
    );
    let v = project.config.app.version.clone().unwrap_or_else(|| "0.1.0".into());
    println!("    {}", cyan(&format!("git tag v{v} && git push origin v{v}")));
    println!(
        "  {}",
        dim("users then download from https://github.com/<owner>/<repo>/releases/latest (download page: index.html)")
    );
    if project.config.package.update_url.is_none() {
        println!(
            "  {} {}",
            dim("update checks:"),
            cyan(
                r#""package": { "update_url": "https://github.com/<owner>/<repo>/releases/latest/download/downloads.json" }"#
            )
        );
    }
    Ok(())
}

pub fn manifest(dir: &Path, files: &Path, base_url: Option<String>) -> Result<()> {
    let project = load_project(dir)?;
    let version = project.config.app.version.clone().unwrap_or_else(|| "0.1.0".into());
    let d = release::manifest(files, &project.config.app.name, &version, base_url.as_deref().unwrap_or(""))?;
    release::write(files, &d)?;
    for x in &d.downloads {
        println!(
            "  {} {:<8} {:<9} {:<10} {}",
            green("✔"),
            x.platform,
            x.kind,
            x.arch.as_deref().unwrap_or("-"),
            dim(&x.file)
        );
    }
    output::ok(&format!(
        "wrote {} and {} in {}",
        release::MANIFEST_FILE,
        release::PAGE_FILE,
        files.display()
    ));
    Ok(())
}

pub async fn install(file: &Path, start: bool) -> Result<()> {
    let apps = bundle::apps_dir();
    let (m, dir) =
        bundle::install(file, &apps).with_context(|| format!("cannot install {}", file.display()))?;
    output::ok(&format!("installed {} {} → {}", bold(&m.app), m.version, dir.display()));
    if start {
        return super::run::run(&dir, openshell_core::config::Mode::Production, true, false).await;
    }
    println!("  {} {}", dim("start:"), cyan(&format!("openshell start --headless -p \"{}\"", dir.display())));
    Ok(())
}

pub fn apps() -> Result<()> {
    let dir = bundle::apps_dir();
    let list = bundle::installed(&dir);
    if list.is_empty() {
        println!(
            "no apps installed in {} (install one with `openshell install <file.osapp>`)",
            dir.display()
        );
        return Ok(());
    }
    for (m, path) in list {
        println!("  {:<24} {:<10} {}", bold(&m.app), m.version, dim(&path.display().to_string()));
    }
    Ok(())
}

pub async fn update(dir: &Path) -> Result<()> {
    let project = load_project(dir)?;
    let Some(url) = project.config.package.update_url.clone() else {
        bail!("no `package.update_url` in openshell.json – point it at the downloads.json of your latest release");
    };
    let current = project.config.app.version.clone().unwrap_or_else(|| "0.0.0".into());
    let caps = openshell_core::runtime::default_adapter().capabilities();
    let info = release::check_update(&url, &current, &caps.os, &caps.arch).await?;
    if !info.available {
        output::ok(&format!("{} {} is up to date", project.config.app.name, info.current));
        return Ok(());
    }
    println!("{} {} → {}", yellow("update available:"), info.current, bold(&info.latest));
    match &info.download {
        Some(d) => println!("  {} {}", dim("download:"), d.url),
        None => println!("  {} {}", dim("downloads:"), info.page),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn workflow_is_pinned_and_covers_every_platform() {
        let w = super::workflow_text();
        assert!(!w.contains("__OPENSHELL_VERSION__"));
        assert!(w.contains(env!("CARGO_PKG_VERSION")));
        for needle in [
            "windows-latest",
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "appimage",
            "--target bundle",
            "release manifest",
        ] {
            assert!(w.contains(needle), "{needle}");
        }
    }
}
