# CLI reference

Global option: `-p, --project <dir>` – project directory (default: current directory or the nearest parent with an
`openshell.json`). Environment: `OPENSHELL_LOG` (log filter, e.g. `debug`), `OPENSHELL_DESKTOP` (path of the desktop
binary), `NO_COLOR`.

## Project setup

| Command | Description |
|---|---|
| `openshell create <name> [-t <template>]` | New project from a template (`vanilla-python` default, `react-python`, `react-node`, `vue-python`, `svelte-go`, `vanilla-java`, `vanilla-cpp`, `custom`). `--list` shows all. |
| `openshell init [--name <n>] [--toml] [--force]` | Generate a config for an existing project (auto-detection). |
| `openshell doctor` | Validate config, commands, directories, ports, desktop binary, WebView2, toolchain. Exit code 1 on errors. |
| `openshell schema` | Print the JSON Schema of the configuration. |
| `openshell build` | Production build: `services[].build` (C++, Java, …) and `frontend.build` (with `frontend.setup` if needed). |
| `openshell package [-t nsis\|msi\|dmg\|deb\|…] [-o dir] [--skip-build]` | Build a native installer – see [installers.md](installers.md). |

## Running

| Command | Description |
|---|---|
| `openshell dev [--headless]` | Development mode. Opens the desktop window (or runs headless for the browser) and streams logs. Ctrl+C stops everything. |
| `openshell start [--headless] [--detach]` | Production mode. `--detach` returns immediately and keeps the app running in the background. |
| `openshell stop` | Stop the whole shell (all services, window). |
| `openshell stop <service>` | Stop one service. |
| `openshell run <service>` | Start a stopped service (and its dependencies). |
| `openshell restart <service>` | Restart a service and wait until it is healthy. |

## Inspecting

| Command | Description |
|---|---|
| `openshell status [--json]` | Mode, URLs, services (state, pid, port, CPU, memory, restarts, errors), bridged pages, pending approvals. |
| `openshell logs [service] [-f] [-n 100] [--errors] [--grep <text>]` | Logs of all or one service. |
| `openshell audit [-n 30]` | Recent agent actions. |

## Agents

| Command | Description |
|---|---|
| `openshell mcp [--app] [--start]` | MCP server over stdio. `--app`: only the application's own functions (agents that *use* the app). `--start`: start the app headless if it is not running. |
| `openshell tools [--json]` | Tools with scope and current access (allowed / approval / denied). |
| `openshell call <tool> ['<json args>']` | Call a tool as agent `cli`, e.g. `openshell call ui_click '{"text":"Save"}'`. |
| `openshell approvals` | Pending approval requests. |
| `openshell approve <id>` / `openshell deny <id>` | Answer an approval request. |
