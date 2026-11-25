# Configuration reference (`openshell.json`, version 1)

OpenShell looks for `openshell.json` or `openshell.toml` in the given directory and its parents. All relative paths
are resolved against the directory containing the file. Unknown fields are errors (typos are caught early).
Editor completion: add `"$schema": "https://raw.githubusercontent.com/RobinsSuperSchmiede/EDA/main/schema/openshell.schema.json"`.
`openshell schema` prints the schema; `openshell doctor` validates the file and the environment.

## Top level

| Field | Type | Description |
|---|---|---|
| `version` | number | Schema version, currently `1` (default). Newer versions are rejected with a clear message. |
| `app` | object | `name` (required), `id`, `version` (semver, needed for installers), `description`, `icon` (PNG for window and installers). |
| `window` | object | Native window settings. |
| `frontend` | object | What the window shows. |
| `services` | array | Backends, databases, workers, AI servers, … |
| `databases` | array | Built-in SQLite databases managed by OpenShell. |
| `runtime` | object | Runtime manager behaviour. |
| `agents` | object | Agent Interaction Layer: on/off (opt-in), permissions, approvals, providers. |
| `capabilities` | array | Functions the app exposes to agents (HTTP endpoint, script, task). |
| `tasks` | object | Named commands (`"test": "pytest"`), runnable by agents with the `system` scope. |
| `package` | object | Installer settings – see [installers.md](installers.md). |
| `runtimes` | object | Language runtimes shipped inside the app – see [`runtimes`](#runtimes). |

## `window`

| Field | Default | Description |
|---|---|---|
| `title` | `app.name` | Window title |
| `width`, `height` | 1280, 800 | Initial size |
| `min_width`, `min_height` | – | Minimum size (both required to take effect) |
| `resizable` | `true` | |
| `maximized` | `false` | |
| `close_to_tray` | `false` | Closing hides the window; services keep running |
| `title_bar` | `"native"` | `"overlay"`: the frontend draws the title bar (see [sdk.md](sdk.md#own-title-bar)). No system title bar on Windows/Linux; window buttons over the content on macOS |
| `tray` | `true` | Show a tray icon (show, reload, runtime panel, restart services, quit) |
| `devtools` | dev mode only | Allow WebView devtools |
| `enabled` | `true` | `false`: no window – the app runs in the tray / background (services, databases, agent functions). Apps without a frontend show OpenShell's console in the window |

## `frontend`

OpenShell does not care which framework you use. It needs one of:

- `command` + `url` (or `port`): a process that serves the frontend (dev server, local web server),
- `url` alone: an already running or remote frontend,
- `dist`: a directory of built files that OpenShell serves itself.

| Field | Description |
|---|---|
| `command` | Shell string (`"npm run dev"`) or argv array |
| `cwd` | Working directory |
| `env` | Extra environment variables (values support `${...}`) |
| `port` | Number or `"auto"`; available as `${port}` and `$PORT` |
| `url` | URL loaded into the window |
| `dist` | Built frontend directory (production) |
| `build` | Build command for `openshell build` |
| `setup` | One-time step, e.g. `{ "command": "npm install", "creates": "frontend/node_modules" }` |
| `proxy` | Static server reverse proxy, e.g. `{ "/api": "${services.backend.url}" }` |
| `watch` | Paths that reload the window when changed (dev mode; useful for static frontends) |
| `healthcheck` | Override readiness check (default: the URL answers) |
| `depends_on` | Services that must be healthy first |
| `production` | Overrides for production mode: `{ "command", "url", "dist" }` |

Mode selection: **development** uses `command`/`url`, falling back to `dist`. **Production** uses `production`
overrides, then `dist`, then the development settings.

## `services[]`

| Field | Default | Description |
|---|---|---|
| `name` | – | Unique, `[a-z0-9_-]` (`frontend` is reserved) |
| `description`, `kind` | – | Informational (`backend`, `database`, `worker`, `ai`, …) |
| `command` | – | Shell string or argv. In argv form `python` falls back to `python3` (and vice versa) |
| `args` | `[]` | Appended arguments |
| `cwd` | project root | |
| `env` | `{}` | Values support `${...}` |
| `port` | – | Number or `"auto"`; exported as `PORT` |
| `url` | `http://127.0.0.1:<port>` | Base URL (used by `${services.<name>.url}` and HTTP capabilities) |
| `external` | `false` | Do not start; only monitor (`url` / `healthcheck` required) |
| `healthcheck` | TCP on `port` | URL string (HTTP 2xx/3xx) or `{ "http" \| "tcp", "interval_ms", "timeout_ms", "failure_threshold", "ready_timeout_ms" }` |
| `restart` | `true` (on-failure) | `true`, `false`, `"never"`, `"on-failure"`, `"always"` |
| `max_restarts` | 5 | Crashes per minute before the service is marked `failed` |
| `depends_on` | `[]` | Started after these are healthy; stopped before them |
| `autostart` | `true` | Start with the runtime |
| `setup` | – | One-time step (`command`, `creates`, `cwd`) |
| `build` | – | Build step for compiled services: command, or `{ "command", "cwd", "on_start": "development" \| "always" \| "never" }`. Runs before every start in dev mode and in `openshell build` / `package`. See [languages.md](languages.md) |
| `openapi` | – | Import the service's OpenAPI 3 (JSON) document as agent tools: `"/openapi.json"`, URL, project file, or `{ "location", "include", "exclude", "prefix" }` |
| `watch` | `[]` | Paths whose changes restart the service (dev mode). `node_modules`, `.git`, `__pycache__`, `target`, `.venv`, `*.log` are ignored |
| `modes` | all | Only run in `["development"]` or `["production"]` |
| `platforms` | all | Only run on `desktop`, `android` and/or `server`, e.g. `["desktop", "server"]` for a service that needs a toolchain. See [platforms.md](platforms.md) |
| `background` | `keep` | Mobile platforms: `pause` stops the service while the app is in the background and starts it again afterwards |
| `stop_timeout_ms` | `runtime.shutdown_timeout_ms` | Grace period before a forced kill (Unix; Windows terminates the tree immediately) |
| `requires` | `{}` | Runtime versions the service needs, e.g. `{ "python": ">=3.10", "java": ">=17" }`. Known runtimes: `python`, `node`, `java`, `dotnet`, `go`, `rust`. Checked by `openshell doctor` (error) and when the app starts (warning in the service's log). The frontend has the same field for its dev server (`"requires": { "node": ">=18" }`) |

Version requirements: `>=3.10`, `>3`, `<=21`, `<4`, `=3.12.1`, `3.12` (same as `=3.12`, matches 3.12.x),
`^1.2` (`>=1.2 <2`), `~1.2` (`>=1.2 <1.3`), combined with commas: `">=3.10, <3.13"`.

Every process also receives `OPENSHELL=1`, `OPENSHELL_MODE`, `OPENSHELL_SERVICE`, `OPENSHELL_PROJECT_ROOT`,
`OPENSHELL_CONTROL_URL`, `OPENSHELL_DATA_DIR` (writable per-app data directory – per user for installed apps; store
databases and files there) and `PYTHONUNBUFFERED=1` (unless set).

## Variables

Usable in commands, args, env values, URLs, health checks and proxy targets:

| Variable | Value |
|---|---|
| `${port}` | Port of the current service / frontend |
| `${services.<name>.port}` / `${services.<name>.url}` | Another service |
| `${frontend.url}` | Frontend URL |
| `${project.root}` | Absolute project directory |
| `${mode}` | `development` / `production` |
| `${env.NAME}` | Environment variable of the shell |
| `$$` | Literal `$` |

## `runtime`

| Field | Default | Description |
|---|---|---|
| `autostart` | `true` | Start services when the shell starts |
| `background_mode` | `false` | Keep running in the tray when the window is closed |
| `control_port` | random | Fixed port for the control API |
| `log_buffer_lines` | 10000 | Lines kept in memory (files in `.openshell/logs/`) |
| `shutdown_timeout_ms` | 5000 | Default grace period when stopping |
| `launch_at_login` | `false` | Start the installed app when the user logs in (installed apps only) |
| `remote` | off | Remote access for other devices – see [`runtime.remote`](#runtimeremote) |

### `runtime.remote`

Makes the app usable from other devices (a laptop using an app hosted on a phone, an agent on another machine).
Everything except `enabled` is optional. Full guide: [remote-access.md](remote-access.md).

| Field | Default | Description |
|---|---|---|
| `enabled` | `false` | Start the gateway (app profile over MCP) and the exposed ports |
| `bind` | `0.0.0.0` | Address to listen on; a VPN address (Tailscale `100.x.y.z`) limits access to your devices |
| `port` | 7420 | Gateway port |
| `allow` | everyone | IP addresses / CIDR ranges that may connect, e.g. `["192.168.1.0/24", "100.64.0.0/10"]`. Loopback is always allowed |
| `shared_token` | `true` | Accept the shared pairing token; `false` = only per-device tokens (`openshell remote pair <device>`) |
| `rate_limit` | 120 / 10 / 5 | `{ "requests_per_minute": 120, "max_failed_auth": 10, "block_minutes": 5 }` per address; `0` switches a limit off |
| `tls` | off | `{ "enabled": true }` with a self-signed certificate (clients pin its SHA-256 fingerprint), or `{ "enabled": true, "cert": "certs/fullchain.pem", "key": "certs/key.pem" }` |
| `expose` | none | Services reachable on their own port: `[{ "service": "api", "port": 8080 }]` – see below |

`expose[]` entries:

| Field | Default | Description |
|---|---|---|
| `service` | – | Service name, or `frontend` |
| `port` | – | Port other devices connect to (must differ from the gateway and control ports) |
| `protocol` | `http` | `http` (websites, APIs; token login, TLS) or `tcp` (any protocol, e.g. PostgreSQL, Redis, MQTT) |
| `auth` | `token` for `http` | `token`: pairing/device token as `Authorization: Bearer`, cookie, or once `?openshell_token=…` in the browser; `none`: no login (use `allow`). `tcp` is always `none` |
| `allow` | `runtime.remote.allow` | Addresses / CIDR ranges for this port only |

## `agents`

| Field | Default | Description |
|---|---|---|
| `enabled` | `false` | **Opt-in.** `true` opens the agent layer: MCP endpoints (`/mcp`, `/mcp/app`, `--mcp`), the tools API, the UI bridge and the remote gateway's MCP endpoint. While `false` they refuse every request and the UI bridge is not injected; the app, its frontend SDK and exposed services keep working. `openshell create`, `openshell init` and `openshell add function` write `true`; `openshell agents enable` / `disable` switch it |
| `permissions` | dev: `read, ui, actions, services, files, code` · prod: `read, ui, actions` | A list for all modes or `{ "development": [...], "production": [...] }` |
| `approval_required` | `["system"]` | Scopes that need a human decision |
| `approval_timeout_ms` | 120000 | Unanswered requests are denied |
| `sdk_permissions` | `["read", "ui"]` | Scopes for the frontend SDK (app token) |
| `providers` | `[]` | `{ "name", "kind": "ollama" \| "lm-studio" \| "openai-compatible" \| "openai" \| "anthropic" \| "mcp" \| "custom", "url", "api_key_env", "models" }` |
| `detect_local_providers` | `true` | Probe Ollama (11434) and LM Studio (1234) |

Permission scopes: `read`, `ui`, `actions` (alias `write`), `services`, `files`, `code`, `system`.

Agent sessions (`openshell agents token <name> --scopes read,actions`) and paired devices
(`openshell remote pair <device> --scopes read`) get their own tokens and scopes; they never get more than
`permissions` grants in the current mode. See [agents.md](agents.md#agent-sessions).

## `runtimes`

Language runtimes shipped inside the app, so the user's machine needs no Python, Node.js or Java:

```json
"runtimes": {
  "python": "3.12",
  "node": { "version": "22", "path": "vendor/node" },
  "java": "21"
}
```

| Runtime | Downloaded from | Default version |
|---|---|---|
| `python` | python-build-standalone (`install_only` builds) | `3.12` |
| `node` | nodejs.org | `22` |
| `java` | Eclipse Temurin JRE (adoptium.net) | `21` |

A value is a version (`"22"`, `"3.12"`, `"21.0.5"`) or `{ "version", "path" }` (default path `runtimes/<name>`,
inside the project). `openshell runtime install` downloads every declared runtime for the current system and checks
its SHA-256; `openshell runtime add <name> [version]` declares and downloads in one step. The runtime's executable
folders are put in front of `PATH` for every service, setup and build step, function and task, so `python main.py`
or `npm install` use the bundled copy. `openshell package` ships the folder with the app. A runtime is built for one
operating system: build installers on each system (the release workflow runs `openshell runtime install` there).
Downloaded runtimes do not belong in git; `runtime add` adds the folder to `.gitignore`.

## `capabilities[]`

```json
{
  "name": "add_todo",
  "description": "Create a todo",
  "permission": "actions",
  "input_schema": { "type": "object", "properties": { "title": { "type": "string" } }, "required": ["title"] },
  "http": { "service": "backend", "method": "POST", "path": "/api/todos" }
}
```

Exactly one target:

- `http` – `service` + `path`, or `url`; GET/DELETE send the input as query parameters, other methods as JSON body.
- `run` – a script/program: `["python", "tools/x.py"]` or `{ "command", "cwd", "env", "timeout_ms" }`. It receives the
  input as JSON on stdin (and in `OPENSHELL_INPUT`) and prints its result (JSON or text) on stdout; a non-zero exit
  code is an error (message from stderr). No server needed – see [building-blocks.md](building-blocks.md#functions).
- `task` – name of a task (input is ignored).

Agents see each capability as a tool `cap_<name>` (developer profile), under its plain name (app profile) and via
`app_invoke`. The frontend calls them with `OpenShell.invoke(name, input)`.

## `databases[]`

```json
{ "name": "main", "migrations": "db/migrations", "agent_access": "write" }
```

| Field | Default | Description |
|---|---|---|
| `name` | – | `[a-z0-9_]`; services get the path as `OPENSHELL_DB_<NAME>` |
| `type` | `"sqlite"` | Engine (SQLite is built in) |
| `path` | `<name>.db` | File, relative to the app's data directory (`OPENSHELL_DATA_DIR`) or absolute |
| `migrations` | – | Folder of `*.sql` files, applied once each in file-name order |
| `agent_access` | `"read"` | `none`, `read` (`db_list`, `db_schema`, `db_query`) or `write` (also `db_execute`) |
| `description` | – | Shown to agents |

The main (or only) database is also exported as `DATABASE_URL=sqlite:///…`. Details:
[building-blocks.md](building-blocks.md#database).

## TOML

```toml
version = 1

[app]
name = "My App"

[frontend]
command = "npm run dev"
cwd = "frontend"
url = "http://localhost:5173"

[[services]]
name = "api"
command = ["node", "server.js"]
port = 3001
restart = "always"
```
