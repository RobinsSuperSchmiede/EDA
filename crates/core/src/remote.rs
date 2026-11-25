//! Remote access gateway (`runtime.remote`).
//!
//! Lets other devices use the app – typically a phone that hosts an app and a laptop or an
//! agent that uses it. The gateway is a second listener next to the loopback control API and
//! deliberately exposes much less:
//!
//! | Route                 | Auth          | Purpose                                         |
//! |-----------------------|---------------|-------------------------------------------------|
//! | `GET  /v1/health`     | public        | liveness (app name, platform)                   |
//! | `GET  /v1/remote/info`| token         | app, platform, the functions it offers          |
//! | `POST /mcp/app`       | token         | the app profile over MCP (the app's functions)  |
//!
//! Security layers, each optional except the token:
//!
//! * **Tokens** – the shared pairing token (`.openshell/data/remote-token`, can be switched
//!   off with `shared_token: false`) and per-device tokens (`openshell remote pair <device>`,
//!   stored hashed, revocable one by one). Calls are audited with the device name.
//! * **Allowlist** – `allow: ["192.168.1.0/24", "100.64.0.0/10"]`; other addresses are dropped
//!   right after `accept`.
//! * **Rate limit and lockout** – requests per minute per address; an address with too many
//!   failed logins is blocked for a while.
//! * **TLS** – a configured certificate or a self-signed one whose fingerprint clients pin.
//! * **Exposed services** – `expose: [{ "service": "api", "port": 8080 }]` makes a service
//!   reachable on its own port (see [`expose`]).
//!
//! Requests with an `Origin` header are rejected on the gateway, so web pages in a browser
//! cannot use it (agents and MCP clients do not send one).

mod access;
mod devices;
mod expose;
mod tls;

pub use access::Refusal;
pub use devices::{sha256_hex, DeviceInfo};

use crate::server::{mcp_with, McpProfile};
use crate::Shell;
use access::{AccessControl, Range};
use anyhow::{Context, Result};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use devices::Devices;
use openshell_agent::audit::{AuditEntry, AuditOutcome};
use openshell_config::{ExposeAuth, ExposeConfig, ExposeProtocol, Permission, Project};
use serde::Serialize;
use serde_json::{json, Value};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;

pub const TOKEN_FILE: &str = "remote-token";
pub use devices::DEVICES_FILE;
pub use expose::COOKIE as EXPOSE_COOKIE;
pub use tls::{CERT_FILE, KEY_FILE};

/// Where the gateway listens and how to pair with it.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteInfo {
    pub bind: String,
    pub port: u16,
    /// URLs other devices can try (the primary network address first).
    pub urls: Vec<String>,
    /// MCP endpoint of the app profile on the first URL.
    pub mcp_url: String,
    /// SHA-256 fingerprint of the TLS certificate (`AB:CD:…`), if TLS is on.
    pub tls_fingerprint: Option<String>,
    /// The certificate is self-signed: clients must pin the fingerprint.
    pub self_signed: bool,
    pub allow: Vec<String>,
    pub shared_token: bool,
    pub exposed: Vec<ExposedInfo>,
    /// Shared pairing token (`Authorization: Bearer <token>`).
    #[serde(skip)]
    pub token: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExposedInfo {
    pub service: String,
    pub port: u16,
    pub protocol: ExposeProtocol,
    pub auth: ExposeAuth,
    pub url: String,
    pub allow: Vec<String>,
}

/// The running gateway: configuration, tokens and per-address state.
pub struct Gateway {
    pub info: RemoteInfo,
    access: AccessControl,
    ranges: Vec<Range>,
    devices: Devices,
    tls: Option<tls::Tls>,
}

/// Listeners bound at launch (so a busy port fails the start, not a background task).
pub(crate) struct Bound {
    gateway: TcpListener,
    exposed: Vec<(ExposeConfig, TcpListener)>,
}

/// Bind the gateway and the exposed ports if `runtime.remote.enabled` is set.
pub(crate) async fn bind(project: &Project) -> Result<Option<(Bound, Arc<Gateway>)>> {
    let cfg = &project.config.runtime.remote;
    if !cfg.enabled {
        return Ok(None);
    }
    let ip: IpAddr =
        cfg.bind.parse().with_context(|| format!("invalid runtime.remote.bind `{}`", cfg.bind))?;
    let listener = TcpListener::bind(SocketAddr::new(ip, cfg.port))
        .await
        .with_context(|| format!("cannot bind the remote gateway on {}:{}", cfg.bind, cfg.port))?;
    let port = listener.local_addr()?.port();
    let mut exposed_listeners = Vec::new();
    for e in &cfg.expose {
        let l = TcpListener::bind(SocketAddr::new(ip, e.port))
            .await
            .with_context(|| format!("cannot expose `{}` on {}:{}", e.service, cfg.bind, e.port))?;
        exposed_listeners.push((e.clone(), l));
    }
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
    let tls = if cfg.tls.enabled { Some(tls::load(project, &hosts)?) } else { None };
    let scheme = if tls.is_some() { "https" } else { "http" };
    let url_for = |h: &str, port: u16, scheme: &str| {
        if h.contains(':') {
            format!("{scheme}://[{h}]:{port}")
        } else {
            format!("{scheme}://{h}:{port}")
        }
    };
    let urls: Vec<String> = hosts.iter().map(|h| url_for(h, port, scheme)).collect();
    let mcp_url = format!("{}/mcp/app", urls[0]);
    let exposed = cfg
        .expose
        .iter()
        .map(|e| {
            let scheme = match e.protocol {
                ExposeProtocol::Tcp => "tcp",
                ExposeProtocol::Http => scheme,
            };
            ExposedInfo {
                service: e.service.clone(),
                port: e.port,
                protocol: e.protocol,
                auth: e.effective_auth(),
                url: url_for(&hosts[0], e.port, scheme),
                allow: e.allow.clone().unwrap_or_else(|| cfg.allow.clone()),
            }
        })
        .collect();
    let info = RemoteInfo {
        bind: cfg.bind.clone(),
        port,
        urls,
        mcp_url,
        tls_fingerprint: tls.as_ref().map(|t| t.fingerprint.clone()),
        self_signed: tls.as_ref().is_some_and(|t| t.self_signed),
        allow: cfg.allow.clone(),
        shared_token: cfg.shared_token,
        exposed,
        token,
    };
    let gateway = Gateway {
        info,
        access: AccessControl::new(cfg.rate_limit.clone()),
        ranges: access::ranges(&cfg.allow),
        devices: Devices::load(&project.data_dir()),
        tls,
    };
    Ok(Some((Bound { gateway: listener, exposed: exposed_listeners }, Arc::new(gateway))))
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
pub(crate) fn same_token(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Gateway {
    /// Who a token belongs to: `paired-device` for the shared token, else the device name.
    pub fn authenticate(&self, token: &str) -> Option<String> {
        self.identify(token).map(|(who, _)| who)
    }

    /// Who a token belongs to and the device's scopes (empty: what the app grants agents).
    pub fn identify(&self, token: &str) -> Option<(String, Vec<Permission>)> {
        if self.info.shared_token && !token.is_empty() && same_token(token, &self.info.token) {
            return Some(("paired-device".into(), vec![]));
        }
        self.devices.verify(token).map(|(d, scopes)| (format!("device:{d}"), scopes))
    }

    /// Pair a device and return its token (shown once).
    pub fn pair(&self, name: &str, scopes: Vec<Permission>) -> Result<String> {
        self.devices.pair(name, scopes)
    }
    pub fn revoke(&self, name: &str) -> Result<bool> {
        self.devices.revoke(name)
    }
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.devices.list()
    }
    pub fn blocked(&self) -> Vec<IpAddr> {
        self.access.blocked()
    }
    pub fn unblock(&self, ip: IpAddr) -> bool {
        self.access.unblock(ip)
    }

    /// Text for a pairing QR code: everything a client needs in one line.
    pub fn pairing_uri(&self, app: &str, token: &str) -> String {
        let enc = |s: &str| -> String {
            s.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect()
        };
        let mut uri =
            format!("openshell://pair?app={}&url={}&token={}", enc(app), enc(&self.info.urls[0]), enc(token));
        if let Some(fp) = &self.info.tls_fingerprint {
            uri.push_str(&format!("&fp={}", enc(fp)));
        }
        uri
    }

    fn audit(&self, shell: &Shell, peer: SocketAddr, who: &str, tool: &str, ok: bool, msg: &str) {
        shell.inner.audit.record(AuditEntry {
            ts: chrono::Utc::now(),
            agent: who.to_string(),
            channel: "remote".into(),
            tool: tool.to_string(),
            permission: Permission::Read,
            args: json!({ "address": peer.ip().to_string() }),
            outcome: if ok { AuditOutcome::Ok } else { AuditOutcome::Denied },
            message: Some(msg.to_string()),
            approved_by_human: false,
            duration_ms: 0,
        });
    }

    fn login_failed(&self, shell: &Shell, peer: SocketAddr, what: &str) {
        let blocked = self.access.failed_login(peer.ip());
        self.audit(shell, peer, "unknown", what, false, "missing or invalid token");
        if blocked {
            tracing::warn!("remote gateway: blocked {} after too many failed logins", peer.ip());
            self.audit(
                shell,
                peer,
                "unknown",
                "remote_block",
                false,
                "too many failed logins: address blocked",
            );
        }
    }

    fn refused(&self, _shell: &Shell, peer: SocketAddr, r: Refusal, what: &str) {
        // not audited: refused connections can be a flood; the block itself is audited
        tracing::debug!("remote gateway: refused {} ({what}): {}", peer.ip(), r.message());
    }
}

#[derive(Clone, Copy)]
struct Peer(SocketAddr);

pub(crate) fn spawn(shell: Shell, gw: Arc<Gateway>, bound: Bound) {
    for (e, l) in bound.exposed {
        tracing::info!("exposing `{}` on port {}", e.service, e.port);
        expose::spawn(shell.clone(), gw.clone(), e, l);
    }
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/v1/remote/info", get(info))
        .route("/mcp/app", post(mcp).get(not_allowed).delete(not_allowed))
        .layer(from_fn_with_state(shell.clone(), guard))
        .with_state(shell.clone());
    let listener = bound.gateway;
    tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("remote gateway: accept failed: {e}");
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    continue;
                }
            };
            if !access::allowed(&gw.ranges, peer.ip()) {
                gw.refused(&shell, peer, Refusal::NotAllowed, "gateway");
                continue;
            }
            let app = app.clone().layer(Extension(Peer(peer)));
            let tls = gw.tls.clone();
            tokio::spawn(async move {
                let _ = stream.set_nodelay(true);
                match tls {
                    Some(t) => {
                        if let Ok(s) = t.acceptor.accept(stream).await {
                            serve_http(s, app).await;
                        }
                    }
                    None => serve_http(stream, app).await,
                }
            });
        }
    });
}

async fn serve_http<S>(io: S, app: Router)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let svc = hyper_util::service::TowerToHyperService::new(app);
    let _ = hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
        .serve_connection(hyper_util::rt::TokioIo::new(io), svc)
        .await;
}

fn deny(status: StatusCode, msg: &str) -> Response {
    (status, Json(json!({ "error": msg }))).into_response()
}

async fn guard(
    State(shell): State<Shell>,
    Extension(Peer(peer)): Extension<Peer>,
    mut req: Request,
    next: Next,
) -> Response {
    let Some(gw) = shell.inner.remote.clone() else {
        return deny(StatusCode::SERVICE_UNAVAILABLE, "the remote gateway is off");
    };
    if let Err(r) = gw.access.admit(&gw.ranges, peer.ip()) {
        gw.refused(&shell, peer, r, "gateway");
        let status = match r {
            Refusal::NotAllowed => StatusCode::FORBIDDEN,
            _ => StatusCode::TOO_MANY_REQUESTS,
        };
        return deny(status, r.message());
    }
    if req.headers().contains_key(header::ORIGIN) {
        return deny(StatusCode::FORBIDDEN, "browser requests are not allowed on the remote gateway");
    }
    if req.uri().path() != "/v1/health" {
        let given = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .unwrap_or("")
            .trim()
            .to_string();
        match gw.identify(&given) {
            Some((who, scopes)) => {
                req.extensions_mut().insert(DeviceScopes(scopes));
                gw.access.logged_in(peer.ip());
                // the identity in the audit log is the device, not what the client claims
                let agent = match req.headers().get("x-openshell-agent").and_then(|h| h.to_str().ok()) {
                    Some(a) if who.starts_with("device:") => format!("{who} ({a})"),
                    Some(a) => a.to_string(),
                    None => who,
                };
                if let Ok(v) = HeaderValue::from_str(&agent) {
                    req.headers_mut().insert("x-openshell-agent", v);
                }
            }
            None => {
                gw.login_failed(&shell, peer, "gateway");
                return deny(StatusCode::UNAUTHORIZED, "missing or invalid pairing token");
            }
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
    let exposed = shell.inner.remote.as_ref().map(|g| g.info.exposed.clone()).unwrap_or_default();
    Json(json!({
        "app": { "name": cfg.app.name, "version": cfg.app.version, "description": cfg.app.description },
        "platform": shell.inner.runtime.capabilities(),
        "functions": shell.capabilities_summary(),
        "exposed": exposed,
        "mcp": "/mcp/app",
    }))
}

/// Scopes of the paired device that made a request (empty: what the app grants agents).
#[derive(Clone)]
struct DeviceScopes(Vec<Permission>);

async fn mcp(
    State(shell): State<Shell>,
    Extension(DeviceScopes(scopes)): Extension<DeviceScopes>,
    headers: HeaderMap,
    Json(msg): Json<Value>,
) -> Response {
    if !shell.inner.project.config.agents.enabled {
        return deny(StatusCode::FORBIDDEN, crate::server::AGENTS_OFF);
    }
    let policy = shell.inner.agent_policy.restricted(&scopes);
    mcp_with(shell, headers, msg, McpProfile::Remote, policy, None).await
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
