//! `openshell runtime` – language runtimes shipped inside the app.
//!
//! ```text
//! openshell runtime                 # declared runtimes, installed versions, requirement checks
//! openshell runtime add node 22     # declare in openshell.json and download into runtimes/node
//! openshell runtime install         # download every declared runtime missing on this system
//! openshell runtime remove node     # drop it from openshell.json and delete the folder
//! ```

use super::add::ConfigFile;
use super::load_project;
use crate::output::{self, bold, dim, green, red, yellow};
use anyhow::{bail, Result};
use openshell_core::config::runtimes::{runtime, RUNTIMES};
use openshell_core::runtime::runtimes::{bundled, check_requirements};
use serde_json::json;
use std::path::Path;

pub enum Action {
    List,
    Add { name: String, version: Option<String>, path: Option<String>, install: bool },
    Install { name: Option<String>, force: bool },
    Remove { name: String, keep_files: bool },
}

pub async fn run(dir: &Path, action: Action) -> Result<()> {
    match action {
        Action::List => list(dir),
        Action::Add { name, version, path, install: do_install } => {
            add(dir, &name, version, path)?;
            if do_install {
                install(dir, Some(name), false).await
            } else {
                println!("  {} run {} to download it", dim("•"), bold("openshell runtime install"));
                Ok(())
            }
        }
        Action::Install { name, force } => install(dir, name, force).await,
        Action::Remove { name, keep_files } => remove(dir, &name, keep_files),
    }
}

fn bundleable() -> String {
    RUNTIMES.iter().filter(|r| r.bundle.is_some()).map(|r| r.name).collect::<Vec<_>>().join(", ")
}

fn list(dir: &Path) -> Result<()> {
    let p = load_project(dir)?;
    let declared = bundled(&p);
    println!("{}", bold("Bundled runtimes"));
    if declared.is_empty() {
        println!(
            "  {} none – the app uses the runtimes installed on the machine\n  {} {}",
            dim("·"),
            dim("ship one with:"),
            bold("openshell runtime add python|node|java [version]")
        );
    }
    for b in &declared {
        let wanted = b.version.clone().unwrap_or_else(|| {
            runtime(&b.name)
                .and_then(|r| r.bundle)
                .map(|x| format!("{} (default)", x.default_version))
                .unwrap_or_default()
        });
        match openshell_core::runtimes::installed(&b.dir) {
            Some(i) if i.os == std::env::consts::OS && i.arch == std::env::consts::ARCH => println!(
                "  {} {:<8} {} {}",
                green("✔"),
                b.name,
                i.version,
                dim(&format!("wanted {wanted} · {} · {}", super::rel_path(&p.root, &b.dir), i.source))
            ),
            Some(i) => println!(
                "  {} {:<8} {}",
                yellow("!"),
                b.name,
                dim(&format!(
                    "installed for {}/{}, this is {}/{} – run `openshell runtime install --force`",
                    i.os,
                    i.arch,
                    std::env::consts::OS,
                    std::env::consts::ARCH
                ))
            ),
            None if b.installed() => println!(
                "  {} {:<8} {}",
                green("✔"),
                b.name,
                dim(&format!("provided by you in {}", super::rel_path(&p.root, &b.dir)))
            ),
            None => println!(
                "  {} {:<8} {}",
                red("✘"),
                b.name,
                dim(&format!("wanted {wanted}, not installed – run `openshell runtime install`"))
            ),
        }
    }
    let checks = check_requirements(&p);
    if !checks.is_empty() {
        println!("\n{}", bold("Requirements (`requires`)"));
        for c in checks {
            let mark = if c.ok { green("✔") } else { red("✘") };
            println!("  {mark} {:<10} {}", c.owner, c.message());
        }
    }
    Ok(())
}

fn add(dir: &Path, name: &str, version: Option<String>, path: Option<String>) -> Result<()> {
    let Some(def) = runtime(name) else {
        bail!("unknown runtime `{name}` (bundled runtimes: {})", bundleable());
    };
    let Some(bundle) = def.bundle else {
        bail!(
            "{} cannot be bundled; compile to a self-contained program instead (bundled runtimes: {})",
            def.title,
            bundleable()
        );
    };
    let mut cfg = ConfigFile::open(dir)?;
    if !cfg.value.is_object() {
        bail!("openshell.json is not a JSON object");
    }
    let obj = cfg.value.as_object_mut().unwrap();
    let runtimes = obj.entry("runtimes").or_insert_with(|| json!({}));
    if !runtimes.is_object() {
        *runtimes = json!({});
    }
    let version = version.unwrap_or_else(|| bundle.default_version.to_string());
    runtimes.as_object_mut().unwrap().insert(
        name.to_string(),
        match &path {
            Some(p) => json!({ "version": version, "path": p }),
            None => json!(version),
        },
    );
    cfg.save()?;
    output::ok(&format!("added {} {} to openshell.json ({})", bold(def.title), version, dim(bundle.source)));
    let folder = path.clone().unwrap_or_else(|| "runtimes/".to_string());
    ignore_in_git(&cfg.root, &folder)?;
    Ok(())
}

/// Downloaded runtimes do not belong in version control: add the folder to an existing .gitignore.
fn ignore_in_git(root: &Path, folder: &str) -> Result<()> {
    let gi = root.join(".gitignore");
    if !gi.exists() && !root.join(".git").exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&gi).unwrap_or_default();
    let entry = format!("{}/", folder.trim_end_matches('/'));
    if text.lines().any(|l| l.trim() == entry || l.trim() == entry.trim_end_matches('/')) {
        return Ok(());
    }
    let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    std::fs::write(&gi, format!("{text}{sep}# bundled runtimes (openshell runtime install)\n{entry}\n"))?;
    println!("  {} added {} to .gitignore", dim("•"), entry);
    Ok(())
}

async fn install(dir: &Path, name: Option<String>, force: bool) -> Result<()> {
    let p = load_project(dir)?;
    let names: Vec<String> = match name {
        Some(n) => vec![n],
        None => p.config.runtimes.keys().cloned().collect(),
    };
    if names.is_empty() {
        println!("no runtimes declared – add one with {}", bold("openshell runtime add python|node|java"));
        return Ok(());
    }
    let mut failed = 0;
    for n in names {
        println!("{}", bold(&n));
        match openshell_core::runtimes::install(&p, &n, force, |line| println!("  {} {line}", dim("·"))).await
        {
            Ok(i) => {
                output::ok(&format!("{} {} ready in {}", n, i.version, dim(&p.config.runtimes[&n].path(&n))))
            }
            Err(e) => {
                failed += 1;
                output::error(&format!("{n}: {e:#}"));
            }
        }
    }
    if failed > 0 {
        bail!("{failed} runtime(s) could not be installed");
    }
    println!(
        "\n  {} services now use the bundled runtimes; `openshell package` ships them with the app",
        dim("•")
    );
    Ok(())
}

fn remove(dir: &Path, name: &str, keep_files: bool) -> Result<()> {
    let p = load_project(dir)?;
    let Some(entry) = p.config.runtimes.get(name) else {
        bail!("`{name}` is not declared in `runtimes`");
    };
    let folder = p.root.join(entry.path(name));
    let mut cfg = ConfigFile::open(dir)?;
    if let Some(r) = cfg.value.get_mut("runtimes").and_then(|r| r.as_object_mut()) {
        r.remove(name);
        if r.is_empty() {
            cfg.value.as_object_mut().unwrap().remove("runtimes");
        }
    }
    cfg.save()?;
    output::ok(&format!("removed `{name}` from openshell.json"));
    if !keep_files && folder.is_dir() {
        // only folders inside the project, and only ones OpenShell installed
        if folder.starts_with(&p.root) && openshell_core::runtimes::installed(&folder).is_some() {
            std::fs::remove_dir_all(&folder)?;
            output::ok(&format!("deleted {}", super::rel_path(&p.root, &folder)));
        } else {
            println!("  {} kept {} (not installed by OpenShell)", dim("•"), folder.display());
        }
    }
    Ok(())
}
