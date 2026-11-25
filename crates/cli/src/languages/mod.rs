//! Backend languages: one module per language, each with its detector for `openshell init`.
//!
//! The data about a language – its runtime, how well it is tested, its templates – lives in
//! [`openshell_core::config::runtimes::LANGUAGES`]; this module adds the part that needs code:
//! recognising a project of that language and proposing a service for it. To add a language,
//! add an entry to `LANGUAGES`, a module here and a line in [`DETECTORS`].

mod cpp;
mod dotnet;
mod go;
mod java;
mod node;
mod python;
mod rust;

use openshell_core::config::runtimes::{language, LanguageDef};
use serde_json::Value;
use std::path::Path;

/// A backend service proposed by a detector.
pub struct Backend {
    pub dir: String,
    pub command: Value,
    pub port: u16,
    /// Variant shown to the user, e.g. `Java (Spring Boot, Maven)`.
    pub flavor: String,
    pub language: &'static LanguageDef,
    pub build: Option<Value>,
    pub healthcheck: Option<String>,
    pub openapi: Option<String>,
    pub watch: Option<String>,
    pub note: Option<String>,
}

impl Backend {
    pub fn new(language_id: &str, dir: &str, command: Value, port: u16, flavor: &str) -> Self {
        Backend {
            dir: dir.to_string(),
            command,
            port,
            flavor: flavor.into(),
            language: language(language_id).expect("language in LANGUAGES"),
            build: None,
            healthcheck: None,
            openapi: None,
            watch: None,
            note: None,
        }
    }
}

/// Where a detector looks.
pub struct Ctx<'a> {
    pub dir: &'a Path,
    /// `dir` relative to the project root (`backend`, `.`).
    pub rel_dir: &'a str,
    /// The frontend's folder, if one was found (its package.json is not a backend).
    pub frontend_dir: Option<&'a str>,
}

impl Ctx<'_> {
    pub fn is_root(&self) -> bool {
        self.rel_dir == "."
    }
}

pub struct Detector {
    pub language: &'static str,
    pub detect: fn(&Ctx) -> Option<Backend>,
}

/// Detectors in the order of [`LANGUAGES`](openshell_core::config::runtimes::LANGUAGES).
pub const DETECTORS: &[Detector] = &[
    java::DETECTOR,
    cpp::DETECTOR,
    python::DETECTOR,
    go::DETECTOR,
    rust::DETECTOR,
    dotnet::DETECTOR,
    node::DETECTOR,
];

/// The first backend found in the usual folders.
pub fn detect_backend(root: &Path, frontend_dir: Option<&str>) -> Option<Backend> {
    for d in ["backend", "server", "api", "."] {
        let dir = root.join(d);
        if !dir.is_dir() {
            continue;
        }
        let rel_dir = super::commands::rel_path(root, &dir);
        let ctx = Ctx { dir: &dir, rel_dir: &rel_dir, frontend_dir };
        if let Some(b) = DETECTORS.iter().find_map(|det| (det.detect)(&ctx)) {
            return Some(b);
        }
    }
    None
}

pub(crate) fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(p).ok()?).ok()
}

pub(crate) fn package_manager(dir: &Path) -> &'static str {
    if dir.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if dir.join("yarn.lock").exists() {
        "yarn"
    } else if dir.join("bun.lockb").exists() || dir.join("bun.lock").exists() {
        "bun"
    } else {
        "npm"
    }
}

/// A short line for `openshell init`: language, support level and what the target machine needs.
pub fn describe(b: &Backend) -> String {
    let l = b.language;
    let mut s = format!("backend: {} in `{}` (port {}, {})", b.flavor, b.dir, b.port, l.support.label());
    if let Some(rt) = l.runtime {
        let bundle = openshell_core::config::runtimes::runtime(rt).and_then(|r| r.bundle).is_some();
        if bundle {
            s.push_str(&format!(
                " – ship the runtime with `openshell runtime add {rt}` or require it on the machine"
            ));
        }
    }
    s
}

/// `requires` entry proposed for a detected backend: the major version found on this machine.
pub fn suggested_requires(b: &Backend, cwd: &Path) -> Option<(String, String)> {
    let rt = openshell_core::config::runtimes::runtime(b.language.runtime?)?;
    let (v, _) = openshell_core::runtime::runtimes::probe_version(rt, None, cwd)?;
    Some((rt.name.to_string(), format!(">={}", v.0.first()?)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use openshell_core::config::runtimes::LANGUAGES;

    #[test]
    fn every_language_has_a_detector_in_order() {
        let ids: Vec<&str> = DETECTORS.iter().map(|d| d.language).collect();
        let langs: Vec<&str> = LANGUAGES.iter().map(|l| l.id).collect();
        assert_eq!(ids, langs);
    }

    fn detect(files: &[(&str, &str)]) -> Backend {
        let dir = tempfile::tempdir().unwrap();
        for (f, c) in files {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, c).unwrap();
        }
        detect_backend(dir.path(), None).expect("detected")
    }

    #[test]
    fn detects_each_language() {
        assert_eq!(detect(&[("backend/main.py", "")]).language.id, "python");
        assert_eq!(detect(&[("server/go.mod", "module x")]).language.id, "go");
        assert_eq!(detect(&[("backend/Cargo.toml", "[package]")]).language.id, "rust");
        assert_eq!(detect(&[("backend/server.js", "")]).language.id, "node");
        let net = detect(&[("api/Api.csproj", "<Project><ItemGroup><PackageReference Include=\"Swashbuckle.AspNetCore\" /></ItemGroup></Project>")]);
        assert_eq!(net.language.id, "dotnet");
        assert_eq!(net.openapi.as_deref(), Some("/swagger/v1/swagger.json"));
        assert!(net.command.to_string().contains("--urls"));
    }
}
