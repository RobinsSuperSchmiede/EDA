# Building apps on OpenShell – guide for AI coding agents

This guide is written for AI coding agents (Claude Code, Codex, Cursor, Copilot, Aider, custom agents) that are asked
to **build a new desktop application with OpenShell** or to extend an existing OpenShell app. It is self-contained:
follow it top to bottom. The same text is printed by `openshell guide`.

## What OpenShell is (in one paragraph)

OpenShell is a desktop shell (Rust + Tauri 2) that runs a web frontend in a native window and supervises any number of
backend processes. An application is **not compiled into the shell**: it is a normal project folder with a web
frontend, one or more services (any language) and an `openshell.json` that describes them. You write the application;
OpenShell starts it, checks health, restarts on crashes, reloads on file changes, collects logs, packages installers
and exposes the app to AI agents over MCP.

## The app contract

Everything an app must do – nothing more:

| # | Rule | Why |
|---|---|---|
| 1 | Every service that listens reads its port from the **`PORT`** environment variable and binds to **`127.0.0.1`**. Use `"port": "auto"` in the config. | Ports are assigned by OpenShell; no conflicts between apps. |
| 2 | HTTP services answer **`GET /health`** with `200` (configure `"healthcheck": "http://127.0.0.1:${port}/health"`). | Readiness and dependency ordering. |
| 3 | Persistent data goes to **`OPENSHELL_DATA_DIR`** (fallback for running without OpenShell: a local `data/` folder). Never write into the project/install folder at runtime. | Installed apps live in read-only or replaced folders; the data dir is per user. |
| 4 | Log to **stdout/stderr**, one line per event, unbuffered (Python gets `PYTHONUNBUFFERED=1` automatically; flush in Java/C++). | Logs are shown in the terminal, the runtime panel and to agents. |
| 5 | The frontend calls the backend via **relative URLs** (`/api/...`). Static frontends: `"proxy": {"/api": "${services.backend.url}"}`; dev servers (Vite): proxy `/api` to `process.env.BACKEND_URL` and set `"env": {"BACKEND_URL": "${services.backend.url}"}`. | Same code works in dev, production and in a browser. |
| 6 | Expose the app's functions to agents as **capabilities** (config, OpenAPI or frontend actions) – see below. | Agents can *use* the app without UI automation. |
| 7 | The frontend works without OpenShell too: guard SDK usage with `if (window.__OPENSHELL__)`. | Browser mode, tests. |

## Workflow

### 1. Check the environment

```bash
openshell --version          # CLI available?
openshell doctor             # in a project: config, commands, ports, toolchain
openshell create --list      # available templates
```

If `openshell` is missing, tell the user to install it (see the OpenShell README) – do not try to re-implement it.

### 2. Choose the building blocks (new app), a template, or `init` (existing web app)

An app needs **only the blocks the task requires** – frontend, backend, database, functions for AI agents
(docs/building-blocks.md). Pick the smallest set:

| User wants | Blocks |
|---|---|
| A UI that stores data locally | `frontend,database` (the page uses `OpenShell.db`; no backend) |
| Tools/an AI interface for agents, no UI | `functions` (+ `database` if data must persist) |
| An API / background service | `backend` (+ `database`) |
| A full app | `frontend,backend,database` (+ `functions`) |

```bash
openshell create <name> --with frontend,database --lang python   # compose; files + openshell.json + AGENTS.md
openshell add function <name>                                    # add blocks later, in any order
openshell add database [name] | openshell add backend --lang node | openshell add frontend
```

Use a framework template when the user wants a specific stack:

| User wants | Template |
|---|---|
| Simple app, no build tools, fastest start | `vanilla-python` (HTML/JS + Python stdlib) |
| React UI | `react-python` or `react-node` |
| Vue / Svelte UI | `vue-python` / `svelte-go` |
| Java backend | `vanilla-java` (javac) – Spring Boot: start from `vanilla-java`, switch the service to `mvnw spring-boot:run` (see docs/languages.md) |
| C++ backend | `vanilla-cpp` (CMake) |
| Something else | `custom`, then fill in `openshell.json` |

```bash
openshell create <name> --template <template>     # creates <name>/ with AGENTS.md for you
# or, in an existing project:
openshell init                                    # writes openshell.json (+ AGENTS.md if missing)
```

Python backends in templates use only the standard library on purpose. If you need packages (FastAPI, SQLAlchemy, …),
add a `requirements.txt` and a setup step: `"setup": { "command": "python -m pip install -r requirements.txt",
"creates": "<marker>" }`, or a venv (`python -m venv .venv` + `.venv/Scripts/python` / `.venv/bin/python` as command).

### 3. Run it and keep it running

```bash
openshell dev --headless     # no window; prints the URL. Use this when you (the agent) drive the app.
openshell dev                # with the native window (when the user is watching)
```

Run it **in the background** (it blocks). Then verify:

```bash
openshell status             # every service must be "running"
openshell logs --errors      # stderr + runtime messages
openshell logs backend -n 50
```

While it runs, OpenShell restarts services whose `watch` paths change and reloads static frontends – **do not restart
manually after every edit**; check `openshell status` / `openshell logs` instead. Compiled services (Java/C++) rebuild
automatically; a failing build shows the service as `failed` with compiler output in the logs and retries on the next
change.

### 4. Implement features

- Backend first (endpoint + `/health` stays green), then frontend, then capabilities.
- After each step: `openshell status`, `openshell logs --errors`, and exercise the endpoint:
  `curl` the service URL from `openshell status`, or call a capability: `openshell call cap_<name> '<json>'`.

### 5. Verify the UI like a user

With the app running (window or a browser tab on the printed URL for static frontends):

```bash
openshell call ui_snapshot                                   # page text + interactive elements with refs (e1, e2, …)
openshell call ui_fill '{"label": "Title", "value": "Test", "submit": true}'
openshell call ui_click '{"text": "Save"}'                   # or {"ref": "e7"}
openshell call ui_wait_for '{"text": "Saved"}'
openshell call ui_read_text '{"selector": "#list"}'
```

Notes: the UI bridge is injected into the desktop window and into pages served by OpenShell's static server. For dev
server frontends (Vite) in `--headless` mode it is not injected – verify through the API/capabilities instead, or run
`openshell dev` with the window.

If you are connected over MCP (full profile: `openshell mcp`), the same tools exist as MCP tools.

### 6. Make the app usable for agents (capabilities)

Design capabilities like a small, task-oriented API: verbs, clear descriptions, JSON Schema inputs, `read` for
queries and `actions` for changes. Four ways:

**0) Script functions** – no server needed (`openshell add function <name>` scaffolds one): a script reads a JSON
object from stdin and prints JSON to stdout; declare it with `"run": ["python", "tools/<name>.py"]`. Use the database
from scripts through `OPENSHELL_DB_<NAME>`. Errors: message on stderr + non-zero exit. Logs: `openshell logs fn:<name>`.


**a) Declared in `openshell.json`** (HTTP endpoint or task):

```json
"capabilities": [
  {
    "name": "add_item",
    "description": "Create an item. Returns the created item with its id.",
    "permission": "actions",
    "input_schema": { "type": "object", "properties": { "title": { "type": "string" } }, "required": ["title"] },
    "http": { "service": "backend", "method": "POST", "path": "/api/items" }
  }
]
```

GET/DELETE send the input as query parameters, other methods as JSON body.

**b) OpenAPI import** – if the backend has an OpenAPI 3 JSON document (FastAPI `/openapi.json`, Spring springdoc
`/v3/api-docs`, ASP.NET `/swagger/v1/swagger.json`): `"openapi": "/openapi.json"` on the service. Give operations
meaningful `operationId`s and summaries – they become tool names and descriptions. Filter with
`{"location": "/openapi.json", "include": ["items"], "exclude": ["internal"]}`.

**c) Frontend actions** for UI-level functions:

```js
if (window.__OPENSHELL__) {
  const { OpenShell } = await import(`${window.__OPENSHELL__.api}/sdk.js`);
  OpenShell.agent.registerAction("open_item", {
    description: "Show an item in the editor",
    permission: "ui",
    input_schema: { type: "object", properties: { id: { type: "integer" } }, required: ["id"] },
  }, ({ id }) => openItem(id));
}
```

Check: `openshell call capabilities_list`, then test each with `openshell call cap_<name> '<json>'`. Agents that use
the app connect with `openshell mcp --app` and see exactly these functions.

### 7. Production check and packaging

```bash
openshell stop
openshell build              # frontend.build + services[].build
openshell start --headless   # production mode (static dist, no file watching, restricted agent permissions)
openshell status && openshell stop
openshell package            # native installer (needs app.version as semver, optional app.icon PNG)
```

## `openshell.json` cheat sheet

```jsonc
{
  "version": 1,
  "app": { "name": "Notes", "id": "com.example.notes", "version": "0.1.0", "description": "…", "icon": "icon.png" },
  "window": { "width": 1200, "height": 800, "min_width": 600, "min_height": 400 },

  // frontend: dev server (command+url), static files (dist) or external url
  "frontend": {
    "command": "npm run dev -- --host 127.0.0.1 --port ${port} --strictPort",
    "cwd": "frontend", "port": "auto", "url": "http://127.0.0.1:${port}",
    "env": { "BACKEND_URL": "${services.backend.url}" },
    "setup": { "command": "npm install", "creates": "frontend/node_modules" },
    "build": "npm run build", "dist": "frontend/dist",          // production: OpenShell serves dist
    "proxy": { "/api": "${services.backend.url}" },             // used by the static server
    "depends_on": ["backend"]
  },

  "services": [
    {
      "name": "backend",                                        // [a-z0-9_-], "frontend" is reserved
      "command": ["python", "main.py"],                         // argv, or a shell string "npm start"
      "cwd": "backend",
      "port": "auto",                                           // -> $PORT, ${port}, ${services.backend.url}
      "healthcheck": "http://127.0.0.1:${port}/health",
      "build": ["javac", "-d", "build/classes", "src/Main.java"],  // compiled languages; dev: before every start
      "openapi": "/openapi.json",
      "env": { "LOG_LEVEL": "info" },
      "restart": true,                                          // on-failure (default) | "always" | false
      "watch": ["backend"],                                     // dev: restart on change
      "depends_on": ["db"],
      "platforms": ["desktop", "server"],                       // optional: not on Android (needs a toolchain)
      "background": "keep"                                      // mobile: "pause" stops it in the background
    },
    { "name": "db", "external": true, "healthcheck": { "tcp": "127.0.0.1:5432" } }
  ],

  "capabilities": [ /* see above */ ],
  "tasks": { "test": "pytest", "migrate": "python manage.py migrate" },   // agents: task_run (scope "system")
  "agents": {
    "permissions": { "development": ["read","ui","actions","services","files","code"], "production": ["read","ui","actions"] },
    "approval_required": ["system"]
  },
  "package": { "exclude": ["docs"], "targets": ["nsis"] }
}
```

Variables: `${port}`, `${services.<name>.port|url}`, `${frontend.url}`, `${project.root}`, `${mode}`, `${env.NAME}`.
Env vars every service gets: `PORT`, `OPENSHELL_DATA_DIR`, `OPENSHELL_MODE`, `OPENSHELL_SERVICE`,
`OPENSHELL_PROJECT_ROOT`, `OPENSHELL_CONTROL_URL`. Full reference: `openshell schema` or docs/configuration.md.

## Databases

`databases: [{ "name": "main", "migrations": "db/migrations", "agent_access": "write" }]` gives the app a SQLite file
in `OPENSHELL_DATA_DIR`. Schema changes: add `db/migrations/002_….sql` (never edit applied migrations; they run once,
in order, at startup). Access: services/functions `OPENSHELL_DB_MAIN` (+ `DATABASE_URL`), frontend `OpenShell.db.query
/ execute` (needs `actions` in `agents.sdk_permissions` to write), agents `db_query` / `db_execute`. Inspect with
`openshell call db_list`, `openshell call db_query '{"sql": "SELECT …"}'` or the console's data browser.

## Pitfalls

- **Hard-coded ports** (`8000`, `5173`) → conflicts. Use `"port": "auto"` and `$PORT`.
- **Binding to `0.0.0.0` or `localhost`** → use `127.0.0.1` (the health check and proxy use it; `localhost` may resolve to `::1`).
- **Writing data next to the code** in a watched folder → endless restarts in dev mode and lost data in installed apps. Use `OPENSHELL_DATA_DIR`.
- **Buffered output** in Java/C++/Python without OpenShell → empty logs. Flush.
- **Vite without `--strictPort`** silently picks another port → the window loads the wrong URL.
- **Absolute URLs to the backend in the frontend** → break in production. Use `/api` + proxy.
- **`frontend.dist` must exist for production** → run `openshell build`.
- **Installing global tools** or changing the user's system is not needed; templates have no dependencies except the language runtime and (for Vite templates) `npm install`, which OpenShell runs itself via `setup`.
- **Assuming a desktop** → apps can run on Android or a server too. Check `platform` in `app_info`; on Android only
  system tools and bundled programs start (see docs/platforms.md).
- **Do not edit `.openshell/`** – it is runtime state (logs, audit log, `runtime.json` with the control token).

## Definition of done

- [ ] Only the building blocks the task needs (no backend just to store data, no UI for a pure agent tool)
- [ ] `openshell doctor` shows no errors
- [ ] `openshell dev --headless` → `openshell status`: all services `running`; `openshell logs --errors` clean
- [ ] Main user flows verified through the API and, where possible, `ui_snapshot` / `ui_click` / `ui_fill`
- [ ] Capabilities for the app's main functions; each tested with `openshell call cap_<name>`
- [ ] Data stored in `OPENSHELL_DATA_DIR`
- [ ] Production check: `openshell build` + `openshell start --headless` works
- [ ] `README.md` of the app explains how to run it (`openshell dev`) and what agents can do
- [ ] Optional: `openshell package` produces an installer

## Reference

| Need | Where |
|---|---|
| All config fields | `openshell schema`, docs/configuration.md |
| Agent tools, permissions, MCP | docs/agents.md, `openshell tools` |
| Languages (Java, C++, Go, Rust, .NET) | docs/languages.md |
| Installers | docs/installers.md |
| Frontend SDK | docs/sdk.md, `<control_url>/sdk.js` |
| HTTP control API | docs/control-api.md |
