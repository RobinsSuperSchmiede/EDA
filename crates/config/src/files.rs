//! Which project files belong into a package or app bundle.

use crate::Project;
use globset::{Glob, GlobSet, GlobSetBuilder};

/// Never shipped: VCS data, OpenShell state, dependency caches, editor/OS junk.
pub const DEFAULT_EXCLUDES: &[&str] = &[
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

fn invalid(e: globset::Error) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
}

fn globs(patterns: &[String]) -> std::io::Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        let p = p.trim_start_matches("./").trim_end_matches('/');
        b.add(Glob::new(p).map_err(invalid)?);
        // a plain directory name includes everything below it
        if !p.contains('*') {
            b.add(Glob::new(&format!("{p}/**")).map_err(invalid)?);
        }
    }
    b.build().map_err(invalid)
}

/// Relative paths (with `/`) of all files that belong into the package.
pub fn package_files(project: &Project) -> std::io::Result<Vec<String>> {
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
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| std::io::Error::new(e.kind(), format!("reading {}: {e}", dir.display())))?;
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let rel = path.strip_prefix(&project.root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
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
