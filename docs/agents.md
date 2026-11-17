# Agent Interaction Layer

OpenShell gives AI agents a standardized, model-agnostic interface to a running desktop application – in two
profiles:

| Profile | For | Tools |
|---|---|---|
| **app** ("use the app") | assistants that should *work with* the application, e.g. Claude Desktop on the user's PC | only the application's own functions (`list_todos`, `add_todo`, …) + `app_status` |
| **full** ("develop the app") | coding agents that build and evolve the application | everything below: runtime, logs, services, files, UI automation, capabilities |

Neither profile needs the UI: the app profile calls the application's API directly; UI automation is an *extra*
for the full profile.

In the full profile agents can

- query application and runtime state, read logs,
- start, stop and restart services,
- see and operate the user interface (semantic snapshot, click, fill, navigate, …),
- discover and invoke application capabilities,
- read and modify project files (scoped to the project),
- run declared tasks,

all subject to permission scopes, optional human approval and an audit log. OpenShell does not ship or prescribe an
AI model – it is infrastructure for any agent: Claude Code, Codex, Cursor, OpenAI- or Anthropic-based agents, local
models via Ollama / LM Studio, or your own agent system.

## Connecting

### Using an installed app

Installed apps (see [installers.md](installers.md)) are MCP servers themselves:

```json
{ "mcpServers": { "my-app": { "command": "C:\\Users\\me\\AppData\\Local\\Programs\\My App\\my-app.exe", "args": ["--mcp"] } } }
```

If the app is not running, it starts **in the background without a window**, so an agent can use it while the user
does something else. Opening the app later replaces the background instance with the window. The tool list contains
only the app's functions; permissions follow `agents.permissions.production`. Add `--profile full` for the developer
tool set. The runtime panel (title bar right-click → *Runtime panel*) shows both commands ready to copy.

### Using a project from the CLI

```bash
openshell mcp --app --start --project /path/to/app   # app profile, start headless (production) if needed
```

### Developing: MCP over stdio (recommended)

```bash
openshell mcp --project /path/to/app            # add --start to launch `openshell dev --headless` on demand
```

Configure it as an MCP server in your client:

```bash
claude mcp add my-app -- openshell mcp --project /path/to/app       # Claude Code
```

```json
{ "mcpServers": { "my-app": { "command": "openshell", "args": ["mcp", "--project", "/path/to/app"] } } }
```

The stdio server forwards to the running shell and reconnects automatically, so the agent can be started before the
app. If the app is not running, `tools/list` still works and tool calls explain how to start it.

### MCP over HTTP

`POST {control_url}/mcp` (full profile) and `POST {control_url}/mcp/app` (app profile) implement the Streamable HTTP
transport (JSON responses). They require
`Authorization: Bearer <token>` from `.openshell/runtime.json`. Because the port and token change on each start,
stdio is more convenient; set `runtime.control_port` for a fixed port.

### Plain HTTP / JSON

```
GET  /v1/tools                      list tools with permission and access (allowed / approval / denied)
POST /v1/tools/{name}               body = arguments → { "ok": true, "result": ... } or { "ok": false, "result": { "error": "..." } }
```

Send `X-OpenShell-Agent: <name>` to identify the agent in the audit log. From a terminal: `openshell call <tool> '<json>'`.

## Tools

| Tool | Scope | Purpose |
|---|---|---|
| `app_info` | read | Overview: app, mode, services, permissions, capabilities, hints. Call first. |
| `runtime_status` | read | State, pid, port, URL, restarts, last error per service |
| `logs_read` | read | Log lines (`service`, `since`, `limit`, `contains`, `errors_only`) |
| `capabilities_list` | read | Capabilities from config and frontend |
| `config_get` | read | Effective configuration |
| `providers_list` | read | Configured and detected AI providers with models |
| `audit_read` | read | Recent agent actions |
| `ui_pages` | ui | Pages connected to the UI bridge |
| `ui_snapshot` | ui | Title, URL, headings, dialogs, text and interactive elements with refs |
| `ui_click` | ui | Click by `ref`, `selector` or `text` |
| `ui_fill` | ui | Set input/textarea/select/checkbox/contenteditable (`label`, `value`, `submit`) |
| `ui_press` | ui | Key press (`Enter`, `Escape`, …) |
| `ui_read_text` | ui | Text of page or element |
| `ui_wait_for` | ui | Wait for selector/text (or its disappearance) |
| `ui_navigate` | ui | Navigate to `path` (SPA-friendly) or `url` |
| `ui_reload` | ui | Reload the frontend |
| `window_control` | ui | show / hide / focus / minimize / maximize / unmaximize |
| `notify` | ui | Native notification |
| `app_invoke` | capability's scope | Invoke a capability by name |
| `cap_<name>` | capability's scope | One tool per capability |
| `service_start` / `service_stop` / `service_restart` | services | Waits until the service is healthy and returns recent logs on failure |
| `fs_list` / `fs_read` / `fs_search` | files | Project files |
| `fs_write` | code | Create/overwrite a project file |
| `task_run` | system | Run a task from `tasks` and return its output |
| `ui_eval` | system | Evaluate JavaScript in the page |

## UI automation

`ui_snapshot` returns a compact, semantic view of the page:

```json
{
  "url": "http://127.0.0.1:52471/",
  "title": "Todo App",
  "headings": [{ "level": 1, "text": "Todo App" }],
  "elements": [
    { "ref": "e1", "role": "textbox", "name": "New todo", "value": "" },
    { "ref": "e2", "role": "button", "name": "Add" },
    { "ref": "e6", "role": "checkbox", "name": "Buy milk", "checked": false }
  ],
  "text": "Todo App 3 todos · 1 done …",
  "actions": [{ "name": "set_filter", "description": "…" }]
}
```

Use the refs with `ui_click {"ref": "e2"}` or `ui_fill {"ref": "e1", "value": "…", "submit": true}`. Refs stay stable
while the element exists; take a new snapshot after larger UI changes. The bridge (`bridge.js`) is injected into the
desktop WebView and into pages served by OpenShell's static server, so it works with any framework and also in
browser mode (`openshell dev --headless`). Password values are never included in snapshots.

## Capabilities: the application's own functions

Prefer capabilities over UI automation – they are faster and more reliable, and they are what the app profile
exposes. Three sources:

- **OpenAPI import** – set `"openapi": "/openapi.json"` (FastAPI), `"/v3/api-docs"` (Spring/springdoc),
  `"/swagger/v1/swagger.json"` (ASP.NET) or a file/URL on a service. When the service becomes healthy, every operation
  becomes a tool: name from `operationId`, input schema from path/query parameters and the JSON request body,
  `GET` → scope `read`, other methods → `actions`. Filter with `{ "location": "...", "include": [...], "exclude": [...], "prefix": "shop" }`.
- **Declared** in `openshell.json` (`capabilities`), backed by an HTTP endpoint or a task.
- **Registered by the frontend** at runtime with the SDK:

```js
OpenShell.agent.registerAction("set_filter", {
  description: "Show all, open or done todos",
  permission: "ui",
  input_schema: { type: "object", properties: { filter: { enum: ["all", "open", "done"] } } },
}, ({ filter }) => setFilter(filter));
```

## Permissions and approvals

Every tool needs exactly one scope:

| Scope | Allows | Default dev | Default prod |
|---|---|---|---|
| `read` | state, logs, config, capabilities list | ✅ | ✅ |
| `ui` | UI automation, window control, notifications | ✅ | ✅ |
| `actions` | capabilities that change data | ✅ | ✅ |
| `services` | start/stop/restart services | ✅ | – |
| `files` | read project files | ✅ | – |
| `code` | write project files | ✅ | – |
| `system` | declared tasks, JavaScript evaluation | – | – |

Configure with `agents.permissions`. Scopes in `agents.approval_required` (default `["system"]`) need a human
decision: the desktop app shows a dialog (and the runtime panel lists pending requests), headless mode prints the
request; answer with `openshell approve <id>` / `openshell deny <id>`. Unanswered requests are denied after
`approval_timeout_ms`.

Every call – allowed, denied, rejected or failed – is appended to `.openshell/audit.jsonl` with agent name, channel,
tool, scope, (truncated) arguments, outcome and duration. View it with `openshell audit`.

## Security

- The control API listens on `127.0.0.1` only and requires a bearer token. The admin token lives in
  `.openshell/runtime.json` (mode 0600 on Unix, ignored by git).
- Requests with a foreign `Host` header (DNS rebinding) or a non-allow-listed `Origin` (other websites) are rejected.
- The frontend gets a separate app token limited to `/v1/sdk/*` and the bridge, with `agents.sdk_permissions`.
- Application frontends loaded from http(s) URLs get no Tauri IPC access.
- File tools reject absolute paths, `..`, symlink escapes and OpenShell's state directory; `fs_write` also protects `.git`.
- Agents cannot run arbitrary shell commands; only tasks declared in the config, under the `system` scope.
- Limits: the permission layer governs cooperating agents. A local process running as the same user can read the token
  file; OpenShell is not a sandbox against local malware.

## A typical agent loop

1. `app_info` → understand the app, its services and capabilities.
2. Edit code with the agent's own file tools (or `fs_write`).
3. Services with `watch` restart automatically; otherwise `service_restart`. Check `runtime_status` / `logs_read {"errors_only": true}`.
4. `ui_reload` (static frontends reload automatically), then `ui_snapshot` and `ui_click` / `ui_fill` to verify the feature like a user would.
