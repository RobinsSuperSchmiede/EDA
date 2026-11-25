//! `openshell create <name> --template <template>`

use super::add::{self, Block, ConfigFile, Lang};
use crate::output::{self, bold, dim};
use anyhow::{bail, Context, Result};
use include_dir::{include_dir, Dir, DirEntry};
use std::io::IsTerminal;
use std::path::Path;

pub(crate) static TEMPLATES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../templates");

pub fn template_names() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = TEMPLATES
        .dirs()
        .filter(|d| !d.path().to_string_lossy().starts_with('_'))
        .map(|d| {
            let name = d.path().to_string_lossy().to_string();
            let desc = d
                .get_file(d.path().join("template.json"))
                .and_then(|f| f.contents_utf8())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v["description"].as_str().map(str::to_string))
                .unwrap_or_default();
            (name, desc)
        })
        .collect();
    out.sort();
    out
}

pub(crate) fn slug(name: &str) -> String {
    let s: String =
        name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    s.trim_matches('-').split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

pub(crate) fn title(name: &str) -> String {
    slug(name)
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A file from `templates/_shared` (added to every project), rendered for an app.
pub fn shared_file(file: &str, project: &str, app_name: &str) -> Option<String> {
    TEMPLATES
        .get_file(format!("_shared/{file}"))
        .and_then(|f| f.contents_utf8())
        .map(|t| render(t, project, app_name))
}

/// Write `AGENTS.md` and `CLAUDE.md` (guides for AI coding agents) if they do not exist yet.
/// Returns the names of the files written.
pub fn write_agent_files(target: &Path, project: &str, app_name: &str) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for file in ["AGENTS.md", "CLAUDE.md"] {
        let path = target.join(file);
        if path.exists() {
            continue;
        }
        if let Some(text) = shared_file(file, project, app_name) {
            std::fs::write(&path, text)?;
            written.push(file.to_string());
        }
    }
    Ok(written)
}

fn render(text: &str, project: &str, app_name: &str) -> String {
    text.replace("{{app_name}}", app_name)
        .replace("{{app_id}}", &format!("dev.openshell.{}", project.replace('-', "")))
        .replace("{{project_name}}", project)
}

fn write_dir(dir: &Dir<'_>, base: &Path, target: &Path, project: &str, app_name: &str) -> Result<()> {
    for entry in dir.entries() {
        let rel = entry.path().strip_prefix(base).unwrap_or(entry.path());
        match entry {
            DirEntry::Dir(d) => {
                std::fs::create_dir_all(target.join(rel))?;
                write_dir(d, base, target, project, app_name)?;
            }
            DirEntry::File(f) => {
                let name = rel.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if name == "template.json" && rel.parent().map(|p| p.as_os_str().is_empty()).unwrap_or(true) {
                    continue;
                }
                let out_rel =
                    if name == "_gitignore" { rel.with_file_name(".gitignore") } else { rel.to_path_buf() };
                let out = target.join(out_rel);
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                match f.contents_utf8() {
                    Some(text) => std::fs::write(&out, render(text, project, app_name))?,
                    None => std::fs::write(&out, f.contents())?,
                }
            }
        }
    }
    Ok(())
}

const SCHEMA_URL: &str =
    "https://raw.githubusercontent.com/RobinsSuperSchmiede/EDA/main/schema/openshell.schema.json";

/// `openshell create`: template, building blocks, or ask interactively.
pub fn run_command(
    name: &str,
    template: Option<String>,
    with: Vec<String>,
    lang: Lang,
    list: bool,
) -> Result<()> {
    if list {
        return run(name, "", true);
    }
    if let Some(t) = template {
        return run(name, &t, false);
    }
    let blocks = if !with.is_empty() {
        parse_blocks(&with, lang)?
    } else if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        match ask_blocks()? {
            Some(b) => b,
            None => return run(name, "vanilla-python", false),
        }
    } else {
        return run(name, "vanilla-python", false);
    };
    compose(name, &blocks)
}

fn parse_blocks(with: &[String], lang: Lang) -> Result<Vec<Block>> {
    let mut blocks = Vec::new();
    for w in with {
        let w = w.trim().to_lowercase();
        let block = match w.as_str() {
            "frontend" | "ui" | "web" => Block::Frontend,
            "backend" | "api" | "server" => Block::Backend(lang),
            "database" | "db" | "sqlite" => Block::Database("main".into()),
            "functions" | "function" | "mcp" | "tools" | "ai" => Block::Function("hello".into(), lang),
            "" => continue,
            other => bail!("unknown building block `{other}` (frontend, backend, database, functions)"),
        };
        blocks.push(block);
    }
    if blocks.is_empty() {
        bail!("choose at least one building block: frontend, backend, database, functions");
    }
    Ok(blocks)
}

/// Ask which building blocks the app should have. `None` = use the default template.
fn ask_blocks() -> Result<Option<Vec<Block>>> {
    use std::io::Write;
    println!("{}", bold("Which building blocks should the app have?"));
    println!("  1) frontend    web UI in the native window");
    println!("  2) backend     HTTP service (Python or Node.js)");
    println!("  3) database    built-in SQLite database (usable by frontend, backend and AI agents)");
    println!("  4) functions   functions for AI agents (MCP tools as scripts, no server)");
    println!("  t) a ready-made template instead (`openshell create --list`)");
    print!("Choose, e.g. 1,3 or 2,3,4 [1,2]: ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    let line = line.trim();
    if line.eq_ignore_ascii_case("t") {
        return Ok(None);
    }
    let choice = if line.is_empty() { "1,2" } else { line };
    let picks: Vec<&str> = choice.split([',', ' ']).filter(|s| !s.is_empty()).collect();
    let needs_lang = picks.iter().any(|p| *p == "2" || *p == "4");
    let lang = if needs_lang {
        print!("Language for backend / functions – python or node [python]: ");
        std::io::stdout().flush()?;
        let mut l = String::new();
        std::io::stdin().read_line(&mut l)?;
        if l.trim().starts_with('n') {
            Lang::Node
        } else {
            Lang::Python
        }
    } else {
        Lang::Python
    };
    let names: Vec<String> = picks
        .iter()
        .map(|p| match *p {
            "1" => Ok("frontend".to_string()),
            "2" => Ok("backend".to_string()),
            "3" => Ok("database".to_string()),
            "4" => Ok("functions".to_string()),
            other => Err(anyhow::anyhow!("unknown choice `{other}`")),
        })
        .collect::<Result<_>>()?;
    Ok(Some(parse_blocks(&names, lang)?))
}

/// Create a project composed of building blocks.
pub fn compose(name: &str, blocks: &[Block]) -> Result<()> {
    let target = Path::new(name);
    if target.exists() && target.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true) {
        bail!("{} already exists and is not empty", target.display());
    }
    let project = slug(
        &target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string()),
    );
    if project.is_empty() {
        bail!("invalid project name `{name}`");
    }
    let app_name = title(&project);
    std::fs::create_dir_all(target).with_context(|| format!("cannot create {}", target.display()))?;
    let base = serde_json::json!({
        "$schema": SCHEMA_URL,
        "version": 1,
        "app": {
            "name": app_name,
            "id": format!("dev.openshell.{}", project.replace('-', "")),
            "version": "0.1.0"
        },
        "window": { "width": 1100, "height": 760 }
    });
    std::fs::write(target.join("openshell.json"), serde_json::to_string_pretty(&base)? + "\n")?;
    let mut cfg = ConfigFile::open(target)?;
    // order matters only for readability of openshell.json
    let rank = |b: &Block| match b {
        Block::Database(_) => 0,
        Block::Backend(_) => 1,
        Block::Function(..) => 2,
        Block::Frontend => 3,
    };
    let mut sorted: Vec<&Block> = blocks.iter().collect();
    sorted.sort_by_key(|b| rank(b));
    sorted.dedup_by_key(|b| rank(b));
    for b in &sorted {
        add::apply(&mut cfg, b)?;
    }
    cfg.save()?;
    std::fs::write(target.join(".gitignore"), ".openshell/\n__pycache__/\nnode_modules/\ndata/\n")?;
    let labels: Vec<String> = sorted.iter().map(|b| b.label()).collect();
    std::fs::write(target.join("README.md"), composed_readme(&app_name, &project, &sorted))?;
    write_agent_files(target, &project, &app_name)?;
    output::ok(&format!("created {} with {}", bold(&target.display().to_string()), labels.join(", ")));
    println!("\n  cd {}\n  openshell dev\n", target.display());
    println!("  {}", dim("add more later: openshell add frontend | backend | database | function <name>"));
    Ok(())
}

fn composed_readme(app_name: &str, project: &str, blocks: &[&Block]) -> String {
    let mut parts = String::new();
    for b in blocks {
        parts.push_str(match b {
            Block::Frontend => "- **Frontend** – `web/` (HTML/JS, served by OpenShell, reloads on save)\n",
            Block::Backend(_) => "- **Backend** – `backend/` (HTTP service with `/health` and `/api/hello`)\n",
            Block::Database(_) => "- **Database** – SQLite, migrations in `db/migrations/` (file lives in the app's data directory)\n",
            Block::Function(..) => "- **Functions for AI agents** – `tools/` (scripts: JSON in on stdin, JSON out on stdout)\n",
        });
    }
    format!(
        "# {app_name}\n\n\
         An [OpenShell](https://github.com/RobinsSuperSchmiede/EDA) app made of these building blocks:\n\n\
         {parts}\n\
         ## Run\n\n\
         ```bash\n\
         openshell dev            # with window (apps without frontend show the OpenShell console)\n\
         openshell dev --headless # without window\n\
         ```\n\n\
         ## Extend\n\n\
         ```bash\n\
         openshell add frontend\n\
         openshell add backend --lang node\n\
         openshell add database\n\
         openshell add function summarize\n\
         ```\n\n\
         ## AI agents\n\n\
         ```bash\n\
         claude mcp add {project} -- openshell mcp --app --start --project .   # use the app\n\
         claude mcp add {project}-dev -- openshell mcp --project .            # develop it\n\
         ```\n"
    )
}

/// `openshell languages`: every backend language with its support level and templates.
pub fn print_languages() {
    use openshell_core::config::runtimes::LANGUAGES;
    for l in LANGUAGES {
        let templates = if l.templates.is_empty() { "-".to_string() } else { l.templates.join(", ") };
        println!(
            "  {:<20} {:<12} {}",
            bold(l.title),
            l.support.label(),
            dim(&format!("runtime: {} · templates: {templates}", l.runtime.unwrap_or("none (compiled)")))
        );
        let detected = crate::languages::DETECTORS.iter().any(|d| d.language == l.id);
        println!(
            "  {:<20} {}",
            "",
            dim(&format!("{}{}", l.note, if detected { " · detected by `openshell init`" } else { "" }))
        );
    }
    println!(
        "
  {}",
        dim("anything else works if it reads PORT, binds 127.0.0.1 and answers /health (\"by contract\")")
    );
}

pub fn run(name: &str, template: &str, list: bool) -> Result<()> {
    let templates = template_names();
    if list {
        for (t, d) in &templates {
            let lang = openshell_core::config::runtimes::language_of_template(t)
                .map(|l| format!("{} · {}", l.title, l.support.label()))
                .unwrap_or_default();
            println!(
                "  {:<16} {} {}",
                bold(t),
                dim(d),
                dim(&if lang.is_empty() { lang } else { format!("[{lang}]") })
            );
        }
        println!(
            "
  {}",
            dim("languages and how well they are tested: openshell languages")
        );
        return Ok(());
    }
    let Some(dir) = TEMPLATES.get_dir(template) else {
        let names: Vec<_> = templates.iter().map(|(t, _)| t.as_str()).collect();
        bail!("unknown template `{template}` (available: {})", names.join(", "));
    };
    let target = Path::new(name);
    if target.exists() && target.read_dir().map(|mut d| d.next().is_some()).unwrap_or(true) {
        bail!("{} already exists and is not empty", target.display());
    }
    let project = slug(
        &target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string()),
    );
    if project.is_empty() {
        bail!("invalid project name `{name}`");
    }
    let app_name = title(&project);
    std::fs::create_dir_all(target).with_context(|| format!("cannot create {}", target.display()))?;
    write_dir(dir, dir.path(), target, &project, &app_name)?;
    write_agent_files(target, &project, &app_name)?;
    output::ok(&format!("created {} from template {}", bold(&target.display().to_string()), bold(template)));
    println!("\n  cd {}\n  openshell dev\n", target.display());
    if template != "vanilla-python" && template != "custom" {
        println!("  {}", dim("the first start runs `npm install` for the frontend automatically"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_are_embedded_and_valid() {
        let names = template_names();
        assert!(names.iter().any(|(n, _)| n == "vanilla-python"));
        for (name, _) in names {
            let dir = TEMPLATES.get_dir(&name).unwrap();
            let cfg = dir
                .get_file(dir.path().join("openshell.json"))
                .unwrap_or_else(|| panic!("{name} has no openshell.json"));
            let text = render(cfg.contents_utf8().unwrap(), "demo-app", "Demo App");
            let parsed = openshell_core::config::parse_json(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            let issues = openshell_core::config::validate(&parsed);
            assert!(issues.iter().all(|i| !i.is_error()), "{name}: {issues:?}");
        }
    }

    #[test]
    fn composes_projects_from_blocks() {
        for with in [
            vec!["frontend"],
            vec!["backend"],
            vec!["database"],
            vec!["functions"],
            vec!["database", "functions"],
            vec!["frontend", "database"],
            vec!["frontend", "backend", "database", "functions"],
        ] {
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("app");
            let names: Vec<String> = with.iter().map(|s| s.to_string()).collect();
            compose(target.to_str().unwrap(), &parse_blocks(&names, Lang::Python).unwrap())
                .unwrap_or_else(|e| panic!("{with:?}: {e}"));
            let project = openshell_core::config::Project::load(&target).unwrap();
            assert_eq!(project.config.frontend.is_some(), with.contains(&"frontend"), "{with:?}");
            assert_eq!(!project.config.databases.is_empty(), with.contains(&"database"));
            assert!(target.join("AGENTS.md").exists());
            assert!(target.join("README.md").exists());
        }
        assert!(parse_blocks(&["nonsense".into()], Lang::Node).is_err());
    }

    #[test]
    fn creates_project() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("My Cool App");
        run(target.to_str().unwrap(), "vanilla-python", false).unwrap();
        let cfg = std::fs::read_to_string(target.join("openshell.json")).unwrap();
        assert!(cfg.contains("\"name\": \"My Cool App\""));
        assert!(cfg.contains("dev.openshell.mycoolapp"));
        assert!(target.join(".gitignore").exists());
        assert!(!target.join("template.json").exists());
        assert!(target.join("backend/main.py").exists());
        let agents = std::fs::read_to_string(target.join("AGENTS.md")).unwrap();
        assert!(agents.contains("# AGENTS.md – My Cool App"));
        assert_eq!(std::fs::read_to_string(target.join("CLAUDE.md")).unwrap().trim(), "@AGENTS.md");
        assert!(!template_names().iter().any(|(n, _)| n.starts_with('_')));
    }
}
