# Remote access: use an app from other devices

An OpenShell app can be reached from your other devices over IP address and port – for example a
phone that hosts an app, its database and its API, and a laptop (or an AI agent on it) that uses
them. Everything is **off by default** and every security layer can be switched on separately.

```json
"runtime": {
  "remote": {
    "enabled": true,
    "bind": "0.0.0.0",
    "port": 7420,
    "allow": ["192.168.1.0/24", "100.64.0.0/10"],
    "shared_token": false,
    "tls": { "enabled": true },
    "rate_limit": { "requests_per_minute": 120, "max_failed_auth": 10, "block_minutes": 5 },
    "expose": [
      { "service": "frontend", "port": 8080 },
      { "service": "api", "port": 8081 },
      { "service": "postgres", "port": 5433, "protocol": "tcp", "allow": ["100.64.0.0/10"] }
    ]
  }
}
```

## What other devices can reach

| Port | What | Login |
|---|---|---|
| `runtime.remote.port` (7420) | The **gateway**: `GET /v1/health` (public), `GET /v1/remote/info` and `POST /mcp/app` – the app's own functions over MCP, never developer tools, files, logs or the admin API | token |
| `expose[].port`, `protocol: http` | A service's website or API, forwarded unchanged (WebSockets, streaming and uploads work) | token (default) or none |
| `expose[].port`, `protocol: tcp` | Any TCP protocol (PostgreSQL, Redis, MQTT, a game server …), bytes forwarded unchanged | address allowlist only |

The local control API (`127.0.0.1`, admin token) is never reachable from other devices.

## Security layers

| Layer | Setting | Notes |
|---|---|---|
| Off by default | `enabled` | Nothing listens until you switch it on |
| Bind address | `bind` | `0.0.0.0` = every network; a VPN address (`100.x.y.z` for Tailscale) = only your devices |
| Opt-in per service | `expose[]` | A service is reachable only if you list it, on the port you choose |
| Address allowlist | `allow`, `expose[].allow` | IPs or CIDR ranges; others are dropped right after the TCP handshake. Loopback is always allowed |
| Tokens | shared token, `openshell remote pair <device>` | Per-device tokens are stored as SHA-256 hashes, revocable one by one; `shared_token: false` accepts only them |
| Rate limit | `rate_limit.requests_per_minute` | Per address: requests on the gateway, new connections on exposed ports |
| Lockout | `rate_limit.max_failed_auth`, `block_minutes` | Too many wrong tokens block the address for a while; `openshell remote unblock <ip>` lifts it |
| TLS | `tls` | Gateway and exposed HTTP ports. Self-signed (fingerprint pinning) or your own certificate |
| Browser protection | – | The gateway rejects requests with an `Origin` header, so websites cannot call it |
| Audit | audit log, channel `remote` | Function calls with the device name, logins, failed logins, blocks, connections to exposed ports |

## Pair a device

```bash
openshell remote pair laptop
```

prints the device's token (only once), the MCP endpoint, the TLS fingerprint, a link per exposed
HTTP service and a QR code with a pairing URI:

```
openshell://pair?app=Notes&url=https%3A%2F%2F192.168.1.20%3A7420&token=osd_…&fp=AB%3ACD%3A…
```

Use the token as `Authorization: Bearer <token>` (MCP clients, scripts) or open
`https://192.168.1.20:8080/?openshell_token=<token>` once in a browser: OpenShell sets an
`HttpOnly`, `SameSite=Strict` cookie and reloads the page without the token in the address bar.

```bash
openshell remote devices          # paired devices, last use
openshell remote revoke laptop    # the laptop's token stops working immediately
openshell remote                  # everything at a glance
```

MCP client configuration (Claude Code, Cursor, …):

```json
{ "url": "https://192.168.1.20:7420/mcp/app", "headers": { "Authorization": "Bearer osd_…" } }
```

## TLS

`"tls": { "enabled": true }` creates a self-signed certificate once
(`.openshell/data/remote-cert.pem`, `remote-key.pem`) for `localhost`, the device's addresses and
reuses it. Browsers warn about self-signed certificates; compare the fingerprint that
`openshell remote` shows with the one the browser displays, or use your own certificate:

```json
"tls": { "enabled": true, "cert": "certs/fullchain.pem", "key": "certs/privkey.pem" }
```

With Tailscale, `tailscale cert <name>.ts.net` gives a trusted certificate for the device.

## Typical setups

| Goal | Configuration |
|---|---|
| Phone hosts an app, laptop at home uses it | `bind: "0.0.0.0"`, `allow: ["192.168.1.0/24"]`, `tls.enabled`, pair the laptop |
| Reachable only through a VPN | `bind: "100.x.y.z"` (the phone's Tailscale address), `shared_token: false` |
| Database on the phone for a desktop tool | `expose: [{ "service": "postgres", "port": 5433, "protocol": "tcp", "allow": ["192.168.1.10"] }]` |
| A website for the family in the LAN | `expose: [{ "service": "frontend", "port": 8080 }]`, open the link from `openshell remote pair <device>` |

Never expose a port to the internet without TLS and an allowlist; prefer a VPN.

## Limits

- Exposed HTTP ports check the token on the **first request of a connection**; later requests on the
  same keep-alive connection are trusted. After revoking a device, its open connections end when the
  client closes them (restart the app to cut them immediately).
- `tcp` ports have no login – use `allow`, a VPN, or the service's own authentication.
- Rate limits and blocks are kept in memory and reset when the app restarts.
- The tokens give access to the app profile and exposed services only. Per-device *permissions*
  (different functions per device) are planned.
