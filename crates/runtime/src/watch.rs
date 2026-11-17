//! File watching for development mode (restart services / reload the frontend).

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Keeps the watcher alive; dropping it stops watching.
pub struct WatchHandle {
    _debouncer: Debouncer<RecommendedWatcher>,
}

const IGNORED_DIRS: [&str; 10] = [
    "node_modules",
    ".git",
    "__pycache__",
    ".openshell",
    "target",
    ".venv",
    "venv",
    ".pytest_cache",
    ".mypy_cache",
    ".next",
];

pub(crate) fn is_ignored(path: &Path) -> bool {
    if path.components().any(|c| {
        c.as_os_str()
            .to_str()
            .map(|s| IGNORED_DIRS.contains(&s))
            .unwrap_or(false)
    }) {
        return true;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    name.ends_with(".pyc")
        || name.ends_with(".log")
        || name.ends_with('~')
        || name.ends_with(".swp")
        || name.ends_with(".tmp")
        || name.starts_with(".#")
}

pub fn watch_paths(
    paths: &[PathBuf],
    on_change: impl Fn(Vec<PathBuf>) + Send + 'static,
) -> anyhow::Result<WatchHandle> {
    let mut debouncer = new_debouncer(
        Duration::from_millis(400),
        move |res: DebounceEventResult| {
            if let Ok(events) = res {
                let mut changed: Vec<PathBuf> = events
                    .into_iter()
                    .map(|e| e.path)
                    .filter(|p| !is_ignored(p))
                    .collect();
                changed.sort();
                changed.dedup();
                if !changed.is_empty() {
                    on_change(changed);
                }
            }
        },
    )?;
    for p in paths {
        if p.exists() {
            debouncer.watcher().watch(p, RecursiveMode::Recursive)?;
        } else {
            tracing::warn!("watch path {} does not exist", p.display());
        }
    }
    Ok(WatchHandle {
        _debouncer: debouncer,
    })
}
