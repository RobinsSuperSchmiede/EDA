# Control API

Every running shell exposes a local HTTP API on `127.0.0.1:<port>`. Its URL and admin token are written to
`<project>/.openshell/runtime.json` while the shell runs:

```json
{ "pid": 648, "url": "http://127.0.0.1:52472", "token": "…", "mode": "development", "host": "desktop", "app": "Todo App", "started_at": "…", "version": "0.1.0" }
```

Authenticate with `Authorization: Bearer <token>` (WebSocket: `?token=`). Requests with a foreign `Host` header or a
non-allow-listed `Origin` are rejected with 403.

## Endpoints

| Method & path | Token | Description |
|---|---|---|
| `GET /v1/health` | – | `{ ok, app, mode, host, version }` |
| `GET /sdk.js`, `GET /bridge.js` | – | Frontend SDK, UI bridge script |
| `GET /v1/status` (alias `/v1/info`) | admin | App, mode, host, pid, URLs, services, bridged pages, pending approvals, agent permissions |
| `POST /v1/services/{name}/{start\|stop\|restart}` | admin | Change the desired state (returns immediately) |
| `GET /v1/logs?service=&since=&limit=&contains=&errors_only=` | admin | `{ lines: [{ seq, ts, service, stream, text }], last_seq }` |
| `POST /v1/shutdown` | admin | Stop all services and exit |
| `GET /v1/approvals` | admin | `{ pending: [{ id, agent, tool, permission, summary, args, created_at, expires_at }] }` |
| `POST /v1/approvals/{id}/{approve\|deny}` | admin | Answer an approval request |
| `GET /v1/audit?limit=` | admin | Recent audit entries |
| `GET /v1/tools` | admin | Tools with `permission`, `access` and `input_schema` |
| `POST /v1/tools/{name}` | admin | Call a tool with the **agent** policy. Header `X-OpenShell-Agent` names the agent. |
| `POST /mcp` | admin | MCP Streamable HTTP (JSON-RPC, JSON responses, `Mcp-Session-Id`) |
| `GET /v1/ui/bridge` | app or admin | WebSocket for `bridge.js` |
| `GET /v1/sdk/info` | app | App info and SDK permissions |
| `POST /v1/sdk/call/{tool}` | app | Call a tool with the **SDK** policy |

## Example

```bash
URL=$(jq -r .url .openshell/runtime.json); TOKEN=$(jq -r .token .openshell/runtime.json)
curl -s -H "Authorization: Bearer $TOKEN" $URL/v1/status
curl -s -H "Authorization: Bearer $TOKEN" -H "X-OpenShell-Agent: my-script" -H "Content-Type: application/json" \
     -d '{"service":"backend","errors_only":true}' $URL/v1/tools/logs_read
```
