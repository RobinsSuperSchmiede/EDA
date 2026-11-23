//! Remote access gateway (`runtime.remote`).
//!
//! Lets other devices use the app – typically a phone that hosts an app and a laptop or an
//! agent that uses it. The gateway is a second listener next to the loopback control API and
//! deliberately exposes much less:
//!
//! | Route                 | Auth          | Purpose                                         |
//! |-----------------------|---------------|-------------------------------------------------|
//! | `GET  /v1/health`     | public        | liveness (app name, platform)                   |
//! | `GET  /v1/remote/info`| pairing token | app, platform, the functions it offers          |
//! | `POST /mcp/app`       | pairing token | the app profile over MCP (the app's functions)  |
//!
//! Requests with an `Origin` header are rejected, so web pages in a browser cannot use the
//! gateway (agents and MCP clients do not send one). The pairing token is stored in the app's
//! data directory and survives restarts; delete the file to revoke all paired devices.
//! Traffic is plain HTTP: use it in a trusted network or over a VPN such as Tailscale.

use crate::server::{mcp_with, McpProfile};
use crate::Shell;
use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use openshell_config::Project;
use serde::Serialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;

pub const TOKEN_FILE: &str = "remote-token";

/// Where the gateway listens and how to pair with it.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteInfo {
    pub bind: String,
    pub port: u16,
    /// URLs other devices can try (the primary network address first).
    pub urls: Vec<String>,
    /// MCP endpoint of the app profile on the first URL.
    pub mcp_url: String,
    /// Pairing token (`Authorization: Bearer <token>`).
    #[serde(skip)]
    pub token: String,
}

/// Bind the gateway if `runtime.remote.enabled` is set.
pub(crate) async fn bind(project: &Project) -> Result<Option<(tokio::net::TcpListener, RemoteInfo)>> {
    let cfg = &project.config.runtime.remote;
    if !cfg.enabled {
        return Ok(None);
    }
    let ip: IpAddr =
        cfg.bind.parse().with_context(|| format!("invalid runtime.remote.bind `{}`", cfg.bind))?;
    let listener = tokio::net::TcpListener::bind(SocketAddr::new(ip, cfg.port))
        .await
        .with_context(|| format!("cannot bind the remote gateway on {}:{}", cfg.bind, cfg.port))?;
    let port = listener.local_addr()?.port();
    let token = load_or_create_token(&project.data_dir())?;
    let mut hosts = Vec::new();
    if ip.is_unspecified() {
        if let Some(primary) = primary_address() {
            hosts.push(primary.to_string());
        }
        hosts.push("127.0.0.1".into());
    } else {
        hosts.push(ip.to_string());
    }
    let urls: Vec<String> = hosts
        .iter()
        .map(|h| if h.contains(':') { format!("http://[{h}]:{port}") } else { format!("http://{h}:{port}") })
        .collect();
    let mcp_url = format!("{}/mcp/app", urls[0]);
    Ok(Some((listener, RemoteInfo { bind: cfg.bind.clone(), port, urls, mcp_url, token })))
}

/// The address used for outgoing traffic – the one other devices in the network can reach.
/// Connecting a UDP socket sends no packets; it only selects a route.
fn primary_address() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:9").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    (!ip.is_unspecified() && !ip.is_loopback()).then_some(ip)
}

fn load_or_create_token(data_dir: &Path) -> Result<String> {
    let path = data_dir.join(TOKEN_FILE);
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    std::fs::create_dir_all(data_dir)?;
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    std::fs::write(&path, &token).with_context(|| format!("cannot write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(token)
}

/// Compare without leaking the position of the first difference through timing.
fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub(crate) fn spawn(shell: Shell, listener: tokio::net::TcpListener) {
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/remote/info", get(info))
        .route("/mcp/app", post(mcp).get(not_allowed).delete(not_allowed))
        .layer(from_fn_with_state(shell.clone(), guard))
        .with_state(shell);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("remote gateway stopped: {e}");
        }
    });
}

fn deny(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

async fn guard(State(shell): State<Shell>, req: Request, next: Next) -> Response {
    if req.headers().contains_key(header::ORIGIN) {
        return deny(StatusCode::FORBIDDEN, "browser requests are not allowed on the remote gateway");
    }
    if req.uri().path() != "/v1/health" {
        let expected = shell.inner.remote.as_ref().map(|r| r.token.as_str()).unwrap_or("");
        let given = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or("");
        if expected.is_empty() || !same_token(given, expected) {
            return deny(StatusCode::UNAUTHORIZED, "missing or invalid pairing token");
        }
    }
    next.run(req).await
}

async fn health(State(shell): State<Shell>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "app": shell.inner.project.config.app.name,
        "platform": shell.inner.runtime.platform(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn info(State(shell): State<Shell>) -> Json<Value> {
    let cfg = &shell.inner.project.config;
    Json(json!({
        "app": { "name": cfg.app.name, "version": cfg.app.version, "description": cfg.app.description },
        "platform": shell.inner.runtime.capabilities(),
        "functions": shell.capabilities_summary(),
        "mcp": "/mcp/app",
    }))
}

async fn mcp(State(shell): State<Shell>, headers: HeaderMap, Json(msg): Json<Value>) -> Response {
    mcp_with(shell, headers, msg, McpProfile::Remote).await
}

async fn not_allowed() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison() {
        assert!(same_token("abc", "abc"));
        assert!(!same_token("abc", "abd"));
        assert!(!same_token("abc", "abcd"));
        assert!(!same_token("", "a"));
    }

    #[test]
    fn token_is_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let a = load_or_create_token(dir.path()).unwrap();
        let b = load_or_create_token(dir.path()).unwrap();
        assert_eq!(a, b);
        assert!(a.len() >= 32);
    }
}
