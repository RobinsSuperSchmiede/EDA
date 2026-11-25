//! Languages and runtimes known to OpenShell, as data.
//!
//! One entry per runtime holds everything the CLI, the runtime and `openshell doctor` need: the
//! programs it provides, how to ask it for its version, where executables live in a bundled copy
//! and which version is downloaded by default. One entry per language says which runtime it
//! needs, how well it is tested and which templates exist. Adding a language means adding an
//! entry here, a detector in the CLI (`crates/cli/src/languages/`) and – optionally – a template.
//!
//! Version requirements (`services[].requires`) use a small, dependency-free syntax:
//! `>=3.10`, `>3`, `<=21`, `<4`, `=3.12.1`, `3.12` (same as `=3.12`, matches 3.12.x), `^1.2`,
//! `~1.2`, combined with commas: `>=3.10, <3.13`.

use std::cmp::Ordering;
use std::fmt;

/// How well a language is covered by tests and real-world use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// Run end to end with real apps (templates, CI).
    Tested,
    /// Same contract and build steps, fewer real-world runs.
    LessTested,
    /// Works if the program follows the app contract; no template, no detection.
    Contract,
}

impl Support {
    pub fn label(self) -> &'static str {
        match self {
            Support::Tested => "tested",
            Support::LessTested => "less tested",
            Support::Contract => "by contract",
        }
    }
}

/// How a bundled copy of a runtime is laid out and where it comes from.
#[derive(Debug, Clone, Copy)]
pub struct BundleDef {
    /// Version downloaded when the config does not name one.
    pub default_version: &'static str,
    /// Folders (relative to the runtime folder) with executables on Windows; added to `PATH`.
    pub bin_windows: &'static [&'static str],
    /// The same on macOS and Linux (missing folders are skipped).
    pub bin_unix: &'static [&'static str],
    /// Where `openshell runtime install` downloads it from.
    pub source: &'static str,
}

/// A language runtime (interpreter, VM or toolchain).
#[derive(Debug, Clone, Copy)]
pub struct RuntimeDef {
    /// Name used in `runtimes` and `requires` (`python`, `node`, …).
    pub name: &'static str,
    pub title: &'static str,
    /// Programs this runtime provides; the first one is asked for the version.
    pub programs: &'static [&'static str],
    /// Arguments that make the first program print its version.
    pub version_args: &'static [&'static str],
    /// Set when OpenShell can download and ship this runtime inside an app.
    pub bundle: Option<BundleDef>,
}

impl RuntimeDef {
    pub fn program(&self) -> &'static str {
        self.programs[0]
    }

    /// Folders with executables inside a bundled copy, for this operating system.
    pub fn bin_dirs(&self) -> &'static [&'static str] {
        match self.bundle {
            Some(b) if cfg!(windows) => b.bin_windows,
            Some(b) => b.bin_unix,
            None => &[],
        }
    }

    /// Parse the version out of the output of `program version_args`.
    pub fn parse_version(&self, output: &str) -> Option<Version> {
        let v = Version::find_in(output)?;
        // Java 8 and older report `1.8.0_281`
        if self.name == "java" && v.0.len() > 1 && v.0[0] == 1 {
            return Some(Version(v.0[1..].to_vec()));
        }
        Some(v)
    }
}

/// All runtimes OpenShell knows about.
pub const RUNTIMES: &[RuntimeDef] = &[
    RuntimeDef {
        name: "python",
        title: "Python",
        programs: &["python", "python3", "pip", "pip3"],
        version_args: &["--version"],
        bundle: Some(BundleDef {
            default_version: "3.12",
            bin_windows: &[".", "Scripts"],
            bin_unix: &["bin"],
            source: "python-build-standalone (github.com/astral-sh/python-build-standalone)",
        }),
    },
    RuntimeDef {
        name: "node",
        title: "Node.js",
        programs: &["node", "npm", "npx"],
        version_args: &["--version"],
        bundle: Some(BundleDef {
            default_version: "22",
            bin_windows: &["."],
            bin_unix: &["bin"],
            source: "nodejs.org",
        }),
    },
    RuntimeDef {
        name: "java",
        title: "Java",
        programs: &["java", "javac", "jar"],
        version_args: &["-version"],
        bundle: Some(BundleDef {
            default_version: "21",
            bin_windows: &["bin"],
            bin_unix: &["bin", "Contents/Home/bin"],
            source: "Eclipse Temurin JRE (api.adoptium.net)",
        }),
    },
    RuntimeDef {
        name: "dotnet",
        title: ".NET",
        programs: &["dotnet"],
        version_args: &["--version"],
        bundle: None,
    },
    RuntimeDef { name: "go", title: "Go", programs: &["go"], version_args: &["version"], bundle: None },
    RuntimeDef {
        name: "rust",
        title: "Rust",
        programs: &["rustc", "cargo"],
        version_args: &["--version"],
        bundle: None,
    },
];

/// The runtime with this name.
pub fn runtime(name: &str) -> Option<&'static RuntimeDef> {
    RUNTIMES.iter().find(|r| r.name == name)
}

/// The runtime that provides a program (`python3` → python, `npm` → node).
pub fn runtime_for_program(program: &str) -> Option<&'static RuntimeDef> {
    let base = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let base = base.strip_suffix(".exe").or_else(|| base.strip_suffix(".cmd")).unwrap_or(base);
    RUNTIMES.iter().find(|r| r.programs.contains(&base))
}

/// A backend language: which runtime it needs, how well it is tested and its templates.
#[derive(Debug, Clone, Copy)]
pub struct LanguageDef {
    pub id: &'static str,
    pub title: &'static str,
    /// Runtime needed on the target machine (`None` for compiled languages).
    pub runtime: Option<&'static str>,
    pub support: Support,
    /// Templates for `openshell create -t …` that use this language.
    pub templates: &'static [&'static str],
    pub note: &'static str,
}

/// All backend languages, in the order `openshell init` tries to detect them.
pub const LANGUAGES: &[LanguageDef] = &[
    LanguageDef {
        id: "java",
        title: "Java / Spring Boot",
        runtime: Some("java"),
        support: Support::Tested,
        templates: &["vanilla-java"],
        note: "a JRE must exist on the target machine or be shipped with `openshell runtime add java`",
    },
    LanguageDef {
        id: "cpp",
        title: "C++ (CMake)",
        runtime: None,
        support: Support::Tested,
        templates: &["vanilla-cpp"],
        note: "compiled: nothing to install on the target machine",
    },
    LanguageDef {
        id: "python",
        title: "Python",
        runtime: Some("python"),
        support: Support::Tested,
        templates: &["vanilla-python", "react-python", "vue-python"],
        note: "the interpreter must exist on the target machine or be shipped with `openshell runtime add python`",
    },
    LanguageDef {
        id: "go",
        title: "Go",
        runtime: None,
        support: Support::LessTested,
        templates: &["svelte-go"],
        note: "compiled with `go build`; `go run` needs Go on the machine",
    },
    LanguageDef {
        id: "rust",
        title: "Rust",
        runtime: None,
        support: Support::LessTested,
        templates: &[],
        note: "compiled with `cargo build`",
    },
    LanguageDef {
        id: "dotnet",
        title: ".NET",
        runtime: Some("dotnet"),
        support: Support::LessTested,
        templates: &[],
        note: "framework-dependent builds need the .NET runtime; self-contained publishes need nothing",
    },
    LanguageDef {
        id: "node",
        title: "Node.js",
        runtime: Some("node"),
        support: Support::Tested,
        templates: &["react-node"],
        note: "Node.js must exist on the target machine or be shipped with `openshell runtime add node`",
    },
];

/// The language with this id.
pub fn language(id: &str) -> Option<&'static LanguageDef> {
    LANGUAGES.iter().find(|l| l.id == id)
}

/// The language a template is written in (`react-python` → Python).
pub fn language_of_template(template: &str) -> Option<&'static LanguageDef> {
    LANGUAGES.iter().find(|l| l.templates.contains(&template))
}

// --------------------------------------------------------------------------- versions

/// A dotted version number (`3.12.1`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version(pub Vec<u64>);

impl Version {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches(['v', 'V']);
        let parts: Vec<u64> = s.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        (!parts.is_empty()).then_some(Self(parts))
    }

    /// The first `1.2.3`-like number in a text (`Python 3.12.1`, `go version go1.22.1 …`).
    pub fn find_in(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i].is_ascii_digit() {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                let candidate = text[start..i].trim_end_matches('.');
                if let Some(v) = Self::parse(candidate) {
                    return Some(v);
                }
            }
            i += 1;
        }
        None
    }

    fn get(&self, i: usize) -> u64 {
        self.0.get(i).copied().unwrap_or(0)
    }

    /// Compare as if both had the same number of parts (missing parts are 0).
    pub fn cmp_padded(&self, other: &Version) -> Ordering {
        let n = self.0.len().max(other.0.len());
        (0..n)
            .map(|i| self.get(i).cmp(&other.get(i)))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }

    /// Whether `self` starts with all parts of `prefix` (`3.12.4` starts with `3.12`).
    pub fn starts_with(&self, prefix: &Version) -> bool {
        prefix.0.iter().enumerate().all(|(i, p)| self.0.get(i) == Some(p))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&s.join("."))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    /// `=3.12` and `3.12`: same leading parts.
    Prefix,
    Caret,
    Tilde,
}

/// A version requirement such as `>=3.10, <3.13`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    parts: Vec<(Op, Version)>,
    text: String,
}

impl VersionReq {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parts = Vec::new();
        for raw in text.split(',') {
            let s = raw.trim();
            if s.is_empty() {
                continue;
            }
            let (op, rest) = [
                (">=", Op::Ge),
                ("<=", Op::Le),
                (">", Op::Gt),
                ("<", Op::Lt),
                ("=", Op::Prefix),
                ("^", Op::Caret),
                ("~", Op::Tilde),
            ]
            .iter()
            .find_map(|(p, op)| s.strip_prefix(p).map(|r| (*op, r)))
            .unwrap_or((Op::Prefix, s));
            let v = Version::parse(rest.trim()).ok_or_else(|| {
                format!("`{s}` is not a version requirement (examples: `>=3.10`, `22`, `^17`)")
            })?;
            parts.push((op, v));
        }
        if parts.is_empty() {
            return Err("empty version requirement".into());
        }
        Ok(Self { parts, text: text.trim().to_string() })
    }

    pub fn matches(&self, v: &Version) -> bool {
        self.parts.iter().all(|(op, r)| match op {
            Op::Ge => v.cmp_padded(r) != Ordering::Less,
            Op::Gt => v.cmp_padded(r) == Ordering::Greater,
            Op::Le => v.cmp_padded(r) != Ordering::Greater,
            Op::Lt => v.cmp_padded(r) == Ordering::Less,
            Op::Prefix => v.starts_with(r),
            Op::Caret => {
                // ^1.2 → >=1.2 <2 ; ^0.2 → >=0.2 <0.3
                let lead = r.0.iter().position(|p| *p != 0).unwrap_or(r.0.len().saturating_sub(1));
                v.cmp_padded(r) != Ordering::Less && v.starts_with(&Version(r.0[..=lead].to_vec()))
            }
            Op::Tilde => {
                // ~1.2 → >=1.2 <1.3 ; ~1 → >=1 <2
                let keep = if r.0.len() >= 2 { 2 } else { 1 };
                v.cmp_padded(r) != Ordering::Less && v.starts_with(&Version(r.0[..keep].to_vec()))
            }
        })
    }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn finds_versions_in_tool_output() {
        let py = runtime("python").unwrap();
        assert_eq!(py.parse_version("Python 3.12.1"), Some(v("3.12.1")));
        let java = runtime("java").unwrap();
        assert_eq!(java.parse_version("openjdk version \"21.0.2\" 2024-01-16"), Some(v("21.0.2")));
        assert_eq!(java.parse_version("java version \"1.8.0_281\""), Some(v("8.0")));
        let go = runtime("go").unwrap();
        assert_eq!(go.parse_version("go version go1.22.1 windows/amd64"), Some(v("1.22.1")));
        assert_eq!(runtime("node").unwrap().parse_version("v22.3.0\n"), Some(v("22.3.0")));
        assert_eq!(Version::find_in("no digits"), None);
    }

    #[test]
    fn requirements() {
        let req = |s: &str| VersionReq::parse(s).unwrap();
        assert!(req(">=3.10").matches(&v("3.12.1")));
        assert!(!req(">=3.10").matches(&v("3.9.18")));
        assert!(req(">=3.10, <3.13").matches(&v("3.12")));
        assert!(!req(">=3.10, <3.13").matches(&v("3.13.0")));
        assert!(req("3.12").matches(&v("3.12.4")));
        assert!(!req("3.12").matches(&v("3.13.0")));
        assert!(req("22").matches(&v("22.11.0")));
        assert!(req("^17").matches(&v("17.0.9")));
        assert!(!req("^17").matches(&v("21")));
        assert!(req("^0.2").matches(&v("0.2.5")) && !req("^0.2").matches(&v("0.3")));
        assert!(req("~1.2").matches(&v("1.2.9")) && !req("~1.2").matches(&v("1.3")));
        assert!(req("<4").matches(&v("3.99")) && !req("<4").matches(&v("4.0")));
        assert!(VersionReq::parse("latest").is_err());
        assert!(VersionReq::parse(" , ").is_err());
    }

    #[test]
    fn programs_map_to_runtimes() {
        assert_eq!(runtime_for_program("python3").unwrap().name, "python");
        assert_eq!(runtime_for_program("npm.cmd").unwrap().name, "node");
        assert_eq!(runtime_for_program("/usr/bin/java").unwrap().name, "java");
        assert!(runtime_for_program("cmake").is_none());
        assert_eq!(language_of_template("react-python").unwrap().id, "python");
        for l in LANGUAGES {
            if let Some(r) = l.runtime {
                assert!(runtime(r).is_some(), "{}", l.id);
            }
        }
    }
}
