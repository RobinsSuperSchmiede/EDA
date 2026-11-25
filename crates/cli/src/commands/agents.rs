//! `openshell agents` – who may do what: the agent layer on/off, effective permissions per mode,
//! agent sessions with their own tokens and scopes, paired devices.
//!
//! ```text
//! openshell agents                                   # effective permissions, sessions, devices
//! openshell agents enable | disable                  # agents.enabled in openshell.json
//! openshell agents token claude --scopes read,actions # a session token (shown once)
//! openshell agents token builder --full               # developer tools as well
//! openshell agents tokens                            # list sessions
//! openshell agents revoke claude
//! ```

use super::add::ConfigFile;
use super::load_project;
use crate::output::{self, bold, cyan, dim, green, red, yellow};
use anyhow::{anyhow, bail, Result};
use openshell_core::config::{Mode, Permission};
use openshell_core::sessions::{SessionProfile, Sessions};
use openshell_core::state::ControlClient;
use serde_json::{json, Value};
use std::path::Path;

pub enum Action {
    Show,
    Enable,
    Disable,
    Token { name: String, scopes: Vec<String>, full: bool },
    Tokens,
    Revoke { name: String },
}

fn parse_scopes(list: &[String]) -> Result<Vec<Permission>> {
    list.iter()
        .filter(|s| !s.trim().is_empty())
        .map(|s| {
            serde_json::from_value::<Permission>(json!(s.trim())).map_err(|_| {
                anyhow!("unknown scope `{s}` (read, ui, actions, services, files, code, system)")
            })
        })
        .collect()
}

fn scopes_text(v: &Value) -> String {
    let list: Vec<&str> = v.as_array().into_iter().flatten().filter_map(|s| s.as_str()).collect();
    if list.is_empty() {
        "none".into()
    } else {
        list.join(", ")
    }
}

pub async fn run(dir: &Path, action: Action) -> Result<()> {
    match action {
        Action::Show => show(dir).await,
        Action::Enable | Action::Disable => {
            let on = matches!(action, Action::Enable);
            let mut cfg = ConfigFile::open(dir)?;
            let obj =
                cfg.value.as_object_mut().ok_or_else(|| anyhow!("openshell.json is not a JSON object"))?;
            let agents = obj.entry("agents").or_insert_with(|| json!({}));
            if !agents.is_object() {
                *agents = json!({});
            }
            agents.as_object_mut().unwrap().insert("enabled".into(), json!(on));
            cfg.save()?;
            output::ok(&format!("agents {} in openshell.json", if on { "enabled" } else { "disabled" }));
            println!("  {} restart the app to apply it", dim("•"));
            Ok(())
        }
        Action::Token { name, scopes, full } => {
            let p = load_project(dir)?;
            let scopes = parse_scopes(&scopes)?;
            let profile = if full { SessionProfile::Full } else { SessionProfile::App };
            let token = Sessions::load(&p.data_dir()).create(&name, profile, scopes.clone())?;
            output::ok(&format!(
                "agent session `{name}` ({} profile, scopes: {})",
                if full { "developer" } else { "app" },
                if scopes.is_empty() {
                    "everything the app grants agents".to_string()
                } else {
                    scopes.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                }
            ));
            println!("\n  {} {}\n", dim("token (shown once):"), bold(&token));
            let mut args = vec!["mcp".to_string()];
            if !full {
                args.push("--app".into());
            }
            args.extend(["--project".into(), p.root.display().to_string()]);
            let snippet = json!({ "mcpServers": { name.clone(): {
                "command": "openshell", "args": args, "env": { "OPENSHELL_AGENT_TOKEN": token }
            } } });
            println!("{}", dim("MCP client configuration:"));
            println!("{}", serde_json::to_string_pretty(&snippet)?);
            if !p.config.agents.enabled {
                output::warn(
                    "agents are off for this app – `openshell agents enable` before the token can be used",
                );
            }
            Ok(())
        }
        Action::Tokens => {
            let p = load_project(dir)?;
            let list = Sessions::load(&p.data_dir()).list();
            if list.is_empty() {
                println!("no agent sessions – create one with {}", bold("openshell agents token <name>"));
            }
            for s in list {
                println!(
                    "  {:<16} {:<5} {}",
                    s.name,
                    if s.profile == SessionProfile::Full { "full" } else { "app" },
                    dim(&format!(
                        "scopes: {} · created {}",
                        if s.scopes.is_empty() {
                            "all granted".into()
                        } else {
                            s.scopes.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")
                        },
                        s.created_at.format("%Y-%m-%d %H:%M")
                    ))
                );
            }
            Ok(())
        }
        Action::Revoke { name } => {
            let p = load_project(dir)?;
            if !Sessions::load(&p.data_dir()).revoke(&name)? {
                bail!("no agent session `{name}`");
            }
            output::ok(&format!("revoked `{name}` – its token stops working at once"));
            Ok(())
        }
    }
}

async fn show(dir: &Path) -> Result<()> {
    let p = load_project(dir)?;
    let running = ControlClient::connect(&p.state_dir()).await.ok();
    let info = match &running {
        Some(c) => c.get("/v1/agents").await.ok(),
        None => None,
    };
    let enabled = p.config.agents.enabled;
    println!(
        "{} {}",
        bold("Agent layer:"),
        if enabled {
            green("on")
        } else {
            yellow("off – agents cannot connect (`openshell agents enable`)")
        }
    );
    let cfg = &p.config.agents;
    for mode in [Mode::Development, Mode::Production] {
        let granted: Vec<&str> =
            if enabled { cfg.permissions_for(mode).iter().map(|x| x.as_str()).collect() } else { vec![] };
        println!(
            "  {:<12} {}",
            dim(&mode.to_string()),
            if granted.is_empty() { dim("nothing") } else { granted.join(", ") }
        );
    }
    let approval: Vec<&str> = cfg.approval_required().iter().map(|x| x.as_str()).collect();
    println!("  {:<12} {}", dim("approval"), approval.join(", "));
    println!(
        "  {:<12} {}",
        dim("frontend"),
        cfg.sdk_permissions().iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")
    );
    match &info {
        Some(i) => println!(
            "  {:<12} {} ({} mode)",
            dim("now"),
            scopes_text(&i["granted"]),
            i["mode"].as_str().unwrap_or("?")
        ),
        None => println!("  {:<12} {}", dim("now"), dim("the app is not running")),
    }

    println!("\n{}", bold("Agent sessions"));
    let sessions: Vec<Value> = match &info {
        Some(i) => i["sessions"].as_array().cloned().unwrap_or_default(),
        None => Sessions::load(&p.data_dir()).list().into_iter().map(|s| json!(s)).collect(),
    };
    if sessions.is_empty() {
        println!(
            "  {} none – agents use the admin token (`openshell mcp`); give each its own with {}",
            dim("·"),
            cyan("openshell agents token <name>")
        );
    }
    for s in sessions {
        println!(
            "  {:<16} {:<5} {} {}",
            s["name"].as_str().unwrap_or("?"),
            s["profile"].as_str().unwrap_or("app"),
            dim("effective:"),
            if s.get("effective").is_some() {
                scopes_text(&s["effective"])
            } else {
                scopes_text(&s["scopes"])
            }
        );
    }
    if let Some(i) = &info {
        if let Some(devs) = i["devices"].as_array() {
            println!("\n{}", bold("Paired devices (remote gateway)"));
            if devs.is_empty() {
                println!("  {} none", dim("·"));
            }
            for d in devs {
                println!(
                    "  {:<16} {} {}",
                    d["name"].as_str().unwrap_or("?"),
                    dim("effective:"),
                    scopes_text(&d["effective"])
                );
            }
        }
        let pending = i["pending_approvals"].as_u64().unwrap_or(0);
        if pending > 0 {
            println!("\n  {} {pending} approval(s) pending – `openshell approvals`", red("!"));
        }
    }
    Ok(())
}
