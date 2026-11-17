//! # openshell-config
//!
//! The versioned configuration format of OpenShell.
//!
//! An application is described by a single `openshell.json` (or `openshell.toml`)
//! file in its project root. This crate defines the schema, loads the file,
//! reports precise parse errors and runs semantic validation (unknown
//! dependencies, cycles, invalid names, ...).
//!
//! The schema is intentionally technology-agnostic: services are arbitrary
//! executables, the frontend is "something that serves a URL".

mod interpolate;
mod model;
mod validate;

pub use interpolate::{interpolate, variables, InterpolationError};
pub use model::*;
pub use validate::{startup_order, validate, Issue, FRONTEND_SERVICE};

use std::fmt;
use std::path::{Path, PathBuf};

/// Current (and highest supported) configuration schema version.
pub const CONFIG_VERSION: u32 = 1;

/// File names that are recognised as project configuration, in priority order.
pub const CONFIG_FILE_NAMES: [&str; 2] = ["openshell.json", "openshell.toml"];

/// Errors that can occur while loading a configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no openshell.json or openshell.toml found in {0} or any parent directory")]
    NotFound(PathBuf),
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("{path} is invalid:\n{}", format_issues(.issues))]
    Invalid { path: PathBuf, issues: Vec<Issue> },
}

fn format_issues(issues: &[Issue]) -> String {
    issues
        .iter()
        .map(|i| format!("  - {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A loaded and validated project: the configuration plus where it lives.
#[derive(Debug, Clone)]
pub struct Project {
    /// Directory containing the configuration file. All relative paths in the
    /// configuration are resolved against this directory.
    pub root: PathBuf,
    /// Absolute path of the configuration file.
    pub config_path: PathBuf,
    pub config: Config,
    /// Where runtime state (`runtime.json`, logs, audit log, app data) is kept.
    /// `None` = `<root>/.openshell`. Installed apps use a per-user directory.
    pub state_dir_override: Option<PathBuf>,
}

impl Project {
    /// Load the configuration from `dir`, walking up parent directories until a
    /// configuration file is found.
    pub fn discover(dir: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let start = absolute(dir.as_ref());
        let mut current = Some(start.as_path());
        while let Some(dir) = current {
            if let Some(path) = find_config_file(dir) {
                return Self::load(path);
            }
            current = dir.parent();
        }
        Err(ConfigError::NotFound(start))
    }

    /// Load a specific configuration file (or a directory containing one).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = absolute(path.as_ref());
        let path = if path.is_dir() {
            find_config_file(&path).ok_or_else(|| ConfigError::NotFound(path.clone()))?
        } else {
            path
        };
        let config = load_file(&path)?;
        let root = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Project {
            root,
            config_path: path,
            config,
            state_dir_override: None,
        })
    }

    /// Resolve a (possibly relative) path from the configuration against the project root.
    pub fn resolve_path(&self, p: &str) -> PathBuf {
        let path = Path::new(p);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    /// Directory for runtime state (`.openshell/` in the project root).
    pub fn state_dir(&self) -> PathBuf {
        self.state_dir_override
            .clone()
            .unwrap_or_else(|| self.root.join(".openshell"))
    }

    /// Directory for application data (`OPENSHELL_DATA_DIR`), inside the state directory.
    pub fn data_dir(&self) -> PathBuf {
        self.state_dir().join("data")
    }

    /// Keep state in a different directory (e.g. a per-user directory for installed apps).
    pub fn with_state_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.state_dir_override = Some(dir.into());
        self
    }
}

/// Returns the first configuration file found directly in `dir`.
pub fn find_config_file(dir: &Path) -> Option<PathBuf> {
    CONFIG_FILE_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|p| p.is_file())
}

/// Read, parse and validate a configuration file.
pub fn load_file(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let is_toml = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("toml"))
        .unwrap_or(false);
    let config = if is_toml {
        parse_toml(&text)
    } else {
        parse_json(&text)
    }
    .map_err(|message| ConfigError::Parse {
        path: path.to_path_buf(),
        message,
    })?;
    let issues = validate(&config);
    if issues.iter().any(|i| i.is_error()) {
        return Err(ConfigError::Invalid {
            path: path.to_path_buf(),
            issues,
        });
    }
    Ok(config)
}

/// Parse a JSON configuration (without semantic validation).
pub fn parse_json(text: &str) -> Result<Config, String> {
    let de = &mut serde_json::Deserializer::from_str(text);
    serde_path_to_error::deserialize(de).map_err(|e| {
        let path = e.path().to_string();
        let inner = e.into_inner();
        if path.is_empty() || path == "." {
            inner.to_string()
        } else {
            format!("at `{path}`: {inner}")
        }
    })
}

/// Parse a TOML configuration (without semantic validation).
pub fn parse_toml(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}

/// JSON Schema of the configuration format (for editor completion and validation).
pub fn json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(Config);
    serde_json::to_value(schema).unwrap_or_default()
}

fn absolute(p: &Path) -> PathBuf {
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|d| d.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    // drop `.` components so paths print nicely
    joined
        .components()
        .filter(|c| !matches!(c, std::path::Component::CurDir))
        .collect()
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Mode::Development => "development",
            Mode::Production => "production",
        })
    }
}

impl std::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "dev" | "development" => Ok(Mode::Development),
            "prod" | "production" => Ok(Mode::Production),
            other => Err(format!(
                "unknown mode `{other}` (expected `development` or `production`)"
            )),
        }
    }
}

#[cfg(test)]
mod tests;

/// Re-export of the TOML implementation (used by tooling that writes `openshell.toml`).
pub mod toml_support {
    pub use toml::{to_string, Value};
}
