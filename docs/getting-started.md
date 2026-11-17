# Getting started

## 1. Install

OpenShell consists of two binaries:

- `openshell` – the CLI (also runs the headless runtime and the MCP server)
- `openshell-desktop` – the desktop shell (Tauri 2)

Until prebuilt releases are published, build them from source:

```bash
git clone https://github.com/RobinsSuperSchmiede/EDA.git
cd EDA
cargo build --release
```

Put `target/release` on your `PATH` (both binaries must be in the same directory, or set `OPENSHELL_DESKTOP` to the
desktop binary). Check your environment:

```bash
openshell doctor
```

## 2a. New project

```bash
openshell create my-app                         # HTML/JS + Python stdlib, no installs
openshell create my-app --template react-python # React (Vite) + Python
openshell create --list                         # show all templates
cd my-app
openshell dev
```

## 2b. Existing project

```bash
cd my-existing-project
openshell init        # detects frontend (Vite, Next, CRA, Angular, Vue CLI, static) and backend (Python, Node, Go, Rust)
openshell doctor      # verifies commands, directories, ports
openshell dev
```

`init` writes a starting point – review the generated `openshell.json`, especially ports and health checks.
A typical configuration:

```json
{
  "version": 1,
  "app": { "name": "My App" },
  "frontend": {
    "command": "npm run dev -- --port ${port} --strictPort",
    "cwd": "frontend",
    "port": "auto",
    "url": "http://127.0.0.1:${port}",
    "setup": { "command": "npm install", "creates": "frontend/node_modules" },
    "build": "npm run build",
    "dist": "frontend/dist",
    "proxy": { "/api": "${services.backend.url}" },
    "env": { "BACKEND_URL": "${services.backend.url}" },
    "depends_on": ["backend"]
  },
  "services": [
    {
      "name": "backend",
      "command": ["python", "main.py"],
      "cwd": "backend",
      "port": "auto",
      "healthcheck": "http://127.0.0.1:${port}/health",
      "restart": true,
      "watch": ["backend"]
    }
  ]
}
```

Your backend reads its port from the `PORT` environment variable. The frontend dev server proxies `/api` to
`BACKEND_URL`; in production OpenShell serves `frontend/dist` itself and proxies `/api`.

## 3. Run

| Command | What happens |
|---|---|
| `openshell dev` | Development mode: window, file watching, broad agent permissions, logs in the terminal. |
| `openshell dev --headless` | Same without a window – open the printed URL in your browser (browser mode). |
| `openshell start` | Production mode (uses `frontend.dist` when configured). |
| `openshell start --headless --detach` | Background runtime without window. |
| `openshell status` / `logs -f` / `restart backend` / `stop` | Manage the running app from another terminal. |

While `openshell dev` runs:

- edit backend files → the backend restarts (`watch`),
- edit frontend files → the dev server hot-reloads, or OpenShell reloads the window for static frontends,
- a crashing service is restarted with backoff; after 5 crashes per minute it is marked `failed`,
- `openshell build` + `openshell start` runs the production build.

## 4. Connect an AI agent

```bash
claude mcp add my-app -- openshell mcp --project /path/to/my-app
```

See [agents.md](agents.md) for other agents, the tool list and permissions.
