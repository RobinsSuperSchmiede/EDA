//! `openshell remote` – the remote gateway: status, device pairing (with QR code), revoking,
//! unblocking addresses.

use super::connect;
use crate::output::{self, bold, cyan, dim, yellow};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::Path;

pub enum Action {
    Show,
    Pair { device: String, qr: bool },
    Devices,
    Revoke { device: String },
    Unblock { address: String },
}

pub async fn run(dir: &Path, action: Action) -> Result<()> {
    let client = connect(dir).await?;
    let r = client.get("/v1/remote").await?;
    if r["enabled"].as_bool() != Some(true) {
        println!("the remote gateway is off");
        println!(
            "\n  {} set {} in openshell.json and restart the app",
            dim("enable it:"),
            cyan(r#""runtime": { "remote": { "enabled": true } }"#)
        );
        if matches!(action, Action::Show) {
            return Ok(());
        }
        bail!("the remote gateway is off");
    }
    match action {
        Action::Show => show(&r),
        Action::Pair { device, qr } => {
            let p = client.post("/v1/remote/devices", &json!({ "name": device })).await?;
            pair(&p, qr)
        }
        Action::Devices => {
            let d = client.get("/v1/remote/devices").await?;
            devices(&d);
            Ok(())
        }
        Action::Revoke { device } => {
            client.post(&format!("/v1/remote/devices/{device}/revoke"), &json!({})).await?;
            output::ok(&format!("revoked `{device}`: its token no longer works"));
            Ok(())
        }
        Action::Unblock { address } => {
            let u = client.post("/v1/remote/unblock", &json!({ "address": address })).await?;
            if u["ok"].as_bool() == Some(true) {
                output::ok(&format!("{address} is no longer blocked"));
            } else {
                println!("{address} was not blocked");
            }
            Ok(())
        }
    }
}

fn mcp_snippet(url: &Value, token: &str) -> Result<String> {
    Ok(serde_json::to_string_pretty(&json!({
        "url": url,
        "headers": { "Authorization": format!("Bearer {token}") }
    }))?)
}

fn show(r: &Value) -> Result<()> {
    println!(
        "{} {}",
        bold("remote gateway"),
        dim(&format!("listening on {}:{}", r["bind"].as_str().unwrap_or("?"), r["port"]))
    );
    for url in r["urls"].as_array().into_iter().flatten() {
        println!("  {} {}", dim("url:     "), url.as_str().unwrap_or(""));
    }
    println!("  {} {}", dim("mcp:     "), r["mcp_url"].as_str().unwrap_or(""));
    match r["tls_fingerprint"].as_str() {
        Some(fp) => {
            let kind =
                if r["self_signed"].as_bool() == Some(true) { "self-signed, pin it" } else { "certificate" };
            println!("  {} {} {}", dim("tls:     "), fp, dim(&format!("({kind})")));
        }
        None => println!("  {} {}", dim("tls:     "), dim("off – plain HTTP, use a trusted network or VPN")),
    }
    let allow: Vec<&str> = r["allow"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
    println!(
        "  {} {}",
        dim("allow:   "),
        if allow.is_empty() { dim("everyone who reaches the port") } else { allow.join(", ") }
    );
    for e in r["exposed"].as_array().into_iter().flatten() {
        println!(
            "  {} {:<12} {} {}",
            dim("exposed: "),
            e["service"].as_str().unwrap_or("?"),
            e["url"].as_str().unwrap_or(""),
            dim(&format!("(login: {})", e["auth"].as_str().unwrap_or("?")))
        );
    }
    let blocked: Vec<&str> =
        r["blocked"].as_array().into_iter().flatten().filter_map(|a| a.as_str()).collect();
    if !blocked.is_empty() {
        println!("  {} {}", dim("blocked: "), yellow(&blocked.join(", ")));
    }
    devices(&json!({ "devices": r["devices"] }));
    match r["token"].as_str() {
        Some(token) => {
            println!("\n  {} {}", dim("shared token:"), yellow(token));
            println!("\n{}", dim("MCP client configuration (streamable HTTP):"));
            println!("{}", mcp_snippet(&r["mcp_url"], token)?);
            println!(
                "\n{}",
                dim("Anyone with the shared token can use the app. Prefer one token per device: `openshell remote pair <device>`.\nDelete .openshell/data/remote-token to revoke the shared token.")
            );
        }
        None => println!("\n{}", dim("shared token off: pair devices with `openshell remote pair <device>`")),
    }
    Ok(())
}

fn devices(d: &Value) {
    let list = d["devices"].as_array().cloned().unwrap_or_default();
    if list.is_empty() {
        println!("  {} {}", dim("devices: "), dim("none paired"));
        return;
    }
    for dev in list {
        println!(
            "  {} {:<16} {}",
            dim("device:  "),
            dev["name"].as_str().unwrap_or("?"),
            dim(&format!(
                "paired {} · last seen {}",
                dev["paired_at"].as_str().and_then(|s| s.get(..16)).unwrap_or("?"),
                dev["last_seen"].as_str().and_then(|s| s.get(..16)).unwrap_or("never")
            ))
        );
    }
}

fn pair(p: &Value, qr: bool) -> Result<()> {
    let token = p["token"].as_str().unwrap_or_default();
    output::ok(&format!("paired `{}`", p["name"].as_str().unwrap_or("?")));
    println!("  {} {}", dim("token:"), yellow(token));
    println!("  {} {}", dim("mcp:  "), p["mcp_url"].as_str().unwrap_or(""));
    if let Some(fp) = p["tls_fingerprint"].as_str() {
        println!("  {} {}", dim("tls:  "), fp);
    }
    for e in p["exposed"].as_array().into_iter().flatten() {
        let url = e["url"].as_str().unwrap_or("");
        if e["auth"] == "token" {
            println!("  {} {url}/?openshell_token={token}", dim("open: "));
        } else {
            println!("  {} {url}", dim("open: "));
        }
    }
    println!("\n{}", dim("MCP client configuration (streamable HTTP):"));
    println!("{}", mcp_snippet(&p["mcp_url"], token)?);
    let uri = p["pairing_uri"].as_str().unwrap_or_default();
    if qr {
        match qr_text(uri) {
            Some(code) => {
                println!("\n{}", dim("Scan with the OpenShell app on the other device:"));
                println!("{code}");
            }
            None => output::warn("the pairing data is too long for a QR code"),
        }
    }
    println!("{} {}", dim("pairing URI:"), uri);
    println!("\n{}", dim("The token is shown only now. Revoke it with `openshell remote revoke <device>`."));
    Ok(())
}

/// A QR code drawn with Unicode half blocks (two rows per line).
pub fn qr_text(data: &str) -> Option<String> {
    let code = qrcode::QrCode::new(data.as_bytes()).ok()?;
    Some(
        code.render::<qrcode::render::unicode::Dense1x2>()
            .dark_color(qrcode::render::unicode::Dense1x2::Light)
            .light_color(qrcode::render::unicode::Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn qr_code_renders() {
        let q = super::qr_text("openshell://pair?app=x&url=http%3A%2F%2F192.168.1.2%3A7420&token=osd_abc")
            .unwrap();
        assert!(q.lines().count() > 10);
        assert!(q.contains('█') || q.contains('▀') || q.contains('▄'));
    }
}
