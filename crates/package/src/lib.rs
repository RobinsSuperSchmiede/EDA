//! # openshell-package
//!
//! Builds native installers for OpenShell applications **without compiling anything
//! application specific**: the generic `openshell-desktop` binary is copied, renamed
//! and re-branded (icon), the application files are placed in `app/` next to it, and
//! the Tauri bundler produces the installer (NSIS/MSI on Windows, .app/.dmg on macOS,
//! .deb/.rpm/.AppImage on Linux).
//!
//! Installed layout (Windows example):
//!
//! ```text
//! C:\Users\<user>\AppData\Local\Programs\My App\
//!   my-app.exe          <- openshell-desktop with the app's icon
//!   app\openshell.json  <- the application (frontend build, backend, ...)
//!   app\...
//! ```
//!
//! At runtime the binary finds `app/openshell.json` and keeps all state (logs, audit log,
//! `OPENSHELL_DATA_DIR`) in a per-user data directory named after the bundle identifier.

use anyhow::{bail, Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use image::{imageops::FilterType, DynamicImage, ImageFormat};
use openshell_config::Project;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri_bundler::bundle::{
    BundleBinary, BundleSettings, NsisSettings, PackageSettings, PackageType, SettingsBuilder, WindowsSettings,
};

/// Default OpenShell icon (used when `app.icon` is not set).
const DEFAULT_ICON: &[u8] = include_bytes!("../../../apps/desktop/src-tauri/icons/icon.png");

/// Never shipped: VCS data, OpenShell state, dependency caches, editor/OS junk.
const DEFAULT_EXCLUDES: &[&str] = &[
    "**/.git/**",
    ".openshell/**",
    "**/.openshell/**",
    "**/node_modules/**",
    "**/__pycache__/**",
    "**/*.pyc",
    "**/.venv/**",
    "**/venv/**",
    "**/.pytest_cache/**",
    "**/.mypy_cache/**",
    "**/.idea/**",
    "**/.vscode/**",
    "**/.DS_Store",
    "**/Thumbs.db",
    "**/.gradle/**",
];

pub struct Options {
    /// The generic desktop shell binary (`openshell-desktop`).
    pub desktop_binary: PathBuf,
    /// Installer formats; empty = platform default (or `package.targets`).
    pub targets: Vec<String>,
    /// Copy the finished installers here.
    pub out_dir: Option<PathBuf>,
}

pub struct Output {
    pub installers: Vec<PathBuf>,
    /// Staging directory: `bin/<app>` + `bin/app/` is a runnable portable build.
    pub staging: PathBuf,
    pub files: usize,
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_start_matches("./").trim_end_matches('/');
        b.add(Glob::new(p)?);
        // a plain directory name includes everything below it
        if !p.contains('*') {
            b.add(Glob::new(&format!("{p}/**"))?);
        }
    }
    Ok(b.build()?)
}

/// Relative paths (with `/`) of all files that belong into the package.
pub fn collect_files(project: &Project) -> Result<Vec<String>> {
    let cfg = &project.config.package;
    let mut ex: Vec<String> = DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect();
    ex.extend(cfg.exclude.iter().cloned());
    let exclude = globs(&ex)?;
    let include = if cfg.include.is_empty() { None } else { Some(globs(&cfg.include)?) };
    let config_name = project
        .config_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "openshell.json".into());

    let mut files = Vec::new();
    let mut stack = vec![project.root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
            let entry = entry?;
            let path = entry.path();
            let rel = path
                .strip_prefix(&project.root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let ft = entry.file_type()?;
            if ft.is_dir() {
                // prune excluded directories early
                if exclude.is_match(format!("{rel}/x")) && exclude.is_match(format!("{rel}/x/y")) {
                    continue;
                }
                stack.push(path);
            } else if ft.is_file() {
                if rel == config_name {
                    files.push(rel);
                    continue;
                }
                if exclude.is_match(&rel) {
                    continue;
                }
                if let Some(inc) = &include {
                    if !inc.is_match(&rel) {
                        continue;
                    }
                }
                files.push(rel);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn default_targets() -> Vec<String> {
    if cfg!(windows) {
        vec!["nsis".into()]
    } else if cfg!(target_os = "macos") {
        vec!["app".into(), "dmg".into()]
    } else {
        vec!["deb".into(), "appimage".into()]
    }
}

fn package_type(name: &str) -> Result<PackageType> {
    PackageType::from_short_name(name).with_context(|| format!("unknown installer format `{name}`"))
}

/// Write the icon set (PNG sizes + ICO) and return (png paths, ico path, source png).
fn write_icons(project: &Project, dir: &Path) -> Result<(Vec<String>, PathBuf, PathBuf)> {
    std::fs::create_dir_all(dir)?;
    let source: DynamicImage = match &project.config.app.icon {
        Some(icon) => {
            let p = project.resolve_path(icon);
            image::open(&p).with_context(|| format!("cannot read app.icon {}", p.display()))?
        }
        None => image::load_from_memory_with_format(DEFAULT_ICON, ImageFormat::Png)?,
    };
    let source = if source.width() != source.height() {
        let s = source.width().min(source.height());
        source.crop_imm(0, 0, s, s)
    } else {
        source
    };
    let mut pngs = Vec::new();
    for (name, size) in [("32x32.png", 32), ("128x128.png", 128), ("128x128@2x.png", 256), ("icon.png", 512)] {
        let path = dir.join(name);
        source.resize_exact(size, size, FilterType::Lanczos3).save(&path)?;
        pngs.push(path.display().to_string());
    }
    // ICO with the usual Windows sizes
    let ico = dir.join("icon.ico");
    let frames: Vec<image::codecs::ico::IcoFrame> = [16u32, 24, 32, 48, 64, 128, 256]
        .iter()
        .map(|&s| {
            let img = source.resize_exact(s, s, FilterType::Lanczos3).to_rgba8();
            image::codecs::ico::IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8)
        })
        .collect::<Result<_, _>>()?;
    let file = std::fs::File::create(&ico)?;
    image::codecs::ico::IcoEncoder::new(file).encode_images(&frames)?;
    Ok((pngs, ico, dir.join("icon.png")))
}

/// Replace the icon embedded in a Windows executable.
fn brand_exe(exe: &Path, icon_png: &Path) -> Result<()> {
    let mut image = editpe::Image::parse_file(exe).context("cannot parse the desktop executable")?;
    let mut resources = image.resource_directory().cloned().unwrap_or_default();
    resources
        .set_main_icon_file(&icon_png.display().to_string())
        .context("cannot set the executable icon")?;
    image.set_resource_directory(resources)?;
    image.write_file(exe)?;
    Ok(())
}

fn semver(v: &str) -> bool {
    let core = v.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| p.parse::<u64>().is_ok())
}

/// Build the installer(s) for `project`. `log` receives progress messages.
pub fn package(project: &Project, opts: &Options, log: &dyn Fn(&str)) -> Result<Output> {
    let cfg = &project.config;
    let name = cfg.app.name.clone();
    let slug = cfg.slug();
    let version = cfg.app.version.clone().unwrap_or_else(|| "0.1.0".into());
    if !semver(&version) {
        bail!("app.version `{version}` must be a semantic version like 1.2.3 for installers");
    }
    if !opts.desktop_binary.is_file() {
        bail!("desktop binary {} not found (build it with `cargo build --release -p openshell-desktop`)", opts.desktop_binary.display());
    }
    let staging = project.state_dir().join("package");
    if staging.exists() {
        std::fs::remove_dir_all(&staging).with_context(|| format!("cannot clean {}", staging.display()))?;
    }
    let bin_dir = staging.join("bin");
    let app_dir = bin_dir.join("app");
    std::fs::create_dir_all(&app_dir)?;

    // 1. application files
    let files = collect_files(project)?;
    for rel in &files {
        let src = project.root.join(rel);
        let dst = app_dir.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&src, &dst).with_context(|| format!("copying {rel}"))?;
    }
    log(&format!("staged {} application file(s)", files.len()));

    // 2. icons
    let (pngs, ico, icon_png) = write_icons(project, &staging.join("icons"))?;

    // 3. branded shell binary
    let exe_name = if cfg!(windows) { format!("{slug}.exe") } else { slug.clone() };
    let exe = bin_dir.join(&exe_name);
    std::fs::copy(&opts.desktop_binary, &exe)?;
    if cfg!(windows) {
        brand_exe(&exe, &icon_png)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
    }
    log(&format!("prepared {exe_name} (portable build: {})", bin_dir.display()));

    // 4. installers
    let targets = if !opts.targets.is_empty() {
        opts.targets.clone()
    } else if !cfg.package.targets.is_empty() {
        cfg.package.targets.clone()
    } else {
        default_targets()
    };
    let package_types = targets.iter().map(|t| package_type(t)).collect::<Result<Vec<_>>>()?;
    let mut resources = HashMap::new();
    resources.insert(app_dir.display().to_string(), "app".to_string());
    let license_file = cfg.package.license_file.as_ref().map(|l| project.resolve_path(l));
    let bundle = BundleSettings {
        identifier: Some(cfg.identifier()),
        publisher: cfg.package.publisher.clone(),
        homepage: cfg.package.homepage.clone(),
        icon: Some(pngs.into_iter().chain([ico.display().to_string()]).collect()),
        resources_map: Some(resources),
        copyright: cfg.package.copyright.clone(),
        license_file,
        short_description: cfg.app.description.clone(),
        #[allow(deprecated)]
        windows: WindowsSettings {
            icon_path: ico.clone(),
            nsis: Some(NsisSettings {
                start_menu_folder: Some(name.clone()),
                ..Default::default()
            }),
            ..Default::default()
        },
        ..Default::default()
    };
    let settings = SettingsBuilder::new()
        .project_out_directory(&bin_dir)
        .package_types(package_types)
        .package_settings(PackageSettings {
            product_name: name.clone(),
            version,
            description: cfg.app.description.clone().unwrap_or_else(|| name.clone()),
            homepage: cfg.package.homepage.clone(),
            authors: cfg.package.publisher.clone().map(|p| vec![p]),
            default_run: None,
        })
        .bundle_settings(bundle)
        .binaries(vec![BundleBinary::new(slug.clone(), true)])
        .build()
        .context("invalid installer settings")?;
    log(&format!("building installer(s): {}", targets.join(", ")));
    let bundles = tauri_bundler::bundle_project(&settings).context("the bundler failed")?;
    let mut installers: Vec<PathBuf> = bundles.into_iter().flat_map(|b| b.bundle_paths).collect();

    if let Some(out) = &opts.out_dir {
        std::fs::create_dir_all(out)?;
        let mut copied = Vec::new();
        for i in &installers {
            if i.is_file() {
                let dst = out.join(i.file_name().unwrap_or_default());
                std::fs::copy(i, &dst)?;
                copied.push(dst);
            } else {
                copied.push(i.clone());
            }
        }
        installers = copied;
    }
    Ok(Output {
        installers,
        staging: bin_dir,
        files: files.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_files_with_excludes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for f in [
            "openshell.json",
            "web/index.html",
            "backend/main.py",
            "backend/__pycache__/main.cpython-313.pyc",
            "frontend/node_modules/react/index.js",
            ".openshell/runtime.json",
            ".git/HEAD",
            "notes/secret.txt",
        ] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::write(
            root.join("openshell.json"),
            r#"{"app":{"name":"Pkg Test"},"package":{"exclude":["notes"]}}"#,
        )
        .unwrap();
        let project = Project::load(root).unwrap();
        let files = collect_files(&project).unwrap();
        assert_eq!(files, vec!["backend/main.py", "openshell.json", "web/index.html"]);

        std::fs::write(
            root.join("openshell.json"),
            r#"{"app":{"name":"Pkg Test"},"package":{"include":["web"]}}"#,
        )
        .unwrap();
        let project = Project::load(root).unwrap();
        assert_eq!(collect_files(&project).unwrap(), vec!["openshell.json", "web/index.html"]);
    }

    #[test]
    fn writes_icons() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("openshell.json"), r#"{"app":{"name":"Icons"}}"#).unwrap();
        let project = Project::load(dir.path()).unwrap();
        let (pngs, ico, _) = write_icons(&project, &dir.path().join("icons")).unwrap();
        assert_eq!(pngs.len(), 4);
        assert!(std::fs::metadata(ico).unwrap().len() > 1000);
    }
}
