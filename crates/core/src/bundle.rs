//! Portable app bundles (`.osapp`): one file that runs the unchanged app on every OpenShell
//! host – desktop, server, Termux and the Android app.
//!
//! A bundle is a zip archive with the project files (the same selection as installers, see
//! `package.include` / `package.exclude`) and `openshell-bundle.json`:
//!
//! ```json
//! { "format": 1, "app": "My App", "identifier": "com.example.myapp", "version": "1.2.0",
//!   "created": "2026-11-24T10:00:00Z", "openshell": "0.3.0", "files": 42 }
//! ```
//!
//! `openshell install app.osapp` unpacks it into the per-user app directory
//! (`~/.openshell/apps/<identifier>/`), keeping the app's data across updates.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use openshell_config::Project;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "openshell-bundle.json";
pub const EXTENSION: &str = "osapp";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BundleManifest {
    pub format: u32,
    pub app: String,
    pub identifier: String,
    pub version: String,
    pub created: DateTime<Utc>,
    /// OpenShell version that built the bundle.
    pub openshell: String,
    pub files: usize,
}

/// `<slug>-<version>.osapp`
pub fn file_name(project: &Project) -> String {
    let version = project.config.app.version.clone().unwrap_or_else(|| "0.1.0".into());
    format!("{}-{version}.{EXTENSION}", project.config.slug())
}

/// Write the bundle of `project` into `out_dir`; returns its path.
pub fn create(project: &Project, out_dir: &Path) -> Result<PathBuf> {
    let files = openshell_config::package_files(project)?;
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(file_name(project));
    let manifest = BundleManifest {
        format: 1,
        app: project.config.app.name.clone(),
        identifier: project.config.identifier(),
        version: project.config.app.version.clone().unwrap_or_else(|| "0.1.0".into()),
        created: Utc::now(),
        openshell: env!("CARGO_PKG_VERSION").into(),
        files: files.len(),
    };
    let file = std::fs::File::create(&path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file(MANIFEST, options.unix_permissions(0o644))?;
    zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
    for rel in &files {
        let src = project.root.join(rel);
        let mode = file_mode(&src);
        zip.start_file(rel.as_str(), options.unix_permissions(mode))?;
        let mut f = std::fs::File::open(&src).with_context(|| format!("cannot read {rel}"))?;
        std::io::copy(&mut f, &mut zip)?;
    }
    zip.finish()?;
    Ok(path)
}

#[cfg(unix)]
fn file_mode(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644)
}

#[cfg(not(unix))]
fn file_mode(p: &Path) -> u32 {
    // keep scripts and binaries executable on Unix hosts
    let exec = matches!(p.extension().and_then(|e| e.to_str()), Some("sh" | "py" | "bin"))
        || p.parent().and_then(|d| d.file_name()).is_some_and(|d| d == "bin");
    if exec {
        0o755
    } else {
        0o644
    }
}

/// Read the manifest of a bundle without unpacking it.
pub fn read_manifest(bundle: &Path) -> Result<BundleManifest> {
    let file = std::fs::File::open(bundle).with_context(|| format!("cannot open {}", bundle.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("not an OpenShell app bundle (zip expected)")?;
    let mut entry = zip.by_name(MANIFEST).map_err(|_| anyhow!("{MANIFEST} is missing in the bundle"))?;
    let mut text = String::new();
    entry.read_to_string(&mut text)?;
    let m: BundleManifest = serde_json::from_str(&text).context("invalid bundle manifest")?;
    if m.format != 1 {
        bail!("bundle format {} is not supported by this OpenShell version", m.format);
    }
    Ok(m)
}

/// Where installed bundles live: `$OPENSHELL_APPS_DIR` or `~/.openshell/apps`.
pub fn apps_dir() -> PathBuf {
    if let Ok(d) = std::env::var("OPENSHELL_APPS_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default();
    PathBuf::from(home).join(".openshell").join("apps")
}

/// Unpack a bundle into `apps_dir/<identifier>/`. An older version there is replaced; the
/// app's state (`.openshell/`, with its databases and data) is kept.
pub fn install(bundle: &Path, apps_dir: &Path) -> Result<(BundleManifest, PathBuf)> {
    let manifest = read_manifest(bundle)?;
    let id = &manifest.identifier;
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        bail!("invalid identifier `{id}` in the bundle");
    }
    let target = apps_dir.join(id);
    let staging = apps_dir.join(format!(".{id}.new"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    let file = std::fs::File::open(bundle)?;
    let mut zip = zip::ZipArchive::new(file)?;
    // `extract` rejects paths that leave the target directory
    zip.extract(&staging).context("cannot unpack the bundle")?;
    let _ = std::fs::remove_file(staging.join(MANIFEST));
    std::fs::write(staging.join(MANIFEST), serde_json::to_vec_pretty(&manifest)?)?;
    if target.exists() {
        let state = target.join(".openshell");
        if state.is_dir() {
            std::fs::rename(&state, staging.join(".openshell")).context("cannot keep the app's data")?;
        }
        std::fs::remove_dir_all(&target).with_context(|| format!("cannot replace {}", target.display()))?;
    }
    std::fs::rename(&staging, &target)?;
    Ok((manifest, target))
}

/// Installed bundles (manifest and directory).
pub fn installed(apps_dir: &Path) -> Vec<(BundleManifest, PathBuf)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(apps_dir).into_iter().flatten().flatten() {
        let dir = e.path();
        if let Ok(text) = std::fs::read_to_string(dir.join(MANIFEST)) {
            if let Ok(m) = serde_json::from_str::<BundleManifest>(&text) {
                out.push((m, dir));
            }
        }
    }
    out.sort_by(|a, b| a.0.app.cmp(&b.0.app));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_install_update_keeps_data() {
        let src = tempfile::tempdir().unwrap();
        let root = src.path();
        std::fs::write(
            root.join("openshell.json"),
            r#"{"app":{"name":"Notes","id":"com.example.notes","version":"1.0.0"},"frontend":{"dist":"web"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(root.join("web")).unwrap();
        std::fs::write(root.join("web/index.html"), "<h1>v1</h1>").unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::write(root.join("node_modules/x/i.js"), "x").unwrap();

        let out = tempfile::tempdir().unwrap();
        let project = Project::load(root).unwrap();
        let bundle = create(&project, out.path()).unwrap();
        assert_eq!(bundle.file_name().unwrap(), "notes-1.0.0.osapp");
        let m = read_manifest(&bundle).unwrap();
        assert_eq!((m.identifier.as_str(), m.version.as_str(), m.files), ("com.example.notes", "1.0.0", 2));

        let apps = tempfile::tempdir().unwrap();
        let (_, dir) = install(&bundle, apps.path()).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("web/index.html")).unwrap(), "<h1>v1</h1>");
        assert!(!dir.join("node_modules").exists());
        assert!(Project::load(&dir).is_ok(), "the unpacked bundle is a runnable project");

        // app data survives an update
        std::fs::create_dir_all(dir.join(".openshell/data")).unwrap();
        std::fs::write(dir.join(".openshell/data/notes.db"), "data").unwrap();
        std::fs::write(
            root.join("openshell.json"),
            r#"{"app":{"name":"Notes","id":"com.example.notes","version":"1.1.0"},"frontend":{"dist":"web"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("web/index.html"), "<h1>v2</h1>").unwrap();
        let bundle2 = create(&Project::load(root).unwrap(), out.path()).unwrap();
        let (m2, dir2) = install(&bundle2, apps.path()).unwrap();
        assert_eq!(dir2, dir);
        assert_eq!(m2.version, "1.1.0");
        assert_eq!(std::fs::read_to_string(dir.join("web/index.html")).unwrap(), "<h1>v2</h1>");
        assert_eq!(std::fs::read_to_string(dir.join(".openshell/data/notes.db")).unwrap(), "data");
        let list = installed(apps.path());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0.version, "1.1.0");
    }

    #[test]
    fn rejects_other_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.osapp");
        std::fs::write(&p, "not a zip").unwrap();
        assert!(read_manifest(&p).is_err());
    }
}
