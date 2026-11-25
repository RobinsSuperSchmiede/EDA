# Control API

Every running shell exposes a local HTTP API on `127.0.0.1:<port>`. Its URL and admin token are written to
`<project>/.openshell/runtime.json` while the shell runs:

```json
{ "pid": 648, "url": "http://127.0.0.1:52472", "token": "…", "mode": "development", "host": "desktop", "app": "Todo App", "started_at": "…", "version": "0.1.0" }
```

Authenticate with `Authorization: Bearer <token>` (WebSocket: `?token=`). Requests with a foreign `Host` header or a
non-allow-listed `Origin` are rejected with 403.

Three kinds of token: the **admin** token of `runtime.json`, the **app** token the frontend gets (SDK and UI bridge
only), and **agent session** tokens (`osa_…`, `openshell agents token`) which reach `/mcp/app` – and with the full
profile `/mcp` and `/v1/tools` – with the session's scopes. With `agents.enabled: false` (the default) `/mcp`,
`/mcp/app`, `/v1/tools` and `/v1/ui/bridge` answer 403 for every token.

## Endpoints

| Method & path | Token | Description |
|---|---|---|
| `GET /v1/health` | – | `{ ok, app, mode, host, platform, version }` |
| `GET /sdk.js`, `GET /bridge.js` | – | Frontend SDK, UI bridge script |
| `GET /v1/status` (alias `/v1/info`) | admin | App, mode, host, platform capabilities, pid, URLs, services, bridged pages, pending approvals, agent permissions, `in_background`, `keep_alive`, `remote` |
| `POST /v1/services/{name}/{start\|stop\|restart}` | admin | Change the desired state (returns immediately) |
| `GET /v1/logs?service=&since=&limit=&contains=&errors_only=` | admin | `{ lines: [{ seq, ts, service, stream, text }], last_seq }` |
| `POST /v1/shutdown` | admin | Stop all services and exit |
| `GET /v1/approvals` | admin | `{ pending: [{ id, agent, tool, permission, summary, args, created_at, expires_at }] }` |
| `POST /v1/approvals/{id}/{approve\|deny}` | admin | Answer an approval request |
| `GET /v1/audit?limit=` | admin | Recent audit entries |
| `GET /v1/tools` | admin, session (full) | Tools with `permission`, `access` and `input_schema` (access for the caller) |
| `POST /v1/tools/{name}` | admin, session (full) | Call a tool with the **agent** policy (limited to a session's scopes). Header `X-OpenShell-Agent` names the agent. |
| `POST /mcp` | admin, session (full) | MCP Streamable HTTP (JSON-RPC, JSON responses, `Mcp-Session-Id`) |
| `POST /mcp/app` | admin, session | MCP with the app profile (the app's own functions) |
| `GET /v1/agents` | admin | Effective agent permissions: `enabled`, `granted`, `by_mode`, `approval_required`, `sdk_permissions`, `sessions` and `devices` with their effective scopes |
| `GET /v1/ui/bridge` | app or admin | WebSocket for `bridge.js` |
| `GET /v1/sdk/info` | app | App info and SDK permissions |
| `POST /v1/sdk/call/{tool}` | app | Call a tool with the **SDK** policy |
| `POST /v1/lifecycle` | admin | App lifecycle from the host: `{ "kind": "foreground" \| "background" \| "low_memory" }`, `{ "kind": "power_save", "enabled": true }`, `{ "kind": "network", "connected": true, "metered": false }` → paused/resumed services and `keep_alive` |
| `GET /v1/remote` | admin | Remote gateway: `enabled`, `urls`, `mcp_url`, shared `token` (if `shared_token`), `tls_fingerprint`, `allow`, `exposed`, `devices`, `blocked`, `pairing_uri` |
| `GET /v1/remote/devices` | admin | Paired devices (`name`, `paired_at`, `last_seen`, `scopes`) |
| `POST /v1/remote/devices` | admin | Pair a device: `{"name":"laptop","scopes":["read"]}` → its `token` (only in this answer) and `pairing_uri` |
| `POST /v1/remote/devices/{name}/revoke` | admin | Revoke a device's token |
| `POST /v1/remote/unblock` | admin | `{"address":"192.168.1.7"}` – lift a lockout |
| `GET /v1/update` | admin | `{configured, update: {current, latest, available, download, page}}` from `package.update_url` |

The optional remote gateway (`runtime.remote`) is a separate listener with `GET /v1/health`, `GET /v1/remote/info`
and `POST /mcp/app` only, authorised with the shared or a device token – see [remote-access.md](remote-access.md).

## Example

```bash
URL=$(jq -r .url .openshell/runtime.json); TOKEN=$(jq -r .token .openshell/runtime.json)
curl -s -H "Authorization: Bearer $TOKEN" $URL/v1/status
curl -s -H "Authorization: Bearer $TOKEN" -H "X-OpenShell-Agent: my-script" -H "Content-Type: application/json" \
     -d '{"service":"backend","errors_only":true}' $URL/v1/tools/logs_read
```
