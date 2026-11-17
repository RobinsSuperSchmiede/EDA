//! Project-scoped file access for agents. Every path is relative to the project
//! root; traversal outside of it (via `..`, absolute paths or symlinks) and
//! access to OpenShell's own state is rejected.

use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};

const SKIP_DIRS: [&str; 9] = [
    "node_modules",
    ".git",
    "target",
    ".openshell",
    "__pycache__",
    ".venv",
    "venv",
    ".next",
    ".pytest_cache",
];

/// Resolve `rel` inside `root`. `for_write` additionally protects `.git` and `.openshell`.
pub fn resolve(root: &Path, rel: &str, for_write: bool) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel.trim());
    let mut out = PathBuf::new();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir => return Err(format!("`{rel}`: `..` is not allowed; paths are relative to the project root")),
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!("`{rel}`: use a path relative to the project root"))
            }
        }
    }
    if let Some(Component::Normal(first)) = out.components().next() {
        let first = first.to_string_lossy();
        if first == ".openshell" || (for_write && first == ".git") {
            return Err(format!("`{rel}` is protected"));
        }
    }
    let full = root.join(&out);
    // symlink escape check on the nearest existing ancestor
    let root_canon = dunce(root.canonicalize().map_err(|e| e.to_string())?);
    let mut probe = full.clone();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let canon = dunce(probe.canonicalize().map_err(|e| e.to_string())?);
    if !canon.starts_with(&root_canon) {
        return Err(format!("`{rel}` resolves outside of the project"));
    }
    Ok(full)
}

/// Strip the Windows `\\?\` prefix so `starts_with` comparisons work.
fn dunce(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if !rest.starts_with("UNC") {
            return PathBuf::from(rest);
        }
    }
    p
}

fn rel_display(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn list(root: &Path, rel: &str, recursive: bool, max: usize) -> Result<Value, String> {
    let dir = resolve(root, if rel.is_empty() { "." } else { rel }, false)?;
    if !dir.is_dir() {
        return Err(format!("`{rel}` is not a directory"));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    let mut stack = vec![dir];
    while let Some(d) = stack.pop() {
        let mut items: Vec<_> = match std::fs::read_dir(&d) {
            Ok(rd) => rd.flatten().collect(),
            Err(e) => return Err(e.to_string()),
        };
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let name = e.file_name().to_string_lossy().to_string();
            let path = e.path();
            let is_dir = path.is_dir();
            if is_dir && SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            if entries.len() >= max {
                truncated = true;
                break;
            }
            let size = if is_dir { None } else { e.metadata().ok().map(|m| m.len()) };
            entries.push(json!({
                "path": rel_display(root, &path),
                "type": if is_dir { "dir" } else { "file" },
                "size": size,
            }));
            if is_dir && recursive {
                stack.push(path);
            }
        }
    }
    Ok(json!({ "entries": entries, "truncated": truncated }))
}

pub fn read(root: &Path, rel: &str, max_bytes: usize) -> Result<Value, String> {
    let path = resolve(root, rel, false)?;
    let bytes = std::fs::read(&path).map_err(|e| format!("`{rel}`: {e}"))?;
    let total = bytes.len();
    let slice = &bytes[..total.min(max_bytes)];
    let text = String::from_utf8(slice.to_vec()).map_err(|_| format!("`{rel}` is not a UTF-8 text file"))?;
    Ok(json!({ "path": rel, "content": text, "size": total, "truncated": total > max_bytes }))
}

pub fn write(root: &Path, rel: &str, content: &str) -> Result<Value, String> {
    let path = resolve(root, rel, true)?;
    if path.is_dir() {
        return Err(format!("`{rel}` is a directory"));
    }
    let existed = path.exists();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| format!("`{rel}`: {e}"))?;
    Ok(json!({ "path": rel, "bytes": content.len(), "created": !existed }))
}

pub fn search(root: &Path, rel: &str, query: &str, max_results: usize) -> Result<Value, String> {
    let dir = resolve(root, if rel.is_empty() { "." } else { rel }, false)?;
    let needle = query.to_lowercase();
    if needle.is_empty() {
        return Err("query must not be empty".into());
    }
    let mut results = Vec::new();
    let mut stack = vec![dir];
    'outer: while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && name != "dist" {
                    stack.push(path);
                }
                continue;
            }
            if e.metadata().map(|m| m.len() > 1_000_000).unwrap_or(true) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            for (i, line) in text.lines().enumerate() {
                if line.to_lowercase().contains(&needle) {
                    results.push(json!({
                        "path": rel_display(root, &path),
                        "line": i + 1,
                        "text": line.trim().chars().take(300).collect::<String>(),
                    }));
                    if results.len() >= max_results {
                        break 'outer;
                    }
                }
            }
        }
    }
    let truncated = results.len() >= max_results;
    Ok(json!({ "results": results, "truncated": truncated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sandbox() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(resolve(root, "../x", false).is_err());
        assert!(resolve(root, "/etc/passwd", false).is_err());
        assert!(resolve(root, ".openshell/runtime.json", false).is_err());
        assert!(resolve(root, ".git/config", true).is_err());
        assert!(resolve(root, "src/new/file.txt", true).is_ok());

        write(root, "src/a.txt", "hello\nWorld line\n").unwrap();
        let r = read(root, "src/a.txt", 1000).unwrap();
        assert_eq!(r["content"], "hello\nWorld line\n");
        let l = list(root, "", true, 100).unwrap();
        assert!(l["entries"].to_string().contains("src/a.txt"));
        let s = search(root, "", "world", 10).unwrap();
        assert_eq!(s["results"][0]["line"], 2);
    }
}
