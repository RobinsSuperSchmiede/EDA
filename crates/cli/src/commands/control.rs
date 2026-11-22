//! Commands that talk to a running shell: status, logs, stop, restart, tools, approvals, ...

use super::connect;
use crate::output::{self, bold, cyan, dim, green, red, yellow};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;

pub async fn status(dir: &Path, as_json: bool) -> Result<()> {
    let client = connect(dir).await?;
    let s = client.get("/v1/status").await?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&s)?);
        return Ok(());
    }
    println!(
        "{} {}  {} mode · {} · pid {}",
        bold(s["app"].as_str().unwrap_or("?")),
        dim(s["version"].as_str().unwrap_or("")),
        s["mode"].as_str().unwrap_or("?"),
        s["host"].as_str().unwrap_or("?"),
        s["pid"]
    );
    if let Some(url) = s["frontend_url"].as_str() {
        println!("{} {url}", dim("frontend:"));
    }
    println!("{} {}", dim("control: "), s["control_url"].as_str().unwrap_or(""));
    println!();
    println!(
        "  {}",
        dim(&format!(
            "{:<14} {:<10} {:<8} {:<7} {:<7} {:<8} {:<9} {}",
            "SERVICE", "STATE", "PID", "PORT", "CPU", "MEMORY", "RESTARTS", "INFO"
        ))
    );

    for svc in s["services"].as_array().into_iter().flatten() {
        let state = svc["state"].as_str().unwrap_or("?");
        let info = svc["last_error"]
            .as_str()
            .map(red)
            .or_else(|| svc["url"].as_str().map(str::to_string))
            .unwrap_or_default();
        let cpu =
            svc["metrics"]["cpu_percent"].as_f64().map(|c| format!("{c:.1}%")).unwrap_or_else(|| "-".into());
        let mem = svc["metrics"]["memory_bytes"]
            .as_u64()
            .map(|b| format!("{:.0} MB", b as f64 / 1_048_576.0))
            .unwrap_or_else(|| "-".into());
        println!(
            "  {:<14} {} {:<8} {:<7} {:<7} {:<8} {:<9} {}",
            svc["name"].as_str().unwrap_or("?"),
            output::state_padded(state, 10),
            svc["pid"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            svc["port"].as_u64().map(|p| p.to_string()).unwrap_or_else(|| "-".into()),
            cpu,
            mem,
            svc["restarts"],
            info
        );
    }
    let dbs = s["databases"].as_array().cloned().unwrap_or_default();
    if !dbs.is_empty() {
        println!();
        for db in &dbs {
            let tables: Vec<String> = db["tables"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|t| format!("{} ({})", t["name"].as_str().unwrap_or("?"), t["rows"]))
                .collect();
            println!(
                "  {} {:<12} {}  {}",
                dim("database"),
                db["name"].as_str().unwrap_or("?"),
                dim(&format!("agents: {}", db["agent_access"].as_str().unwrap_or("?"))),
                if tables.is_empty() { dim("no tables") } else { tables.join(", ") }
            );
        }
    }
    let caps = s["capabilities"].as_array().cloned().unwrap_or_default();
    if !caps.is_empty() {
        let names: Vec<&str> = caps.iter().filter_map(|c| c["name"].as_str()).collect();
        println!("\n{} {}", dim("functions for agents:"), names.join(", "));
    }
    let pages = s["ui_pages"].as_array().map(|a| a.len()).unwrap_or(0);
    println!();
    println!("{} {pages} page(s) connected", dim("ui bridge:"));
    let pending = s["pending_approvals"].as_array().map(|a| a.len()).unwrap_or(0);
    if pending > 0 {
        println!("{} {pending} pending – run `openshell approvals`", yellow("approvals:"));
    }
    Ok(())
}

pub async fn stop(dir: &Path, service: Option<String>) -> Result<()> {
    let client = connect(dir).await?;
    match service {
        Some(name) => {
            client.post(&format!("/v1/services/{name}/stop"), &json!({})).await?;
            output::ok(&format!("stopping `{name}`"));
        }
        None => {
            client.post("/v1/shutdown", &json!({})).await?;
            // wait until the control API is gone
            for _ in 0..150 {
                tokio::time::sleep(Duration::from_millis(200)).await;
                if client.get("/v1/health").await.is_err() {
                    output::ok(&format!("`{}` stopped", client.info.app));
                    return Ok(());
                }
            }
            output::warn("the shell is still shutting down");
        }
    }
    Ok(())
}

pub async fn service(dir: &Path, name: &str, action: &str) -> Result<()> {
    let client = connect(dir).await?;
    client.post(&format!("/v1/services/{name}/{action}"), &json!({})).await?;
    // wait for the outcome
    tokio::time::sleep(Duration::from_millis(300)).await;
    for _ in 0..150 {
        let s = client.get("/v1/status").await?;
        let svc = s["services"]
            .as_array()
            .and_then(|a| a.iter().find(|x| x["name"] == name).cloned())
            .context("service disappeared")?;
        match svc["state"].as_str().unwrap_or("") {
            "running" => {
                output::ok(&format!("`{name}` is running"));
                return Ok(());
            }
            "failed" | "crashed" | "exited" => {
                bail!(
                    "`{name}` is {}: {}\nsee `openshell logs {name}`",
                    svc["state"].as_str().unwrap_or("?"),
                    svc["last_error"].as_str().unwrap_or("")
                );
            }
            _ => tokio::time::sleep(Duration::from_millis(200)).await,
        }
    }
    output::warn(&format!("`{name}` is not ready yet – check `openshell status`"));
    Ok(())
}

pub async fn logs(
    dir: &Path,
    service: Option<String>,
    follow: bool,
    lines: usize,
    errors: bool,
    grep: Option<String>,
) -> Result<()> {
    let client = connect(dir).await?;
    let mut query = format!("limit={lines}");
    if let Some(s) = &service {
        query.push_str(&format!("&service={s}"));
    }
    if errors {
        query.push_str("&errors_only=true");
    }
    if let Some(g) = &grep {
        query.push_str(&format!("&contains={}", g.replace(' ', "%20")));
    }
    let width = 10;
    let mut since: Option<u64> = None;
    loop {
        let q = match since {
            Some(s) => format!("{query}&since={s}"),
            None => query.clone(),
        };
        let v = match client.get(&format!("/v1/logs?{q}")).await {
            Ok(v) => v,
            Err(e) if since.is_some() => {
                output::warn(&format!("connection lost: {e}"));
                return Ok(());
            }
            Err(e) => return Err(e),
        };
        for line in v["lines"].as_array().into_iter().flatten() {
            let svc = line["service"].as_str().unwrap_or("?");
            let text = line["text"].as_str().unwrap_or("");
            let ts = line["ts"].as_str().unwrap_or("");
            let time = ts.get(11..19).unwrap_or("");
            let text = match line["stream"].as_str() {
                Some("stderr") => output::paint("91", text),
                Some("system") => dim(text),
                _ => text.to_string(),
            };
            println!("{} {} {text}", dim(time), output::service_tag(svc, width));
        }
        since = Some(v["last_seq"].as_u64().unwrap_or(0));
        if !follow {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub async fn tools(dir: &Path, as_json: bool) -> Result<()> {
    let client = connect(dir).await?;
    let v = client.get("/v1/tools").await?;
    if as_json {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    for t in v["tools"].as_array().into_iter().flatten() {
        let access = match t["access"].as_str().unwrap_or("") {
            "allowed" => green("allowed "),
            "approval" => yellow("approval"),
            _ => red("denied  "),
        };
        println!(
            "  {access} {:<20} {:<9} {}",
            t["name"].as_str().unwrap_or(""),
            dim(t["permission"].as_str().unwrap_or("")),
            t["description"].as_str().unwrap_or("").chars().take(90).collect::<String>()
        );
    }
    Ok(())
}

pub async fn call(dir: &Path, tool: &str, args: Option<String>) -> Result<()> {
    let client = connect(dir).await?.with_agent("cli");
    let args: Value = match args {
        Some(a) => {
            serde_json::from_str(&a).context("arguments must be JSON, e.g. '{\"name\":\"backend\"}'")?
        }
        None => json!({}),
    };
    let r = client.post(&format!("/v1/tools/{tool}"), &args).await?;
    if r["ok"].as_bool() == Some(true) {
        println!("{}", serde_json::to_string_pretty(&r["result"])?);
        Ok(())
    } else {
        bail!("{}", r["result"]["error"].as_str().unwrap_or("tool failed"))
    }
}

pub async fn approvals(dir: &Path) -> Result<()> {
    let client = connect(dir).await?;
    let v = client.get("/v1/approvals").await?;
    let list = v["pending"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("no pending approvals");
        return Ok(());
    }
    for r in list {
        let id = r["id"].as_str().unwrap_or("");
        println!(
            "  {} {} wants to {}  {}",
            cyan(id),
            bold(r["agent"].as_str().unwrap_or("?")),
            r["summary"].as_str().unwrap_or("?"),
            dim(&format!("[{}]", r["permission"].as_str().unwrap_or(""))),
        );
    }
    println!("\n  {} openshell approve <id>  |  openshell deny <id>", dim("answer with"));
    Ok(())
}

pub async fn decide(dir: &Path, id: &str, approve: bool) -> Result<()> {
    let client = connect(dir).await?;
    let action = if approve { "approve" } else { "deny" };
    client.post(&format!("/v1/approvals/{id}/{action}"), &json!({})).await?;
    output::ok(if approve { "approved" } else { "denied" });
    Ok(())
}

pub async fn audit(dir: &Path, lines: usize) -> Result<()> {
    let client = connect(dir).await?;
    let v = client.get(&format!("/v1/audit?limit={lines}")).await?;
    for e in v["entries"].as_array().into_iter().flatten() {
        let outcome = match e["outcome"].as_str().unwrap_or("") {
            "ok" => green("ok      "),
            "error" => yellow("error   "),
            "denied" => red("denied  "),
            _ => red("rejected"),
        };
        let ts = e["ts"].as_str().unwrap_or("");
        println!(
            "  {} {outcome} {:<14} {:<18} {}",
            dim(ts.get(11..19).unwrap_or("")),
            e["agent"].as_str().unwrap_or("?"),
            e["tool"].as_str().unwrap_or("?"),
            dim(&e["message"].as_str().unwrap_or("").chars().take(80).collect::<String>())
        );
    }
    Ok(())
}
