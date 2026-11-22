//! Built-in databases (SQLite).
//!
//! A database is a building block of its own: an app can consist of only a frontend and a
//! database, only agent functions and a database, or use it from its backend. OpenShell
//! creates the file (inside the app's data directory), applies migrations, hands the path to
//! services (`OPENSHELL_DB_<NAME>`, `DATABASE_URL`) and exposes it to the frontend SDK and – as
//! far as `agent_access` allows – to agents.

use anyhow::{anyhow, Context, Result};
use openshell_config::{DatabaseAccess, DatabaseConfig, Project};
use openshell_runtime::LogStore;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::Connection;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Maximum rows returned by one query (more are reported as `truncated`).
pub const MAX_ROWS: usize = 1000;

pub struct Db {
    pub config: DatabaseConfig,
    pub path: PathBuf,
    conn: Mutex<Connection>,
}

#[derive(Default)]
pub struct Databases {
    dbs: Vec<Db>,
}

fn log_name(db: &DatabaseConfig) -> String {
    format!("db:{}", db.name)
}

impl Databases {
    /// Open (and create) all configured databases and apply pending migrations.
    pub fn open(project: &Project, logs: &LogStore) -> Result<Self> {
        let mut dbs = Vec::new();
        for cfg in &project.config.databases {
            let file = PathBuf::from(cfg.file_name());
            let path = if file.is_absolute() { file } else { project.data_dir().join(file) };
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let conn = Connection::open(&path)
                .with_context(|| format!("cannot open database `{}` at {}", cfg.name, path.display()))?;
            // concurrent access by services, functions and the shell
            conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
            conn.busy_timeout(std::time::Duration::from_secs(5))?;
            conn.pragma_update(None, "foreign_keys", "ON")?;
            let db = Db { config: cfg.clone(), path, conn: Mutex::new(conn) };
            if let Some(dir) = &cfg.migrations {
                let dir = project.resolve_path(dir);
                let applied = db
                    .migrate(&dir)
                    .with_context(|| format!("migration of database `{}` failed", cfg.name))?;
                for m in &applied {
                    logs.system(&log_name(cfg), format!("applied migration {m}"));
                }
            }
            logs.system(&log_name(cfg), format!("ready: {}", db.path.display()));
            dbs.push(db);
        }
        Ok(Self { dbs })
    }

    pub fn is_empty(&self) -> bool {
        self.dbs.is_empty()
    }

    pub fn all(&self) -> &[Db] {
        &self.dbs
    }

    /// A database by name; without a name the only (or `main`, or first) database.
    pub fn get(&self, name: Option<&str>) -> Result<&Db, String> {
        match name.filter(|n| !n.is_empty()) {
            Some(n) => self.dbs.iter().find(|d| d.config.name == n).ok_or_else(|| {
                format!(
                    "unknown database `{n}` (available: {})",
                    self.dbs.iter().map(|d| d.config.name.as_str()).collect::<Vec<_>>().join(", ")
                )
            }),
            None => {
                self.dbs.iter().find(|d| d.config.name == "main").or_else(|| self.dbs.first()).ok_or_else(
                    || "this app has no database (add one with `openshell add database`)".to_string(),
                )
            }
        }
    }

    /// Environment for services and functions.
    pub fn env(&self) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> =
            self.dbs.iter().map(|d| (d.config.env_var(), d.path.display().to_string())).collect();
        if let Ok(main) = self.get(None) {
            let url = format!("sqlite:///{}", main.path.display().to_string().replace('\\', "/"));
            env.push(("DATABASE_URL".into(), url));
        }
        env
    }

    /// Overview of all databases with their tables and row counts.
    pub fn list(&self) -> Value {
        Value::Array(self.dbs.iter().map(|d| d.summary()).collect())
    }
}

fn to_sql(v: &Value) -> SqlValue {
    match v {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

fn to_json(v: ValueRef<'_>) -> Value {
    match v {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => json!(i),
        ValueRef::Real(f) => json!(f),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => json!(format!("<blob, {} bytes>", b.len())),
    }
}

/// Bind positional (`[..]`) or named (`{"name": ..}`) parameters.
fn bind(stmt: &mut rusqlite::Statement<'_>, params: &Value) -> Result<(), String> {
    match params {
        Value::Null => Ok(()),
        Value::Array(a) => {
            if a.len() != stmt.parameter_count() {
                return Err(format!(
                    "the statement has {} parameter(s), {} given",
                    stmt.parameter_count(),
                    a.len()
                ));
            }
            for (i, v) in a.iter().enumerate() {
                stmt.raw_bind_parameter(i + 1, to_sql(v)).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Value::Object(o) => {
            for (k, v) in o {
                let key = if k.starts_with([':', '@', '$']) { k.clone() } else { format!(":{k}") };
                let idx = stmt
                    .parameter_index(&key)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("the statement has no parameter `{key}`"))?;
                stmt.raw_bind_parameter(idx, to_sql(v)).map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        _ => Err("`params` must be an array or an object".into()),
    }
}

impl Db {
    fn migrate(&self, dir: &Path) -> Result<Vec<String>> {
        if !dir.is_dir() {
            return Err(anyhow!("migrations directory {} does not exist", dir.display()));
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e.eq_ignore_ascii_case("sql")).unwrap_or(false))
            .collect();
        files.sort();
        let mut conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS _openshell_migrations (name TEXT PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        )?;
        let mut applied = Vec::new();
        for file in files {
            let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
            let done: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM _openshell_migrations WHERE name = ?1)",
                [&name],
                |r| r.get(0),
            )?;
            if done {
                continue;
            }
            let sql = std::fs::read_to_string(&file)?;
            let tx = conn.transaction()?;
            tx.execute_batch(&sql).with_context(|| format!("in {name}"))?;
            tx.execute("INSERT INTO _openshell_migrations (name) VALUES (?1)", [&name])?;
            tx.commit()?;
            applied.push(name);
        }
        Ok(applied)
    }

    fn tables(&self, conn: &Connection) -> Vec<String> {
        let mut stmt = match conn.prepare(
            "SELECT name FROM sqlite_master WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' \
             AND name <> '_openshell_migrations' ORDER BY name",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map([], |r| r.get::<_, String>(0)).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }

    pub fn summary(&self) -> Value {
        let conn = self.conn.lock().unwrap();
        let tables: Vec<Value> = self
            .tables(&conn)
            .into_iter()
            .map(|t| {
                let rows: i64 = conn
                    .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", t.replace('"', "\"\"")), [], |r| {
                        r.get(0)
                    })
                    .unwrap_or(-1);
                json!({ "name": t, "rows": rows })
            })
            .collect();
        json!({
            "name": self.config.name,
            "type": "sqlite",
            "description": self.config.description,
            "path": self.path,
            "size_bytes": std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0),
            "agent_access": self.config.agent_access,
            "tables": tables,
        })
    }

    /// Tables with columns and their `CREATE` statements.
    pub fn schema(&self, table: Option<&str>) -> Result<Value, String> {
        let conn = self.conn.lock().unwrap();
        let tables = self.tables(&conn);
        let selected: Vec<String> = match table {
            Some(t) if !tables.iter().any(|x| x == t) => return Err(format!("unknown table `{t}`")),
            Some(t) => vec![t.to_string()],
            None => tables,
        };
        let mut out = Vec::new();
        for t in selected {
            let sql: Option<String> =
                conn.query_row("SELECT sql FROM sqlite_master WHERE name = ?1", [&t], |r| r.get(0)).ok();
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info(\"{}\")", t.replace('"', "\"\"")))
                .map_err(|e| e.to_string())?;
            let columns: Vec<Value> = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "name": r.get::<_, String>(1)?,
                        "type": r.get::<_, String>(2)?,
                        "not_null": r.get::<_, i64>(3)? == 1,
                        "default": r.get::<_, Option<String>>(4)?,
                        "primary_key": r.get::<_, i64>(5)? > 0,
                    }))
                })
                .map_err(|e| e.to_string())?
                .flatten()
                .collect();
            out.push(json!({ "table": t, "columns": columns, "sql": sql }));
        }
        Ok(json!({ "database": self.config.name, "tables": out }))
    }

    /// Run a read-only statement (SELECT, read-only PRAGMA, EXPLAIN …).
    pub fn query(&self, sql: &str, params: &Value) -> Result<Value, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        if !stmt.readonly() {
            return Err(
                "db_query only runs read-only statements (SELECT …); use db_execute to change data".into()
            );
        }
        let columns: Vec<String> = stmt.column_names().map(|c| c.to_string()).collect();
        let n = columns.len();
        let mut truncated = false;
        bind(&mut stmt, params)?;
        let mut rows_out: Vec<Value> = Vec::new();
        let mut rows = stmt.raw_query();
        while let Some(row) = rows.next().map_err(|e| e.to_string())? {
            if rows_out.len() >= MAX_ROWS {
                truncated = true;
                break;
            }
            let mut obj = Map::new();
            for (i, name) in columns.iter().enumerate().take(n) {
                obj.insert(name.clone(), to_json(row.get_ref(i).map_err(|e| e.to_string())?));
            }
            rows_out.push(Value::Object(obj));
        }
        let rows = rows_out;
        Ok(json!({ "columns": columns, "rows": rows, "row_count": rows.len(), "truncated": truncated }))
    }

    /// Run a statement that changes data or schema; `script: true` runs several statements.
    pub fn execute(&self, sql: &str, params: &Value, script: bool) -> Result<Value, String> {
        let conn = self.conn.lock().unwrap();
        if script {
            conn.execute_batch(sql).map_err(|e| e.to_string())?;
            return Ok(json!({ "ok": true, "changes": conn.changes() }));
        }
        let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
        bind(&mut stmt, params)?;
        let changes = stmt.raw_execute().map_err(|e| e.to_string())?;
        Ok(json!({ "ok": true, "changes": changes, "last_insert_rowid": conn.last_insert_rowid() }))
    }

    /// May agents (not the app's own frontend) run `op` (`read` / `write`) on this database?
    pub fn agent_allowed(&self, write: bool) -> Result<(), String> {
        match (self.config.agent_access, write) {
            (DatabaseAccess::None, _) => Err(format!(
                "database `{}` is not accessible to agents (agent_access: none)",
                self.config.name
            )),
            (DatabaseAccess::Read, true) => Err(format!(
                "database `{}` is read-only for agents (set \"agent_access\": \"write\" in openshell.json)",
                self.config.name
            )),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openshell_config::parse_json;

    #[test]
    fn migrations_queries_and_access() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("db")).unwrap();
        std::fs::write(
            dir.path().join("db/001_init.sql"),
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL, done INTEGER DEFAULT 0);",
        )
        .unwrap();
        std::fs::write(dir.path().join("db/002_seed.sql"), "INSERT INTO notes (title) VALUES ('first');")
            .unwrap();
        let config = parse_json(
            r#"{"app":{"name":"x"},"databases":[{"name":"main","migrations":"db"},{"name":"logs","agent_access":"none"}]}"#,
        )
        .unwrap();
        let project = Project {
            root: dir.path().to_path_buf(),
            config_path: dir.path().join("openshell.json"),
            config,
            state_dir_override: None,
        };
        let logs = LogStore::new(100, None);
        let dbs = Databases::open(&project, &logs).unwrap();
        assert!(dir.path().join(".openshell/data/main.db").exists());
        let main = dbs.get(None).unwrap();
        assert_eq!(main.config.name, "main");

        let r = main.query("SELECT title FROM notes", &Value::Null).unwrap();
        assert_eq!(r["rows"][0]["title"], "first");
        assert!(main.query("DELETE FROM notes", &Value::Null).is_err());

        let w = main.execute("INSERT INTO notes (title) VALUES (?1)", &json!(["second"]), false).unwrap();
        assert_eq!(w["changes"], 1);
        main.execute("INSERT INTO notes (title) VALUES (:t)", &json!({"t": "third"}), false).unwrap();
        let r = main.query("SELECT COUNT(*) AS n FROM notes WHERE title LIKE ?1", &json!(["%d"])).unwrap();
        assert_eq!(r["rows"][0]["n"], 2);

        let schema = main.schema(Some("notes")).unwrap();
        assert_eq!(schema["tables"][0]["columns"][1]["name"], "title");
        assert_eq!(dbs.list()[0]["tables"][0]["rows"], 3);

        // migrations are applied once
        drop(dbs);
        let dbs = Databases::open(&project, &logs).unwrap();
        assert_eq!(
            dbs.get(None).unwrap().query("SELECT COUNT(*) AS n FROM notes", &Value::Null).unwrap()["rows"][0]
                ["n"],
            3
        );

        assert!(main_access(&dbs, "main", true).is_err());
        assert!(main_access(&dbs, "main", false).is_ok());
        assert!(main_access(&dbs, "logs", false).is_err());
        let env = dbs.env();
        assert!(env.iter().any(|(k, _)| k == "OPENSHELL_DB_LOGS"));
        assert!(env
            .iter()
            .any(|(k, v)| k == "DATABASE_URL" && v.starts_with("sqlite:///") && v.ends_with("main.db")));
    }

    fn main_access(dbs: &Databases, name: &str, write: bool) -> Result<(), String> {
        dbs.get(Some(name)).unwrap().agent_allowed(write)
    }
}
