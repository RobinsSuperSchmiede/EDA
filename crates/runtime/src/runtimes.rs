//! Language runtimes on this machine: bundled copies inside the app (`runtimes` in the config)
//! and version checks for `requires`.
//!
//! A bundled runtime is a folder in the project (`runtimes/python`, …). Its executable folders
//! are put in front of `PATH` for every service, setup step, build step, function and task, so
//! `python main.py` or `npm install` use the bundled copy instead of whatever the user has
//! installed. The data about each runtime lives in [`openshell_config::runtimes`].

use openshell_config::runtimes::{runtime, RuntimeDef, Version, VersionReq};
use openshell_config::{Project, FRONTEND_SERVICE};
use serde::Serialize;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// A runtime declared in `runtimes` and where it is (or would be) on disk.
#[derive(Debug, Clone, Serialize)]
pub struct BundledRuntime {
    pub name: String,
    /// Version requested in the config (`None`: the runtime's default).
    pub version: Option<String>,
    pub dir: PathBuf,
    /// Folders with executables that exist (empty if the runtime is not installed yet).
    pub bin_dirs: Vec<PathBuf>,
}

impl BundledRuntime {
    pub fn installed(&self) -> bool {
        !self.bin_dirs.is_empty()
    }
}

/// The runtimes declared in the project's `runtimes` section.
pub fn bundled(project: &Project) -> Vec<BundledRuntime> {
    project
        .config
        .runtimes
        .iter()
        .filter_map(|(name, entry)| {
            let def = runtime(name)?;
            let dir = project.root.join(entry.path(name));
            let bin_dirs = def
                .bin_dirs()
                .iter()
                .map(|b| if *b == "." { dir.clone() } else { dir.join(b) })
                .filter(|d| d.is_dir())
                .collect();
            Some(BundledRuntime {
                name: name.clone(),
                version: entry.version().map(str::to_string),
                dir,
                bin_dirs,
            })
        })
        .collect()
}

/// `PATH` with the executable folders of the installed bundled runtimes in front, or `None`
/// when the project bundles none.
pub fn path_with_bundled(project: &Project) -> Option<String> {
    let dirs: Vec<PathBuf> = bundled(project).into_iter().flat_map(|b| b.bin_dirs).collect();
    if dirs.is_empty() {
        return None;
    }
    let mut all: Vec<PathBuf> = dirs;
    if let Some(path) = std::env::var_os("PATH") {
        all.extend(std::env::split_paths(&path));
    }
    std::env::join_paths(all).ok().map(|p: OsString| p.to_string_lossy().into_owned())
}

/// Find a program in `path` (or the process `PATH`), including the `python` ↔ `python3` alias.
pub fn find_program(program: &str, path: Option<&str>, cwd: &Path) -> Option<PathBuf> {
    let search = path.map(OsString::from).or_else(|| std::env::var_os("PATH"));
    let try_one = |p: &str| which::which_in(p, search.clone(), cwd).ok();
    try_one(program).or_else(|| match program {
        "python" => try_one("python3"),
        "python3" => try_one("python"),
        _ => None,
    })
}

/// Ask a runtime for its version (`python --version`, `java -version`, …).
pub fn probe_version(def: &RuntimeDef, path: Option<&str>, cwd: &Path) -> Option<(Version, PathBuf)> {
    let program = find_program(def.program(), path, cwd)?;
    let mut cmd = std::process::Command::new(&program);
    cmd.args(def.version_args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(p) = path {
        cmd.env("PATH", p);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd.spawn().ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let out = rx.recv_timeout(Duration::from_secs(10)).ok()?.ok()?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    def.parse_version(&text).map(|v| (v, program))
}

/// Result of checking one `requires` entry.
#[derive(Debug, Clone, Serialize)]
pub struct RequirementCheck {
    /// Service name (`frontend` for the frontend).
    pub owner: String,
    pub runtime: String,
    pub required: String,
    pub found: Option<String>,
    pub program: Option<PathBuf>,
    /// The program comes from a bundled runtime.
    pub bundled: bool,
    pub ok: bool,
}

impl RequirementCheck {
    /// One line for logs and `openshell doctor`.
    pub fn message(&self) -> String {
        let title = runtime(&self.runtime).map(|r| r.title).unwrap_or(self.runtime.as_str());
        match (&self.found, self.ok) {
            (Some(v), true) => format!(
                "{title} {v}{} satisfies `{}`",
                if self.bundled { " (bundled)" } else { "" },
                self.required
            ),
            (Some(v), false) => format!("needs {title} {}, found {v}", self.required),
            (None, _) => format!("needs {title} {}, but it was not found", self.required),
        }
    }
}

/// Check every `requires` entry of the services and the frontend (blocking: runs the programs).
pub fn check_requirements(project: &Project) -> Vec<RequirementCheck> {
    let path = path_with_bundled(project);
    let bundled_dirs: Vec<PathBuf> = bundled(project).into_iter().flat_map(|b| b.bin_dirs).collect();
    let mut owners: Vec<(String, PathBuf, &std::collections::BTreeMap<String, String>)> = project
        .config
        .services
        .iter()
        .map(|s| (s.name.clone(), project.resolve_path(s.cwd.as_deref().unwrap_or(".")), &s.requires))
        .collect();
    if let Some(fe) = &project.config.frontend {
        owners.push((
            FRONTEND_SERVICE.to_string(),
            project.resolve_path(fe.cwd.as_deref().unwrap_or(".")),
            &fe.requires,
        ));
    }
    let mut cache: std::collections::HashMap<String, Option<(Version, PathBuf)>> = Default::default();
    let mut out = Vec::new();
    for (owner, cwd, requires) in owners {
        for (name, req) in requires {
            let (Some(def), Ok(parsed)) = (runtime(name), VersionReq::parse(req)) else { continue };
            let found = cache
                .entry(name.clone())
                .or_insert_with(|| {
                    probe_version(def, path.as_deref(), if cwd.is_dir() { &cwd } else { &project.root })
                })
                .clone();
            out.push(RequirementCheck {
                owner: owner.clone(),
                runtime: name.clone(),
                required: req.clone(),
                ok: found.as_ref().map(|(v, _)| parsed.matches(v)).unwrap_or(false),
                found: found.as_ref().map(|(v, _)| v.to_string()),
                bundled: found
                    .as_ref()
                    .map(|(_, p)| bundled_dirs.iter().any(|d| p.starts_with(d)))
                    .unwrap_or(false),
                program: found.map(|(_, p)| p),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use openshell_config::parse_json;

    fn project(dir: &Path, json: &str) -> Project {
        let config = parse_json(json).unwrap();
        Project {
            root: dir.to_path_buf(),
            config_path: dir.join("openshell.json"),
            config,
            state_dir_override: None,
        }
    }

    #[test]
    fn bundled_runtimes_go_first_in_path() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(
            dir.path(),
            r#"{ "app": { "name": "x" }, "runtimes": { "node": "22", "python": { "path": "vendor/py" } } }"#,
        );
        assert!(path_with_bundled(&p).is_none(), "nothing installed yet");
        let b = bundled(&p);
        assert_eq!(b.len(), 2);
        assert!(b.iter().all(|r| !r.installed()));

        // install a fake node: its bin folder (`bin` on Unix, the folder itself on Windows)
        let node_bin = if cfg!(windows) {
            dir.path().join("runtimes/node")
        } else {
            dir.path().join("runtimes/node/bin")
        };
        std::fs::create_dir_all(&node_bin).unwrap();
        let path = path_with_bundled(&p).unwrap();
        let first = std::env::split_paths(&path).next().unwrap();
        assert_eq!(first, node_bin);
        assert!(bundled(&p).iter().find(|r| r.name == "node").unwrap().installed());
        assert_eq!(
            bundled(&p).iter().find(|r| r.name == "python").unwrap().dir,
            dir.path().join("vendor/py")
        );
    }

    #[test]
    fn requirements_report_missing_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        // an empty PATH: nothing can be found, so the check fails with a clear message
        let p = project(
            dir.path(),
            r#"{ "app": { "name": "x" }, "services": [ { "name": "api", "command": ["python", "main.py"], "requires": { "python": ">=3.10" } } ] }"#,
        );
        let def = runtime("python").unwrap();
        assert!(probe_version(def, Some(""), dir.path()).is_none());
        let checks = check_requirements(&p);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].owner, "api");
        if !checks[0].ok {
            assert!(checks[0].message().contains("Python >=3.10"), "{}", checks[0].message());
        }
    }
}
