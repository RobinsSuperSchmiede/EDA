//! `openshell create <name> --template <template>`

use crate::output::{self, bold, dim};
use anyhow::{bail, Context, Result};
use include_dir::{include_dir, Dir, DirEntry};
use std::path::Path;

static TEMPLATES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../templates");

pub fn template_names() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = TEMPLATES
        .dirs()
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

fn slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.trim_matches('-').split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-")
}

fn title(name: &str) -> String {
    slug(name)
        .split('-')
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
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
                let out_rel = if name == "_gitignore" { rel.with_file_name(".gitignore") } else { rel.to_path_buf() };
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

pub fn run(name: &str, template: &str, list: bool) -> Result<()> {
    let templates = template_names();
    if list {
        for (t, d) in &templates {
            println!("  {:<16} {}", bold(t), dim(d));
        }
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
        &target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| name.to_string()),
    );
    if project.is_empty() {
        bail!("invalid project name `{name}`");
    }
    let app_name = title(&project);
    std::fs::create_dir_all(target).with_context(|| format!("cannot create {}", target.display()))?;
    write_dir(dir, dir.path(), target, &project, &app_name)?;
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
    }
}
