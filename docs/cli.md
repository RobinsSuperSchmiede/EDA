# CLI reference

Global option: `-p, --project <dir>` – project directory (default: current directory or the nearest parent with an
`openshell.json`). Environment: `OPENSHELL_LOG` (log filter, e.g. `debug`), `OPENSHELL_DESKTOP` (path of the desktop
binary), `NO_COLOR`, `OPENSHELL_APPS_DIR` (installed bundles), `OPENSHELL_ANDROID_MODE` (`termux` / `app`, see
[platforms.md](platforms.md#termux-apps-on-the-phone)).

## Project setup

| Command | Description |
|---|---|
| `openshell create <name> [-t <template>]` | New project from a template (`vanilla-python` default, `react-python`, `react-node`, `vue-python`, `svelte-go`, `vanilla-java`, `vanilla-cpp`, `custom`). `--list` shows all. |
| `openshell create <name> --with frontend,backend,database,functions [--lang python\|node]` | New project composed of [building blocks](building-blocks.md). Without `--template`/`--with` the terminal asks. |
| `openshell add frontend \| backend [--lang …] \| database [name] \| function <name> [--lang …]` | Add a building block to the current app (files + `openshell.json`). |
| `openshell init [--name <n>] [--toml] [--force]` | Generate a config for an existing project (auto-detection) and an `AGENTS.md` for coding agents if none exists. |
| `openshell doctor` | Validate config, commands, directories, ports, desktop binary, WebView2, toolchain. Exit code 1 on errors. |
| `openshell schema` | Print the JSON Schema of the configuration. |
| `openshell guide` | Print the guide for AI coding agents that build apps on OpenShell (docs/ai/app-development.md). |
| `openshell build` | Production build: `services[].build` (C++, Java, …) and `frontend.build` (with `frontend.setup` if needed). |
| `openshell package [-t nsis\|msi\|dmg\|deb\|…\|bundle] [-o dir] [--skip-build]` | Build a native installer, or with `-t bundle` the portable app bundle (`.osapp`, also on Android/Termux) – see [installers.md](installers.md). |
| `openshell release workflow [--force]` | Write `.github/workflows/openshell-release.yml`: a tag push builds Windows, macOS, Linux installers and the `.osapp` bundle and publishes them with a download page. |
| `openshell release manifest [dir] [--base-url <url>]` | Write `downloads.json` (platform, file, size, SHA-256) and a download page (`index.html`) for the files in `dir` (default `dist`). |
| `openshell install <file.osapp> [--start]` | Install an app bundle into `~/.openshell/apps/<identifier>` (`OPENSHELL_APPS_DIR`); app data survives updates. |
| `openshell apps` | List installed app bundles. |
| `openshell update` | Check `package.update_url` for a newer release and print the download for this system. |

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
| `openshell remote` | Remote gateway: URLs, MCP endpoint, TLS fingerprint, allowlist, exposed services, paired devices, blocked addresses, shared token and an MCP client configuration (`runtime.remote`). |
| `openshell remote pair <device> [--no-qr]` | Create a token for one device (shown once) with a QR code / pairing URI and links to exposed services. |
| `openshell remote devices` | Paired devices with last use. |
| `openshell remote revoke <device>` | Revoke one device's token. |
| `openshell remote unblock <address>` | Lift the block of an address after too many failed logins. |
| `openshell lifecycle <foreground\|background\|low-memory>` | Simulate a mobile lifecycle event, e.g. to test `"background": "pause"`. |

## Agents

| Command | Description |
|---|---|
| `openshell mcp [--app] [--start]` | MCP server over stdio. `--app`: only the application's own functions (agents that *use* the app). `--start`: start the app headless if it is not running. |
| `openshell tools [--json]` | Tools with scope and current access (allowed / approval / denied). |
| `openshell call <tool> ['<json args>']` | Call a tool as agent `cli`, e.g. `openshell call ui_click '{"text":"Save"}'`. |
| `openshell approvals` | Pending approval requests. |
| `openshell approve <id>` / `openshell deny <id>` | Answer an approval request. |
