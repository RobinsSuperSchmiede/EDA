//! Exposed services (`runtime.remote.expose`): a service's port made reachable from other
//! devices on a port of its own.
//!
//! * `tcp` – bytes are forwarded unchanged (databases, MQTT, game servers …). Protected by the
//!   address allowlist and the rate limit for new connections.
//! * `http` – websites and APIs. With `auth: token` the first request of every connection must
//!   carry a pairing or device token (`Authorization: Bearer`, the cookie `openshell_token`, or
//!   once `?openshell_token=…`, which sets the cookie and redirects). After that the connection
//!   is forwarded unchanged, so WebSockets, streaming and uploads work. TLS from
//!   `runtime.remote.tls` applies.

use super::{access, Gateway};
use crate::Shell;
use openshell_config::{ExposeAuth, ExposeConfig, ExposeProtocol, FRONTEND_SERVICE};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub const COOKIE: &str = "openshell_token";
const QUERY: &str = "openshell_token=";
const MAX_HEAD: usize = 32 * 1024;

pub(super) fn spawn(shell: Shell, gw: Arc<Gateway>, e: ExposeConfig, listener: TcpListener) {
    let ranges = access::ranges(e.allow.as_ref().unwrap_or(&shell.inner.project.config.runtime.remote.allow));
    tokio::spawn(async move {
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!("exposed port: accept failed: {e}");
                    continue;
                }
            };
            if let Err(r) = gw.access.admit(&ranges, peer.ip()) {
                gw.refused(&shell, peer, r, &format!("expose:{}", e.service));
                continue;
            }
            let (shell, gw, e) = (shell.clone(), gw.clone(), e.clone());
            tokio::spawn(async move {
                let _ = stream.set_nodelay(true);
                match (e.protocol, gw.tls.clone()) {
                    (ExposeProtocol::Http, Some(tls)) => {
                        if let Ok(s) = tls.acceptor.accept(stream).await {
                            connection(&shell, &gw, &e, peer, s, true).await;
                        }
                    }
                    _ => connection(&shell, &gw, &e, peer, stream, false).await,
                }
            });
        }
    });
}

/// `127.0.0.1:port` of the service right now (ports may be dynamic).
fn target(shell: &Shell, service: &str) -> Option<String> {
    let url = if service == FRONTEND_SERVICE {
        shell.inner.runtime.frontend_url()
    } else {
        shell.inner.runtime.service_url(service)
    }?;
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(&url);
    let host = rest.split(['/', '?', '#']).next()?;
    if host.contains(':') {
        Some(host.replace("localhost:", "127.0.0.1:"))
    } else {
        Some(format!("{host}:{}", if url.starts_with("https") { 443 } else { 80 }))
    }
}

async fn connection<S>(
    shell: &Shell,
    gw: &Gateway,
    e: &ExposeConfig,
    peer: SocketAddr,
    mut client: S,
    tls: bool,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let label = format!("expose:{}", e.service);
    let mut head = Vec::new();
    let mut identity = None;
    if e.protocol == ExposeProtocol::Http {
        head = match read_head(&mut client).await {
            Some(h) => h,
            None => return,
        };
        if e.effective_auth() == ExposeAuth::Token {
            let req = parse_head(&head);
            let (from_query, token) = match req.as_ref().and_then(|r| r.token.clone()) {
                Some(t) => t,
                None => {
                    let _ = client.write_all(&unauthorized(false)).await;
                    return;
                }
            };
            match gw.authenticate(&token) {
                Some(who) => {
                    gw.access.logged_in(peer.ip());
                    if from_query {
                        // keep the token out of the address bar and the backend: set the cookie and
                        // load the page again without it
                        let location = req.map(|r| r.path_without_token).unwrap_or_else(|| "/".into());
                        let _ = client.write_all(&login_redirect(&location, &token, tls)).await;
                        gw.audit(shell, peer, &who, &label, true, "browser login");
                        return;
                    }
                    identity = Some(who);
                }
                None => {
                    gw.login_failed(shell, peer, &label);
                    let _ = client.write_all(&unauthorized(true)).await;
                    return;
                }
            }
        }
    }
    let Some(addr) = target(shell, &e.service) else {
        let _ = client.write_all(&bad_gateway(&e.service)).await;
        return;
    };
    let mut upstream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(_) => {
            if e.protocol == ExposeProtocol::Http {
                let _ = client.write_all(&bad_gateway(&e.service)).await;
            }
            return;
        }
    };
    let _ = upstream.set_nodelay(true);
    gw.audit(shell, peer, identity.as_deref().unwrap_or("anonymous"), &label, true, "connected");
    if !head.is_empty() && upstream.write_all(&head).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
}

/// Read until the end of the request head (`\r\n\r\n`). Bytes after it (the start of a body)
/// stay in the buffer and are forwarded with the head.
async fn read_head<S: AsyncRead + Unpin>(s: &mut S) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let read = async {
        loop {
            let n = s.read(&mut chunk).await.ok()?;
            if n == 0 {
                return None;
            }
            buf.extend_from_slice(&chunk[..n]);
            if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                return Some(());
            }
            if buf.len() > MAX_HEAD {
                return None;
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(15), read).await.ok()??;
    Some(buf)
}

#[derive(Debug, PartialEq)]
struct Head {
    /// `(from_query, token)`
    token: Option<(bool, String)>,
    path_without_token: String,
}

fn parse_head(head: &[u8]) -> Option<Head> {
    let end = head.windows(4).position(|w| w == b"\r\n\r\n")?;
    let text = std::str::from_utf8(&head[..end]).ok()?;
    let mut lines = text.split("\r\n");
    let target = lines.next()?.split(' ').nth(1)?.to_string();
    let mut token = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else { continue };
        let value = value.trim();
        if name.eq_ignore_ascii_case("authorization") {
            if let Some(t) = value.strip_prefix("Bearer ") {
                token = Some((false, t.trim().to_string()));
            }
        } else if name.eq_ignore_ascii_case("cookie") && token.is_none() {
            token = value
                .split(';')
                .filter_map(|c| c.trim().split_once('='))
                .find(|(k, _)| *k == COOKIE)
                .map(|(_, v)| (false, v.to_string()));
        }
    }
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), Some(q.to_string())),
        None => (target.clone(), None),
    };
    let mut rest = Vec::new();
    if let Some(q) = query {
        for kv in q.split('&') {
            match kv.strip_prefix(QUERY) {
                Some(t) if !t.is_empty() => token = Some((true, t.to_string())),
                _ => rest.push(kv.to_string()),
            }
        }
    }
    let path_without_token = if rest.is_empty() { path } else { format!("{path}?{}", rest.join("&")) };
    Some(Head { token, path_without_token })
}

fn response(status: &str, extra_headers: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n{extra_headers}\r\n{body}",
        body.len()
    )
    .into_bytes()
}

fn unauthorized(wrong: bool) -> Vec<u8> {
    let why =
        if wrong { "The token is not valid (revoked or mistyped)." } else { "This service needs a token." };
    response(
        "401 Unauthorized",
        "WWW-Authenticate: Bearer realm=\"openshell\"\r\n",
        &format!(
            "<!doctype html><meta charset=utf-8><title>OpenShell</title><body style=\"font-family:system-ui;max-width:34rem;margin:4rem auto\">\
             <h1>Sign in</h1><p>{why}</p><p>Open this address once with <code>?openshell_token=&lt;token&gt;</code> \
             (from <code>openshell remote pair &lt;device&gt;</code>) or send <code>Authorization: Bearer &lt;token&gt;</code>.</p></body>"
        ),
    )
}

fn login_redirect(location: &str, token: &str, secure: bool) -> Vec<u8> {
    let location = if location.is_empty() { "/" } else { location };
    let secure = if secure { "; Secure" } else { "" };
    response(
        "302 Found",
        &format!(
            "Location: {location}\r\nSet-Cookie: {COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000{secure}\r\n"
        ),
        "",
    )
}

fn bad_gateway(service: &str) -> Vec<u8> {
    response("502 Bad Gateway", "", &format!("<!doctype html><p>`{service}` is not running.</p>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_from_header_cookie_or_query() {
        let h = parse_head(b"GET /x HTTP/1.1\r\nHost: a\r\nAuthorization: Bearer abc\r\n\r\n").unwrap();
        assert_eq!(h.token, Some((false, "abc".into())));
        let h = parse_head(b"GET /x HTTP/1.1\r\nCookie: a=1; openshell_token=tok; b=2\r\n\r\n").unwrap();
        assert_eq!(h.token, Some((false, "tok".into())));
        let h = parse_head(b"GET /app?x=1&openshell_token=q&y=2 HTTP/1.1\r\nHost: a\r\n\r\nbody").unwrap();
        assert_eq!(h.token, Some((true, "q".into())));
        assert_eq!(h.path_without_token, "/app?x=1&y=2");
        let h = parse_head(b"GET /?openshell_token=q HTTP/1.1\r\n\r\n").unwrap();
        assert_eq!(h.path_without_token, "/");
        let h = parse_head(b"GET / HTTP/1.1\r\nHost: a\r\n\r\n").unwrap();
        assert_eq!(h.token, None);
        assert!(parse_head(b"GET / HTTP/1.1\r\nHost: a\r\n").is_none(), "incomplete head");
    }

    #[test]
    fn redirect_sets_a_strict_cookie() {
        let r = String::from_utf8(login_redirect("/app", "tok", true)).unwrap();
        assert!(r.starts_with("HTTP/1.1 302"));
        assert!(r.contains("Location: /app\r\n"));
        assert!(r.contains("openshell_token=tok; Path=/; HttpOnly; SameSite=Strict"));
        assert!(r.contains("; Secure"));
    }
}
