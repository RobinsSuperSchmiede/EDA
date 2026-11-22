//! `openshell add <block>` – compose an app from building blocks.
//!
//! Every block is optional and independent: an app can be only a frontend, only a database
//! with functions for AI agents, only a backend, or any combination. Blocks know about each
//! other (a frontend added to an app with a backend proxies `/api`, a backend gets the database
//! path, …) regardless of the order in which they are added.

use super::create::TEMPLATES;
use crate::output::{self, bold, dim};
use anyhow::{bail, Context, Result};
use include_dir::{Dir, DirEntry};
use openshell_core::config::{find_config_file, parse_json, validate};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Language of a backend or function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Lang {
    Python,
    Node,
}

/// A building block.
#[derive(Debug, Clone)]
pub enum Block {
    Frontend,
    Backend(Lang),
    Database(String),
    Function(String, Lang),
}

impl Block {
    pub fn label(&self) -> String {
        match self {
            Block::Frontend => "frontend".into(),
            Block::Backend(l) => format!("backend ({})", lang_name(*l)),
            Block::Database(n) => format!("database `{n}`"),
            Block::Function(n, l) => format!("function `{n}` ({})", lang_name(*l)),
        }
    }
}

fn lang_name(l: Lang) -> &'static str {
    match l {
        Lang::Python => "Python",
        Lang::Node => "Node.js",
    }
}

/// The project's `openshell.json`, edited as JSON (key order is preserved).
pub struct ConfigFile {
    pub root: PathBuf,
    path: PathBuf,
    pub value: Value,
}

impl ConfigFile {
    pub fn open(dir: &Path) -> Result<Self> {
        let path = match find_config_file(dir) {
            Some(p) => p,
            None => openshell_core::config::Project::discover(dir)?.config_path,
        };
        if path.extension().map(|e| e.eq_ignore_ascii_case("toml")).unwrap_or(false) {
            bail!(
                "`openshell add` edits openshell.json; this project uses {} – edit it by hand",
                path.display()
            );
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let value: Value =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(Self { root: path.parent().unwrap_or(Path::new(".")).to_path_buf(), path, value })
    }

    fn obj(&mut self) -> &mut Map<String, Value> {
        if !self.value.is_object() {
            self.value = json!({});
        }
        self.value.as_object_mut().unwrap()
    }

    fn array(&mut self, key: &str) -> &mut Vec<Value> {
        let o = self.obj();
        if !o.get(key).map(|v| v.is_array()).unwrap_or(false) {
            o.insert(key.into(), json!([]));
        }
        o.get_mut(key).unwrap().as_array_mut().unwrap()
    }

    fn has_service(&self, name: &str) -> bool {
        self.value["services"].as_array().map(|a| a.iter().any(|s| s["name"] == name)).unwrap_or(false)
    }

    fn has_database(&self, name: &str) -> bool {
        self.value["databases"].as_array().map(|a| a.iter().any(|s| s["name"] == name)).unwrap_or(false)
    }

    fn has_frontend(&self) -> bool {
        self.value.get("frontend").map(|f| f.is_object()).unwrap_or(false)
    }

    fn app_name(&self) -> String {
        self.value["app"]["name"].as_str().unwrap_or("App").to_string()
    }

    /// Validate and write. Nothing is written if the result would be invalid.
    pub fn save(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(&self.value)? + "\n";
        let parsed = parse_json(&text).map_err(anyhow::Error::msg)?;
        let issues = validate(&parsed);
        let errors: Vec<String> = issues.iter().filter(|i| i.is_error()).map(|i| i.to_string()).collect();
        if !errors.is_empty() {
            bail!("the change would make openshell.json invalid:\n  {}", errors.join("\n  "));
        }
        std::fs::write(&self.path, text)?;
        Ok(())
    }
}

fn render(text: &str, app_name: &str, function: &str) -> String {
    text.replace("{{app_name}}", app_name).replace("{{function_name}}", function)
}

/// Copy a block directory into the project. Existing files are kept (reported as skipped).
fn copy_block(
    block: &str,
    root: &Path,
    app_name: &str,
    function: &str,
    created: &mut Vec<String>,
) -> Result<()> {
    let dir =
        TEMPLATES.get_dir(format!("_blocks/{block}")).with_context(|| format!("missing block {block}"))?;
    fn walk(
        d: &Dir<'_>,
        base: &Path,
        root: &Path,
        app: &str,
        function: &str,
        created: &mut Vec<String>,
    ) -> Result<()> {
        for e in d.entries() {
            match e {
                DirEntry::Dir(sub) => walk(sub, base, root, app, function, created)?,
                DirEntry::File(f) => {
                    let rel = f.path().strip_prefix(base).unwrap_or(f.path());
                    let rel = PathBuf::from(rel.to_string_lossy().replace("__name__", function));
                    let out = root.join(&rel);
                    let shown = rel.to_string_lossy().replace('\\', "/");
                    if out.exists() {
                        created.push(format!("{shown} (exists, kept)"));
                        continue;
                    }
                    if let Some(p) = out.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    match f.contents_utf8() {
                        Some(t) => std::fs::write(&out, render(t, app, function))?,
                        None => std::fs::write(&out, f.contents())?,
                    }
                    created.push(shown);
                }
            }
        }
        Ok(())
    }
    walk(dir, dir.path(), root, app_name, function, created)
}

fn valid_ident(name: &str) -> bool {
    !name.is_empty()
        && name.chars().next().map(|c| c.is_ascii_lowercase()).unwrap_or(false)
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Make sure the SDK may write when the frontend works with a database directly.
fn grant_sdk_actions(cfg: &mut ConfigFile) {
    let agents = cfg.obj().entry("agents").or_insert_with(|| json!({}));
    if !agents.is_object() {
        *agents = json!({});
    }
    let sdk =
        agents.as_object_mut().unwrap().entry("sdk_permissions").or_insert_with(|| json!(["read", "ui"]));
    if let Some(list) = sdk.as_array_mut() {
        if !list.iter().any(|p| p == "actions") {
            list.push(json!("actions"));
        }
    }
}

/// Connect a static frontend to the backend (`/api` proxy, start order).
fn link_frontend_backend(cfg: &mut ConfigFile) {
    if !(cfg.has_frontend() && cfg.has_service("backend")) {
        return;
    }
    let fe = cfg.obj().get_mut("frontend").unwrap().as_object_mut().unwrap();
    if fe.contains_key("dist") && !fe.contains_key("command") {
        let proxy = fe.entry("proxy").or_insert_with(|| json!({}));
        if let Some(p) = proxy.as_object_mut() {
            p.entry("/api").or_insert_with(|| json!("${services.backend.url}"));
        }
    }
    let deps = fe.entry("depends_on").or_insert_with(|| json!([]));
    if let Some(d) = deps.as_array_mut() {
        if !d.iter().any(|x| x == "backend") {
            d.push(json!("backend"));
        }
    }
}

/// Apply one block to the project. Returns the files written.
pub fn apply(cfg: &mut ConfigFile, block: &Block) -> Result<Vec<String>> {
    let root = cfg.root.clone();
    let app = cfg.app_name();
    let mut files = Vec::new();
    match block {
        Block::Frontend => {
            if cfg.has_frontend() {
                bail!("the app already has a frontend (see `frontend` in openshell.json)");
            }
            copy_block("frontend", &root, &app, "", &mut files)?;
            cfg.obj().insert("frontend".into(), json!({ "dist": "web", "watch": ["web"] }));
            link_frontend_backend(cfg);
            if cfg.value["databases"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                grant_sdk_actions(cfg);
            }
        }
        Block::Backend(lang) => {
            if cfg.has_service("backend") {
                bail!("the app already has a service named `backend`");
            }
            let (block, command) = match lang {
                Lang::Python => ("backend-python", json!(["python", "main.py"])),
                Lang::Node => ("backend-node", json!(["node", "server.js"])),
            };
            copy_block(block, &root, &app, "", &mut files)?;
            cfg.array("services").push(json!({
                "name": "backend",
                "kind": "backend",
                "command": command,
                "cwd": "backend",
                "port": "auto",
                "healthcheck": "http://127.0.0.1:${port}/health",
                "restart": true,
                "watch": ["backend"]
            }));
            let caps = cfg.array("capabilities");
            if !caps.iter().any(|c| c["name"] == "backend_hello") {
                caps.push(json!({
                    "name": "backend_hello",
                    "description": "Ask the backend for a greeting and the current time",
                    "permission": "read",
                    "http": { "service": "backend", "method": "GET", "path": "/api/hello" }
                }));
            }
            link_frontend_backend(cfg);
        }
        Block::Database(name) => {
            if !valid_ident(name) {
                bail!("`{name}` is not a valid database name (lowercase letters, digits, `_`)");
            }
            if cfg.has_database(name) {
                bail!("the app already has a database `{name}`");
            }
            let migrations =
                if name == "main" { "db/migrations".to_string() } else { format!("db/{name}/migrations") };
            let mut written = Vec::new();
            copy_block("database", &root, &app, "", &mut written)?;
            if name != "main" {
                // move the example migration into the database's own folder
                let from = root.join("db/migrations/001_init.sql");
                let to = root.join(&migrations).join("001_init.sql");
                if !to.exists() && written.iter().any(|f| f == "db/migrations/001_init.sql") {
                    std::fs::create_dir_all(to.parent().unwrap())?;
                    std::fs::rename(&from, &to)?;
                    let _ = std::fs::remove_dir(root.join("db/migrations"));
                    written = vec![format!("{migrations}/001_init.sql")];
                }
            }
            files.extend(written);
            cfg.array("databases").push(json!({
                "name": name,
                "migrations": migrations,
                "agent_access": "write"
            }));
            if cfg.has_frontend() {
                grant_sdk_actions(cfg);
            }
        }
        Block::Function(name, lang) => {
            if !valid_ident(name) {
                bail!("`{name}` is not a valid function name (lowercase letters, digits, `_`)");
            }
            if cfg.value["capabilities"]
                .as_array()
                .map(|a| a.iter().any(|c| c["name"] == name.as_str()))
                .unwrap_or(false)
            {
                bail!("the app already has a capability `{name}`");
            }
            let (block, run) = match lang {
                Lang::Python => ("function-python", json!(["python", format!("tools/{name}.py")])),
                Lang::Node => ("function-node", json!(["node", format!("tools/{name}.mjs")])),
            };
            copy_block(block, &root, &app, name, &mut files)?;
            cfg.array("capabilities").push(json!({
                "name": name,
                "description": format!("Greets someone by name (example function `{name}` – describe what it does)"),
                "permission": "read",
                "input_schema": { "type": "object", "properties": { "name": { "type": "string", "description": "Who to greet" } } },
                "run": run
            }));
        }
    }
    Ok(files)
}

/// `openshell add …`
pub fn run(dir: &Path, block: Block) -> Result<()> {
    let mut cfg = ConfigFile::open(dir)?;
    let files = apply(&mut cfg, &block)?;
    cfg.save()?;
    output::ok(&format!("added {}", bold(&block.label())));
    for f in &files {
        println!("  {} {f}", dim("+"));
    }
    println!("  {} openshell.json", dim("~"));
    let hint = match &block {
        Block::Frontend => "edit web/ – `openshell dev` shows it in the window and reloads on save".to_string(),
        Block::Backend(_) => "the backend restarts on every change in backend/ – try `openshell call cap_backend_hello`".into(),
        Block::Database(n) => format!(
            "schema changes go into new files in the migrations folder; agents: db_query / db_execute, services: $OPENSHELL_DB_{}",
            n.to_uppercase()
        ),
        Block::Function(n, _) => format!("describe it in openshell.json, then: openshell call cap_{n} '{{\"name\": \"Ada\"}}'"),
    };
    println!("\n  {}", dim(&hint));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(dir: &Path) -> ConfigFile {
        std::fs::write(dir.join("openshell.json"), r#"{"version":1,"app":{"name":"Blocks"}}"#).unwrap();
        ConfigFile::open(dir).unwrap()
    }

    #[test]
    fn blocks_combine_in_any_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = project(dir.path());
        apply(&mut cfg, &Block::Frontend).unwrap();
        apply(&mut cfg, &Block::Database("main".into())).unwrap();
        apply(&mut cfg, &Block::Function("greet".into(), Lang::Python)).unwrap();
        apply(&mut cfg, &Block::Backend(Lang::Node)).unwrap();
        cfg.save().unwrap();
        let v = &cfg.value;
        assert_eq!(v["frontend"]["proxy"]["/api"], "${services.backend.url}");
        assert_eq!(v["frontend"]["depends_on"], json!(["backend"]));
        assert!(v["agents"]["sdk_permissions"].as_array().unwrap().contains(&json!("actions")));
        assert_eq!(v["capabilities"][0]["run"], json!(["python", "tools/greet.py"]));
        for f in ["web/index.html", "db/migrations/001_init.sql", "tools/greet.py", "backend/server.js"] {
            assert!(dir.path().join(f).exists(), "{f}");
        }
        assert!(std::fs::read_to_string(dir.path().join("tools/greet.py"))
            .unwrap()
            .contains("Function `greet` of Blocks"));
        // keys keep their order
        let text = std::fs::read_to_string(dir.path().join("openshell.json")).unwrap();
        assert!(text.find("\"version\"").unwrap() < text.find("\"app\"").unwrap());
        assert!(apply(&mut cfg, &Block::Frontend).is_err());
        assert!(apply(&mut cfg, &Block::Function("Bad-Name".into(), Lang::Node)).is_err());
    }

    #[test]
    fn single_blocks_are_valid_apps() {
        for block in [
            Block::Frontend,
            Block::Backend(Lang::Python),
            Block::Database("main".into()),
            Block::Database("archive".into()),
            Block::Function("hello".into(), Lang::Node),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut cfg = project(dir.path());
            apply(&mut cfg, &block).unwrap();
            cfg.save().unwrap_or_else(|e| panic!("{block:?}: {e}"));
        }
    }
}
