# Building blocks: build only what you need

An OpenShell app is assembled from four independent building blocks. Every block is optional – use one, several or
all of them:

| Block | What it is | Add it |
|---|---|---|
| **Frontend** | A web UI in the native window (static files, a dev server or a URL) | `openshell add frontend` |
| **Backend** | An HTTP service in any language, supervised by OpenShell | `openshell add backend [--lang python\|node]` |
| **Database** | A built-in SQLite database with migrations, managed by OpenShell | `openshell add database [name]` |
| **Functions** | Functions for AI agents (MCP tools) implemented as scripts – no server | `openshell add function <name> [--lang python\|node]` |

Start a new app with any combination:

```bash
openshell create my-app --with frontend,database          # UI that stores data – no backend
openshell create my-app --with database,functions         # an AI interface over a database – no UI, no server
openshell create my-app --with backend                    # a supervised API service
openshell create my-app --with frontend,backend,database,functions
openshell create my-app                                   # in a terminal: asks which blocks you want
```

Blocks can be added later in any order; they connect themselves (a frontend added to an app with a backend proxies
`/api` to it, the backend receives the database path, the frontend gets write access to the database, …).

## Typical combinations

| You want | Blocks | How it works |
|---|---|---|
| A desktop UI with local storage | frontend + database | The page uses `OpenShell.db.query/execute` from the [SDK](sdk.md). |
| An AI interface to your data | database + functions | Agents get your functions plus `db_query` / `db_execute` (as allowed by `agent_access`). |
| Tools for AI agents only | functions | Each function is a script: JSON in, JSON out. The app has no UI and no server. |
| A background service | backend | Runs headless or in the tray (`"window": {"enabled": false}`). |
| A classic web app as desktop app | frontend + backend (+ database) | As before; the backend can use `OPENSHELL_DB_MAIN`. |

Apps **without a frontend** show OpenShell's console in the window: services, databases (with a data browser),
functions (with a "Run" button), logs and the commands to connect AI agents. Set `"window": {"enabled": false}` to
run only in the tray instead.

## Frontend

```json
"frontend": { "dist": "web", "watch": ["web"] }
```

Any frontend works – static files, a Vite/Next dev server (`command` + `url`) or an external URL; see
[configuration.md](configuration.md#frontend). The starter page from `openshell add frontend` adapts to the other
blocks: it shows a backend call, a notes list from the database and the app's functions.

## Backend

```json
{
  "name": "backend",
  "command": ["python", "main.py"],
  "cwd": "backend",
  "port": "auto",
  "healthcheck": "http://127.0.0.1:${port}/health",
  "watch": ["backend"]
}
```

Any language ([languages.md](languages.md)). Read `PORT`, bind `127.0.0.1`, answer `/health`.

## Database

```json
"databases": [
  { "name": "main", "migrations": "db/migrations", "agent_access": "write" }
]
```

- **File**: `<name>.db` in the app's data directory (`OPENSHELL_DATA_DIR`, per user for installed apps). Override
  with `"path"` (relative to the data directory, or absolute).
- **Migrations**: every `*.sql` file in `migrations` is applied once, in file-name order, inside a transaction.
  Add new files (`002_add_tags.sql`) – never edit applied ones.
- **Services and functions** get the path as `OPENSHELL_DB_<NAME>` (e.g. `OPENSHELL_DB_MAIN`) and the main database
  as `DATABASE_URL=sqlite:///…`. Any SQLite library works (Python `sqlite3`, `better-sqlite3`, JDBC, …).
- **Frontend**: `OpenShell.db.query(sql, params)` / `OpenShell.db.execute(sql, params)`. Writing needs `actions` in
  `agents.sdk_permissions` (`openshell add` sets it).
- **Agents**: `db_list`, `db_schema`, `db_query` (read-only, enforced by SQLite) and `db_execute`, controlled per
  database by `agent_access`: `none`, `read` (default) or `write`. The schema is also an MCP resource
  (`openshell://db/<name>/schema`).
- WAL mode and a busy timeout are set, so the shell, services and functions can use the file at the same time.
- The console's data browser shows every table.

PostgreSQL, MySQL & co. run as services (`"external": true` with a TCP health check, or a container command) and are
used from your backend.

## Functions

```json
"capabilities": [
  {
    "name": "summarize_notes",
    "description": "Summarize all open notes",
    "permission": "read",
    "input_schema": { "type": "object", "properties": { "limit": { "type": "integer" } } },
    "run": ["python", "tools/summarize_notes.py"]
  }
]
```

```python
import json, os, sqlite3, sys

args = json.load(sys.stdin)                       # input object
con = sqlite3.connect(os.environ["OPENSHELL_DB_MAIN"])
rows = con.execute("SELECT title FROM notes WHERE done = 0 LIMIT ?", (args.get("limit", 20),)).fetchall()
print(json.dumps({"open": [r[0] for r in rows]})) # result
```

- Input: JSON object on stdin (also in `OPENSHELL_INPUT`). Output: JSON or text on stdout. Errors: message on stderr
  and a non-zero exit code. `"run": {"command": …, "cwd": …, "env": {…}, "timeout_ms": 60000}` for more control.
- Any language: `["node", "tools/x.mjs"]`, `["java", "-cp", "build", "Tool"]`, a compiled binary, …
- Agents see functions as MCP tools in the app profile (`openshell mcp --app`) under their own names, and as
  `cap_<name>` in the developer profile. The frontend calls them with `OpenShell.invoke(name, input)`, the console
  has a "Run" button, the CLI `openshell call cap_<name> '<json>'`.
- `stderr` output is in the logs (`openshell logs fn:<name>`).

Other function sources, also listed as capabilities: HTTP endpoints of a backend (`"http"`), declared tasks
(`"task"`), operations imported from an OpenAPI document (`"openapi"` on a service) and actions registered by the
frontend (`OpenShell.agent.registerAction`). See [agents.md](agents.md#capabilities-the-applications-own-functions).
