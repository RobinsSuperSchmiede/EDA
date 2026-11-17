//! The local control API (binds to 127.0.0.1 only).
//!
//! | Route                               | Who            | Purpose                               |
//! |-------------------------------------|----------------|---------------------------------------|
//! | `GET  /v1/health`                   | public         | liveness                              |
//! | `GET  /sdk.js`, `/bridge.js`        | public         | frontend SDK / UI bridge scripts      |
//! | `GET  /v1/status`, `/v1/info`       | admin          | runtime status                        |
//! | `POST /v1/services/{name}/{action}` | admin          | start / stop / restart                |
//! | `GET  /v1/logs`                     | admin          | log query                             |
//! | `POST /v1/shutdown`                 | admin          | stop the shell                        |
//! | `GET  /v1/approvals`, `POST /v1/approvals/{id}/{approve,deny}` | admin | approvals |
//! | `GET  /v1/audit`                    | admin          | audit log                             |
//! | `GET  /v1/tools`, `POST /v1/tools/{name}` | admin (agent policy) | agent tools (plain HTTP) |
//! | `POST /mcp`                         | admin (agent policy) | MCP Streamable HTTP              |
//! | `GET  /v1/ui/bridge`                | app / admin    | UI bridge WebSocket                   |
//! | `GET  /v1/sdk/info`, `POST /v1/sdk/call/{tool}` | app (SDK policy) | frontend SDK           |
//!
//! Protection: loopback only, `Host` header check (DNS rebinding), `Origin`
//! allow-list (CORS / CSRF) and two bearer tokens (admin, app).

use crate::exec::{AgentTools, AppTools};
use crate::Shell;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{from_fn_with_state, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures::{SinkExt, StreamExt};
use openshell_agent::mcp::{handle_message, CallContext};
use openshell_agent::policy::Decision;
use openshell_runtime::LogQuery;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const SDK_JS: &str = include_str!("../../../packages/sdk/src/index.js");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Admin,
    App,
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn bad(e: impl ToString) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}

type ApiResult = Result<Json<Value>, ApiError>;

pub(crate) fn spawn(shell: Shell, listener: tokio::net::TcpListener) {
    let app = Router::new()
        .route("/v1/health", get(health))
        .route("/sdk.js", get(sdk_js))
        .route("/bridge.js", get(bridge_js))
        .route("/v1/info", get(status))
        .route("/v1/status", get(status))
        .route("/v1/services/{name}/{action}", post(service_action))
        .route("/v1/logs", get(logs))
        .route("/v1/shutdown", post(shutdown))
        .route("/v1/approvals", get(approvals))
        .route("/v1/approvals/{id}/{decision}", post(approval_decide))
        .route("/v1/audit", get(audit))
        .route("/v1/tools", get(tools))
        .route("/v1/tools/{name}", post(tool_call))
        .route("/mcp", post(mcp).get(mcp_get).delete(mcp_get))
        .route("/mcp/app", post(mcp_app).get(mcp_get).delete(mcp_get))
        .route("/v1/ui/bridge", get(bridge_ws))
        .route("/v1/sdk/info", get(sdk_info))
        .route("/v1/sdk/call/{tool}", post(sdk_call))
        .layer(from_fn_with_state(shell.clone(), guard))
        .with_state(shell);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("control API stopped: {e}");
        }
    });
}

fn query_token(req: &Request) -> Option<String> {
    req.uri().query()?.split('&').find_map(|kv| kv.strip_prefix("token=").map(str::to_string))
}

fn cors(resp: &mut Response, origin: &str) {
    let h = resp.headers_mut();
    if let Ok(v) = HeaderValue::from_str(origin) {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, v);
    }
    h.insert(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "authorization, content-type, x-openshell-agent, mcp-session-id, mcp-protocol-version",
        ),
    );
    h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, OPTIONS"));
}

async fn guard(State(shell): State<Shell>, mut req: Request, next: Next) -> Response {
    let port = shell.inner.control_url.rsplit(':').next().unwrap_or("").to_string();
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_ok = [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")]
        .iter()
        .any(|h| h == host);
    if !host_ok {
        return ApiError(StatusCode::FORBIDDEN, "invalid Host header".into()).into_response();
    }
    let origin = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()).map(str::to_string);
    if let Some(o) = &origin {
        if !shell.inner.allowed_origins.iter().any(|a| a == o) {
            return ApiError(StatusCode::FORBIDDEN, format!("origin {o} is not allowed")).into_response();
        }
    }
    if req.method() == Method::OPTIONS {
        let mut resp = StatusCode::NO_CONTENT.into_response();
        if let Some(o) = &origin {
            cors(&mut resp, o);
        }
        return resp;
    }
    let path = req.uri().path().to_string();
    let public = matches!(path.as_str(), "/v1/health" | "/sdk.js" | "/bridge.js");
    if !public {
        let token = req
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            .map(str::to_string)
            .or_else(|| query_token(&req));
        let role = match token {
            Some(t) if t == shell.inner.admin_token => Role::Admin,
            Some(t) if t == shell.inner.app_token => Role::App,
            _ => {
                return ApiError(StatusCode::UNAUTHORIZED, "missing or invalid token".into()).into_response()
            }
        };
        if role == Role::App && !(path.starts_with("/v1/sdk/") || path == "/v1/ui/bridge") {
            return ApiError(StatusCode::FORBIDDEN, "not allowed with the app token".into()).into_response();
        }
        req.extensions_mut().insert(role);
    }
    let mut resp = next.run(req).await;
    if let Some(o) = &origin {
        cors(&mut resp, o);
    }
    resp
}

fn agent_name(headers: &HeaderMap, default: &str) -> String {
    headers
        .get("x-openshell-agent")
        .and_then(|h| h.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(64).collect())
        .unwrap_or_else(|| default.to_string())
}

async fn health(State(shell): State<Shell>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "app": shell.inner.project.config.app.name,
        "mode": shell.inner.mode,
        "host": shell.inner.host.kind(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn sdk_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], SDK_JS)
}

async fn bridge_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/javascript; charset=utf-8")], openshell_agent::BRIDGE_JS)
}

async fn status(State(shell): State<Shell>) -> ApiResult {
    let i = &shell.inner;
    Ok(Json(json!({
        "app": i.project.config.app.name,
        "version": i.project.config.app.version,
        "mode": i.mode,
        "host": i.host.kind(),
        "pid": std::process::id(),
        "project_root": i.project.root,
        "control_url": i.control_url,
        "frontend_url": i.runtime.frontend_url(),
        "services": i.runtime.status(),
        "ui_pages": i.hub.pages(),
        "pending_approvals": i.approvals.pending(),
        "agent_permissions": i.agent_policy,
    })))
}

async fn service_action(
    State(shell): State<Shell>,
    Path((name, action)): Path<(String, String)>,
) -> ApiResult {
    let rt = &shell.inner.runtime;
    match action.as_str() {
        "start" => rt.start(&name),
        "stop" => rt.stop(&name),
        "restart" => rt.restart(&name),
        other => return Err(bad(format!("unknown action `{other}`"))),
    }
    .map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(json!({ "ok": true, "service": name, "action": action })))
}

#[derive(Deserialize)]
struct LogParams {
    service: Option<String>,
    since: Option<u64>,
    limit: Option<usize>,
    contains: Option<String>,
    #[serde(default)]
    errors_only: bool,
}

async fn logs(State(shell): State<Shell>, Query(p): Query<LogParams>) -> ApiResult {
    let rt = &shell.inner.runtime;
    if let Some(s) = &p.service {
        if !rt.has_service(s) {
            return Err(ApiError(StatusCode::NOT_FOUND, format!("unknown service `{s}`")));
        }
    }
    let lines = rt.logs().query(&LogQuery {
        service: p.service,
        since: p.since,
        limit: Some(p.limit.unwrap_or(200).min(10_000)),
        contains: p.contains,
        errors_only: p.errors_only,
    });
    Ok(Json(json!({ "lines": lines, "last_seq": rt.logs().last_seq() })))
}

async fn shutdown(State(shell): State<Shell>) -> ApiResult {
    let host = shell.inner.host.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        host.shutdown_requested();
    });
    Ok(Json(json!({ "ok": true })))
}

async fn approvals(State(shell): State<Shell>) -> ApiResult {
    Ok(Json(json!({ "pending": shell.inner.approvals.pending() })))
}

async fn approval_decide(
    State(shell): State<Shell>,
    Path((id, decision)): Path<(String, String)>,
) -> ApiResult {
    let approved = match decision.as_str() {
        "approve" => true,
        "deny" => false,
        other => return Err(bad(format!("unknown decision `{other}` (approve|deny)"))),
    };
    if shell.inner.approvals.resolve(&id, approved) {
        Ok(Json(json!({ "ok": true, "id": id, "approved": approved })))
    } else {
        Err(ApiError(StatusCode::NOT_FOUND, format!("no pending approval `{id}`")))
    }
}

#[derive(Deserialize)]
struct LimitParams {
    limit: Option<usize>,
}

async fn audit(State(shell): State<Shell>, Query(p): Query<LimitParams>) -> ApiResult {
    Ok(Json(json!({ "entries": shell.inner.audit.recent(p.limit.unwrap_or(50)) })))
}

async fn tools(State(shell): State<Shell>) -> ApiResult {
    let policy = &shell.inner.agent_policy;
    let tools: Vec<Value> = shell
        .tools()
        .into_iter()
        .map(|t| {
            let access = match policy.check(t.permission) {
                Decision::Allow => "allowed",
                Decision::NeedsApproval => "approval",
                Decision::Deny => "denied",
            };
            json!({
                "name": t.name,
                "description": t.description,
                "permission": t.permission,
                "access": access,
                "input_schema": t.input_schema,
            })
        })
        .collect();
    Ok(Json(json!({ "tools": tools })))
}

async fn tool_call(
    State(shell): State<Shell>,
    Path(name): Path<String>,
    headers: HeaderMap,
    body: Option<Json<Value>>,
) -> ApiResult {
    let args = body.map(|b| b.0).unwrap_or(json!({}));
    let ctx = CallContext { agent: agent_name(&headers, "http-agent"), channel: "http".into() };
    let policy = shell.inner.agent_policy.clone();
    let outcome = shell.call_tool(&name, args, ctx, &policy).await;
    Ok(Json(json!(outcome)))
}

fn mcp_sessions() -> &'static Mutex<HashMap<String, String>> {
    static S: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

async fn mcp(State(shell): State<Shell>, headers: HeaderMap, Json(msg): Json<Value>) -> Response {
    mcp_with(shell, headers, msg, false).await
}

async fn mcp_app(State(shell): State<Shell>, headers: HeaderMap, Json(msg): Json<Value>) -> Response {
    mcp_with(shell, headers, msg, true).await
}

async fn mcp_with(shell: Shell, headers: HeaderMap, msg: Value, app_profile: bool) -> Response {
    let session = headers.get("mcp-session-id").and_then(|h| h.to_str().ok()).map(str::to_string);
    let is_init = msg.get("method").and_then(|m| m.as_str()) == Some("initialize");
    let client_name = msg.pointer("/params/clientInfo/name").and_then(|n| n.as_str()).map(str::to_string);
    let agent = headers
        .get("x-openshell-agent")
        .and_then(|h| h.to_str().ok())
        .map(str::to_string)
        .or_else(|| client_name.clone())
        .or_else(|| session.as_ref().and_then(|s| mcp_sessions().lock().unwrap().get(s).cloned()))
        .unwrap_or_else(|| "mcp-client".into());
    let ctx = CallContext {
        agent: agent.clone(),
        channel: if app_profile { "mcp-app".into() } else { "mcp".into() },
    };
    let reply = if app_profile {
        handle_message(&AppTools(shell), msg, ctx).await
    } else {
        handle_message(&AgentTools(shell), msg, ctx).await
    };
    let mut resp = match reply {
        Some(v) => Json(v).into_response(),
        None => StatusCode::ACCEPTED.into_response(),
    };
    if is_init {
        let id = uuid::Uuid::new_v4().simple().to_string();
        mcp_sessions().lock().unwrap().insert(id.clone(), agent);
        if let Ok(v) = HeaderValue::from_str(&id) {
            resp.headers_mut().insert("mcp-session-id", v);
        }
    }
    resp
}

async fn mcp_get() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

async fn bridge_ws(State(shell): State<Shell>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| bridge_session(shell, socket))
}

async fn bridge_session(shell: Shell, socket: WebSocket) {
    let (id, mut outgoing) = shell.inner.hub.connect();
    tracing::debug!("UI bridge connected: {id}");
    let (mut tx, mut rx) = socket.split();
    loop {
        tokio::select! {
            msg = outgoing.recv() => match msg {
                Some(text) => {
                    if tx.send(Message::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                None => break,
            },
            incoming = rx.next() => match incoming {
                Some(Ok(Message::Text(t))) => shell.inner.hub.on_message(&id, t.as_str()),
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            }
        }
    }
    shell.inner.hub.disconnect(&id);
    tracing::debug!("UI bridge disconnected: {id}");
}

async fn sdk_info(State(shell): State<Shell>, Extension(_role): Extension<Role>) -> ApiResult {
    let i = &shell.inner;
    Ok(Json(json!({
        "app": i.project.config.app.name,
        "version": i.project.config.app.version,
        "mode": i.mode,
        "host": i.host.kind(),
        "permissions": i.sdk_policy.granted,
        "services": i.runtime.service_names(),
    })))
}

async fn sdk_call(
    State(shell): State<Shell>,
    Path(tool): Path<String>,
    Extension(role): Extension<Role>,
    body: Option<Json<Value>>,
) -> ApiResult {
    let args = body.map(|b| b.0).unwrap_or(json!({}));
    let ctx = CallContext { agent: "frontend".into(), channel: "sdk".into() };
    let policy =
        if role == Role::Admin { shell.inner.agent_policy.clone() } else { shell.inner.sdk_policy.clone() };
    let outcome = shell.call_tool(&tool, args, ctx, &policy).await;
    Ok(Json(json!(outcome)))
}
