# Tutorial: build a complete OpenShell app

In about 20 minutes you build **Bookmarks**, a small desktop app with a Python backend and a plain HTML frontend,
make it usable for AI agents and package it as an installer. Everything here works the same with the React, Vue,
Svelte, Java or C++ templates.

Prerequisites: `openshell` and `openshell-desktop` on your `PATH` ([getting started](getting-started.md)), Python 3.

## 1. Create the project

```bash
openshell create bookmarks
cd bookmarks
openshell dev
```

A window opens with the template's todo list. The terminal shows the logs of both services:

```
 backend │ starting: python main.py
 backend │ ready after 0.4s
frontend │ serving …/web at http://127.0.0.1:53121/
```

Leave it running. The project looks like this:

```
openshell.json      what OpenShell runs (open it – it is short)
backend/main.py     JSON API, Python standard library
web/                index.html, app.js, style.css – served by OpenShell, /api is proxied to the backend
AGENTS.md           instructions for AI coding agents (see step 6)
```

## 2. Add a backend endpoint

Replace the todo logic in `backend/main.py` with bookmarks. The essentials of the OpenShell contract are already in
the template:

```python
PORT = int(os.environ.get("PORT", "8000"))                       # OpenShell assigns the port
DATA_DIR = os.environ.get("OPENSHELL_DATA_DIR") or "../data"     # per-app data directory
# ...
if path == "/health":                                            # health check used by OpenShell
    return self.send_json(200, {"status": "ok"})
```

Add two routes (sketch):

```python
if path == "/api/bookmarks" and self.command == "GET":
    q = parse_qs(urlparse(self.path).query).get("q", [""])[0].lower()
    items = [b for b in load() if q in b["title"].lower() or q in b["url"].lower()]
    return self.send_json(200, items)

if path == "/api/bookmarks" and self.command == "POST":
    body = self.read_json()
    item = {"id": next_id(), "title": body["title"], "url": body["url"]}
    save(load() + [item])
    return self.send_json(201, item)
```

Save the file: the log shows `files changed: backend\main.py` → `restarting` → `ready`. If you made a mistake, the
error is in `openshell logs backend --errors` and the service restarts again on the next save.

## 3. Update the frontend

Edit `web/index.html` and `web/app.js` to list `GET /api/bookmarks` and post new ones. On save, OpenShell reloads the
window (`"watch": ["web"]` in the config). With a dev server template (React/Vue/Svelte) the dev server hot-reloads
instead.

Useful while developing:

```bash
openshell status                 # state, pid, port, CPU, memory of each service
openshell logs -f                # follow all logs
openshell restart backend
```

Right-click the window's title bar (Windows) or use the tray icon for the same overview.

## 4. Make the app usable for AI agents

Agents should be able to *use* Bookmarks without clicking through the UI. Declare its functions as **capabilities**
in `openshell.json`:

```json
"capabilities": [
  {
    "name": "search_bookmarks",
    "description": "Search bookmarks by text in title or URL",
    "permission": "read",
    "input_schema": { "type": "object", "properties": { "q": { "type": "string" } } },
    "http": { "service": "backend", "method": "GET", "path": "/api/bookmarks" }
  },
  {
    "name": "add_bookmark",
    "description": "Save a bookmark",
    "permission": "actions",
    "input_schema": {
      "type": "object",
      "properties": { "title": { "type": "string" }, "url": { "type": "string" } },
      "required": ["title", "url"]
    },
    "http": { "service": "backend", "method": "POST", "path": "/api/bookmarks" }
  }
]
```

Try them right away:

```bash
openshell call cap_add_bookmark '{"title": "Tauri", "url": "https://tauri.app"}'
openshell call cap_search_bookmarks '{"q": "tauri"}'
```

Alternatives:

- **OpenAPI:** if your backend publishes an OpenAPI document (FastAPI does at `/openapi.json`), set
  `"openapi": "/openapi.json"` on the service instead – every endpoint becomes a capability automatically.
- **Frontend actions:** UI-only functions (e.g. "show the bookmarks of folder X") can be registered from the page with
  `OpenShell.agent.registerAction(...)` – see the template's `web/app.js` and the [SDK](sdk.md).

## 5. Connect an assistant

```bash
claude mcp add bookmarks -- openshell mcp --app --start --project /path/to/bookmarks
```

Ask: *"Save https://modelcontextprotocol.io as 'MCP spec' in my bookmarks."* The assistant sees exactly
`app_status`, `search_bookmarks` and `add_bookmark` – nothing else. With `--start`, the app is started in the
background if it is not running.

To let a coding agent work *on* the app, connect the full profile instead: `openshell mcp --project …`
([agents.md](agents.md)).

## 6. Let an AI coding agent extend the app

The project contains an `AGENTS.md` (and `CLAUDE.md`) that tells coding agents how OpenShell apps work, how to verify
changes (`openshell status`, `openshell call ui_snapshot`, `openshell logs --errors`) and how to expose new functions.
Open the folder in Claude Code, Codex, Cursor, … and ask:

> *Add folders to the bookmarks app: each bookmark can be in a folder, the UI shows a folder list, and agents can
> list folders and move bookmarks.*

## 7. Ship it

Give the app a version and an icon in `openshell.json`:

```json
"app": { "name": "Bookmarks", "id": "com.example.bookmarks", "version": "1.0.0", "icon": "icon.png" }
```

```bash
openshell start            # try production mode first
openshell package          # → .openshell/package/bin/bundle/nsis/Bookmarks_1.0.0_x64-setup.exe (Windows)
```

The installed app stores its data in the user's data directory, runs in production mode (agents get only
`read`, `ui` and `actions` by default) and can be used by agents with `bookmarks.exe --mcp`.
More in [installers.md](installers.md).

## Where to go next

- [Configuration reference](configuration.md) – databases as external services, workers, tasks, proxies, modes
- [Languages](languages.md) – Java/Spring Boot, C++/CMake, Go, Rust, .NET backends
- [Agents](agents.md) – permissions, approvals, UI automation, audit log
