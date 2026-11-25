//! `openshell init` – generate a configuration for an existing project.

use crate::output::{self, bold, dim};
use anyhow::{bail, Result};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/RobinsSuperSchmiede/EDA/main/schema/openshell.schema.json";

struct Frontend {
    dir: String,
    command: String,
    url: String,
    setup: Option<(String, String)>,
    build: Option<String>,
    dist: Option<String>,
    framework: String,
}

use super::rel_path as rel;

use crate::languages::{package_manager, read_json};

fn has_dep(pkg: &Value, name: &str) -> bool {
    ["dependencies", "devDependencies"].iter().any(|k| pkg[k].get(name).is_some())
}

fn detect_frontend(root: &Path) -> Option<Frontend> {
    for d in ["frontend", "web", "client", "ui", "app", "."] {
        let dir = root.join(d);
        let pkg_path = dir.join("package.json");
        if let Some(pkg) = read_json(&pkg_path) {
            let is_frontend = [
                "vite",
                "react-scripts",
                "next",
                "@angular/core",
                "@vue/cli-service",
                "svelte",
                "astro",
                "parcel",
            ]
            .iter()
            .any(|d| has_dep(&pkg, d));
            if !is_frontend {
                continue;
            }
            let pm = package_manager(&dir);
            let run = |script: &str| {
                if pm == "npm" {
                    format!("npm run {script}")
                } else {
                    format!("{pm} {script}")
                }
            };
            let scripts = &pkg["scripts"];
            let (framework, port, dist) = if has_dep(&pkg, "next") {
                ("Next.js", 3000, "out")
            } else if has_dep(&pkg, "react-scripts") {
                ("Create React App", 3000, "build")
            } else if has_dep(&pkg, "@angular/core") {
                ("Angular", 4200, "dist")
            } else if has_dep(&pkg, "@vue/cli-service") {
                ("Vue CLI", 8080, "dist")
            } else if has_dep(&pkg, "astro") {
                ("Astro", 4321, "dist")
            } else {
                ("Vite", 5173, "dist")
            };
            let command = if scripts.get("dev").is_some() {
                run("dev")
            } else if scripts.get("start").is_some() {
                run("start")
            } else {
                continue;
            };
            let rel_dir = rel(root, &dir);
            let prefix = if rel_dir == "." { String::new() } else { format!("{rel_dir}/") };
            return Some(Frontend {
                dir: rel_dir.clone(),
                command,
                url: format!("http://localhost:{port}"),
                setup: Some((format!("{pm} install"), format!("{prefix}node_modules"))),
                build: scripts.get("build").map(|_| run("build")),
                dist: Some(format!("{prefix}{dist}")),
                framework: framework.into(),
            });
        }
        if d != "." && dir.join("index.html").is_file() {
            return Some(Frontend {
                dir: rel(root, &dir),
                command: String::new(),
                url: String::new(),
                setup: None,
                build: None,
                dist: Some(rel(root, &dir)),
                framework: "static HTML".into(),
            });
        }
    }
    None
}

fn title_case(s: &str) -> String {
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn generate(root: &Path, name: Option<String>) -> (Value, Vec<String>) {
    let mut notes = Vec::new();
    let dir_name = root
        .canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "My App".into());
    let name = name.unwrap_or_else(|| title_case(&dir_name));
    let mut cfg = Map::new();
    cfg.insert("$schema".into(), json!(SCHEMA_URL));
    cfg.insert("version".into(), json!(1));
    cfg.insert("app".into(), json!({ "name": name, "version": "0.1.0" }));
    cfg.insert("window".into(), json!({ "width": 1280, "height": 800 }));

    let fe = detect_frontend(root);
    let be = crate::languages::detect_backend(root, fe.as_ref().map(|f| f.dir.as_str()));
    let mut services = vec![];
    if let Some(b) = &be {
        notes.push(crate::languages::describe(b));
        let mut svc = json!({
            "name": "backend",
            "kind": "backend",
            "command": b.command,
            "port": b.port,
            "restart": true,
        });
        if let Some(build) = &b.build {
            svc["build"] = build.clone();
        }
        if let Some(h) = &b.healthcheck {
            svc["healthcheck"] = json!(h);
        }
        if let Some((rt, req)) = crate::languages::suggested_requires(b, &root.join(&b.dir)) {
            svc["requires"] = json!({ rt: req });
        }
        if let Some(o) = &b.openapi {
            svc["openapi"] = json!(o);
            notes.push(format!("agents get the backend API as tools via its OpenAPI document ({o})"));
        }
        let prefix = if b.dir == "." { String::new() } else { format!("{}/", b.dir) };
        if b.dir != "." {
            svc["cwd"] = json!(b.dir);
        }
        match &b.watch {
            Some(w) => svc["watch"] = json!([format!("{prefix}{w}")]),
            None if b.dir != "." => svc["watch"] = json!([b.dir]),
            None => {}
        }
        services.push(svc);
        if let Some(n) = &b.note {
            notes.push(n.clone());
        }
        notes.push("check the backend port and add a `healthcheck` URL if your backend has one".into());
    }
    if let Some(f) = &fe {
        notes.push(format!("frontend: {} in `{}`", f.framework, f.dir));
        let mut fj = Map::new();
        if !f.command.is_empty() {
            fj.insert("command".into(), json!(f.command));
            if f.dir != "." {
                fj.insert("cwd".into(), json!(f.dir));
            }
            fj.insert("url".into(), json!(f.url));
        }
        if let Some((cmd, creates)) = &f.setup {
            fj.insert("setup".into(), json!({ "command": cmd, "creates": creates }));
        }
        if let Some(b) = &f.build {
            fj.insert("build".into(), json!(b));
        }
        if let Some(d) = &f.dist {
            fj.insert("dist".into(), json!(d));
        }
        if f.command.is_empty() {
            fj.insert("watch".into(), json!([f.dir]));
        }
        if be.is_some() {
            fj.insert("depends_on".into(), json!(["backend"]));
            if f.command.is_empty() {
                fj.insert("proxy".into(), json!({ "/api": "${services.backend.url}" }));
            }
        }
        cfg.insert("frontend".into(), Value::Object(fj));
    } else {
        notes.push(
            "no frontend detected – set `frontend.url`, `frontend.command` + `url` or `frontend.dist`".into(),
        );
        cfg.insert("frontend".into(), json!({ "url": "http://localhost:3000" }));
    }
    cfg.insert("services".into(), json!(services));
    cfg.insert(
        "agents".into(),
        json!({
            "enabled": true,
            "permissions": {
                "development": ["read", "ui", "actions", "services", "files", "code"],
                "production": ["read", "ui", "actions"]
            },
            "approval_required": ["system"]
        }),
    );
    (Value::Object(cfg), notes)
}

pub fn run(dir: &Path, force: bool, name: Option<String>, toml_out: bool) -> Result<()> {
    let root: PathBuf = dir.to_path_buf();
    if !root.is_dir() {
        bail!("{} is not a directory", root.display());
    }
    for existing in openshell_core::config::CONFIG_FILE_NAMES {
        if root.join(existing).exists() && !force {
            bail!("{existing} already exists (use --force to overwrite)");
        }
    }
    let (cfg, notes) = generate(&root, name);
    // validate what we generated
    let parsed = openshell_core::config::parse_json(&cfg.to_string()).map_err(anyhow::Error::msg)?;
    let issues = openshell_core::config::validate(&parsed);
    let (file, text) = if toml_out {
        let mut v = cfg.clone();
        v.as_object_mut().unwrap().remove("$schema");
        ("openshell.toml", toml_string(&v)?)
    } else {
        ("openshell.json", serde_json::to_string_pretty(&cfg)? + "\n")
    };
    std::fs::write(root.join(file), text)?;
    output::ok(&format!("created {}", bold(file)));
    let app_name = cfg["app"]["name"].as_str().unwrap_or("App").to_string();
    let written =
        super::create::write_agent_files(&root, &openshell_core::config::slug(&app_name), &app_name)?;
    if !written.is_empty() {
        output::ok(&format!("created {} (instructions for AI coding agents)", written.join(", ")));
    } else {
        println!(
            "  {} AGENTS.md exists – run `openshell guide` and add the OpenShell commands to it so coding agents know how to run this app",
            dim("•")
        );
    }
    for n in notes {
        println!("  {} {n}", dim("•"));
    }
    for i in issues {
        output::warn(&i.to_string());
    }
    println!("\nnext: {}  (or {} to check the setup)", bold("openshell dev"), bold("openshell doctor"));
    Ok(())
}

fn toml_string(v: &Value) -> Result<String> {
    let t: toml_value::Value = serde_json::from_value(v.clone())?;
    Ok(toml_value::to_string(&t)?)
}

mod toml_value {
    // tiny indirection so the CLI does not need a direct toml dependency
    pub use openshell_core::config::toml_support::{to_string, Value};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_vite_and_python() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("frontend")).unwrap();
        std::fs::create_dir_all(root.join("backend")).unwrap();
        std::fs::write(
            root.join("frontend/package.json"),
            r#"{"scripts":{"dev":"vite","build":"vite build"},"devDependencies":{"vite":"5"}}"#,
        )
        .unwrap();
        std::fs::write(root.join("backend/main.py"), "print('hi')").unwrap();
        let (cfg, _) = generate(root, Some("Demo".into()));
        assert_eq!(cfg["frontend"]["command"], "npm run dev");
        assert_eq!(cfg["frontend"]["url"], "http://localhost:5173");
        assert_eq!(cfg["frontend"]["dist"], "frontend/dist");
        assert_eq!(cfg["services"][0]["cwd"], "backend");
        let parsed = openshell_core::config::parse_json(&cfg.to_string()).unwrap();
        assert!(openshell_core::config::validate(&parsed).iter().all(|i| !i.is_error()));
    }

    fn detect(files: &[(&str, &str)]) -> Value {
        let dir = tempfile::tempdir().unwrap();
        for (f, c) in files {
            let p = dir.path().join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, c).unwrap();
        }
        let (cfg, _) = generate(dir.path(), Some("X".into()));
        let parsed = openshell_core::config::parse_json(&cfg.to_string()).unwrap();
        let issues = openshell_core::config::validate(&parsed);
        assert!(issues.iter().all(|i| !i.is_error()), "{issues:?}");
        cfg["services"][0].clone()
    }

    #[test]
    fn detects_java_and_cpp() {
        let spring = detect(&[(
            "backend/pom.xml",
            "<project><parent><artifactId>spring-boot-starter-parent</artifactId></parent><artifactId>demo</artifactId><dependencies><dependency>springdoc-openapi</dependency><dependency>spring-boot-starter-actuator</dependency></dependencies></project>",
        )]);
        assert_eq!(spring["command"][0], "mvn");
        assert_eq!(spring["openapi"], "/v3/api-docs");
        assert_eq!(spring["healthcheck"], "http://127.0.0.1:${port}/actuator/health");

        let maven = detect(&[(
            "server/pom.xml",
            "<project><parent><artifactId>p</artifactId><version>9</version></parent><artifactId>shop</artifactId><version>2.1.0</version></project>",
        )]);
        assert_eq!(maven["command"], json!(["java", "-jar", "target/shop-2.1.0.jar"]));
        assert_eq!(maven["build"][0], "mvn");

        let javac = detect(&[("backend/src/Main.java", "class Main {}")]);
        assert_eq!(javac["build"], json!(["javac", "-d", "build/classes", "src/Main.java"]));
        assert_eq!(javac["watch"], json!(["backend/src"]));

        let cpp = detect(&[("backend/CMakeLists.txt", "project(x)\nadd_executable(my-server src/main.cpp)")]);
        assert_eq!(cpp["command"], json!(["build/my-server"]));
        assert!(cpp["build"].as_str().unwrap().contains("cmake --build"));
    }
}
