//! Built-in web server for production frontend builds: serves static files
//! (with SPA fallback), injects the OpenShell bridge into HTML pages and
//! reverse-proxies configured prefixes (e.g. `/api`) to backend services.

use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::Router;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::oneshot;

#[derive(Clone)]
struct AppState {
    dist: Arc<PathBuf>,
    proxy: Arc<Vec<(String, String)>>,
    injection: Arc<RwLock<Option<String>>>,
    client: reqwest::Client,
}

pub struct StaticServer {
    pub port: u16,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
}

impl StaticServer {
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(());
        let _ = self.task.await;
    }
}

pub async fn start(
    port: u16,
    dist: PathBuf,
    mut proxy: Vec<(String, String)>,
    injection: Arc<RwLock<Option<String>>>,
    client: reqwest::Client,
) -> anyhow::Result<StaticServer> {
    // longest prefix first
    proxy.sort_by_key(|p| std::cmp::Reverse(p.0.len()));
    let state = AppState { dist: Arc::new(dist), proxy: Arc::new(proxy), injection, client };
    let app = Router::new().fallback(handle).with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    Ok(StaticServer { port, shutdown: tx, task })
}

async fn handle(State(state): State<AppState>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    if let Some((prefix, target)) = state
        .proxy
        .iter()
        .find(|(p, _)| path == *p || path.starts_with(&format!("{}/", p.trim_end_matches('/'))))
    {
        let _ = prefix;
        return proxy(&state, target, req).await;
    }
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    serve_file(&state, &path)
}

fn status(code: StatusCode) -> Response {
    Response::builder().status(code).body(Body::from(code.canonical_reason().unwrap_or(""))).unwrap()
}

/// Map a URL path to a file inside `dist`, rejecting traversal.
pub(crate) fn safe_join(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path)?;
    let mut out = root.to_path_buf();
    for comp in Path::new(decoded.trim_start_matches('/')).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn serve_file(state: &AppState, url_path: &str) -> Response {
    let Some(mut file) = safe_join(&state.dist, url_path) else {
        return status(StatusCode::BAD_REQUEST);
    };
    if file.is_dir() {
        file = file.join("index.html");
    }
    if !file.is_file() {
        // SPA fallback for client-side routes (paths without extension)
        let has_ext = Path::new(url_path).extension().is_some();
        let index = state.dist.join("index.html");
        if !has_ext && index.is_file() {
            file = index;
        } else {
            return status(StatusCode::NOT_FOUND);
        }
    }
    let Ok(mut bytes) = std::fs::read(&file) else {
        return status(StatusCode::NOT_FOUND);
    };
    let mime = mime_guess::from_path(&file).first_or_octet_stream();
    let is_html = mime.essence_str() == "text/html";
    if is_html {
        if let Some(snippet) = state.injection.read().unwrap().clone() {
            bytes = inject_html(&String::from_utf8_lossy(&bytes), &snippet).into_bytes();
        }
    }
    let content_type = if mime.type_() == "text" || mime.essence_str() == "application/javascript" {
        format!("{}; charset=utf-8", mime.essence_str())
    } else {
        mime.essence_str().to_string()
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(bytes))
        .unwrap()
}

/// Insert `snippet` right after `<head>` (or at the start of the document).
pub(crate) fn inject_html(html: &str, snippet: &str) -> String {
    let lower = html.to_ascii_lowercase();
    if let Some(pos) = lower.find("<head") {
        if let Some(end) = lower[pos..].find('>') {
            let at = pos + end + 1;
            return format!("{}{}{}", &html[..at], snippet, &html[at..]);
        }
    }
    format!("{snippet}{html}")
}

const HOP_HEADERS: [&str; 7] =
    ["connection", "keep-alive", "proxy-connection", "transfer-encoding", "upgrade", "te", "host"];

async fn proxy(state: &AppState, target: &str, req: Request) -> Response {
    let (parts, body) = req.into_parts();
    let path_and_query =
        parts.uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| "/".into());
    let url = format!("{}{}", target.trim_end_matches('/'), path_and_query);
    let Ok(body) = to_bytes(body, 64 * 1024 * 1024).await else {
        return status(StatusCode::PAYLOAD_TOO_LARGE);
    };
    let mut headers = HeaderMap::new();
    for (k, v) in parts.headers.iter() {
        if !HOP_HEADERS.contains(&k.as_str()) {
            headers.insert(k.clone(), v.clone());
        }
    }
    let resp = state.client.request(parts.method, &url).headers(headers).body(body).send().await;
    match resp {
        Ok(r) => {
            let mut builder = Response::builder().status(r.status());
            for (k, v) in r.headers() {
                if !HOP_HEADERS.contains(&k.as_str()) && k != header::CONTENT_LENGTH {
                    builder = builder.header(k, v);
                }
            }
            let bytes = r.bytes().await.unwrap_or_default();
            builder.body(Body::from(bytes)).unwrap()
        }
        Err(e) => Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .header(header::CONTENT_TYPE, HeaderValue::from_static("text/plain"))
            .body(Body::from(format!("OpenShell proxy: {url}: {e}")))
            .unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_safety() {
        let root = Path::new("/srv/app");
        assert_eq!(safe_join(root, "/a/b.js").unwrap(), root.join("a").join("b.js"));
        assert_eq!(safe_join(root, "/my%20file.txt").unwrap(), root.join("my file.txt"));
        assert!(safe_join(root, "/../etc/passwd").is_none());
        assert!(safe_join(root, "/a/%2e%2e/%2e%2e/x").is_none());
    }

    #[test]
    fn html_injection() {
        assert_eq!(
            inject_html("<html><head><title>x</title></head></html>", "<script></script>"),
            "<html><head><script></script><title>x</title></head></html>"
        );
        assert_eq!(inject_html("<p>hi</p>", "<s>"), "<s><p>hi</p>");
    }
}
