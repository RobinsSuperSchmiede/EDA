# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Building blocks**: frontend, backend, database and agent functions are independent and freely combinable.
  `openshell add frontend | backend [--lang] | database [name] | function <name>` adds a block to an app;
  `openshell create <name> --with frontend,database,…` composes a new one (interactive selection in a terminal).
- **Built-in databases** (`databases` in openshell.json): SQLite files in the app's data directory, migrations,
  `OPENSHELL_DB_<NAME>` / `DATABASE_URL` for services, `OpenShell.db.query/execute/schema/list` for the frontend,
  agent tools `db_list`, `db_schema`, `db_query` (read-only), `db_execute`, per-database `agent_access`.
- **Script functions**: capabilities with `"run"` execute a program per call (JSON on stdin, result on stdout) – an app
  can offer an AI interface without any server. `OpenShell.invoke(name, input)` in the SDK.
- Apps without a frontend show a console in the window (services, database browser, functions with "Run", agent
  commands, logs); `window.enabled: false` runs an app in the tray only.
- MCP resources (status, config, logs, capabilities, database schemas) and `notifications/tools/list_changed` from
  `openshell mcp`.
- `runtime.launch_at_login` for installed apps.
- `openshell status` and `openshell doctor` show databases and functions.
- **Platform-neutral core** for desktop, server and Android: a `RuntimeAdapter` builds every command and reports the
  platform's capabilities (`platform` in `/v1/status`, `/v1/health` and the agent tool `app_info`). The
  `AndroidAdapter` runs system tools and programs bundled with the app and explains what to use instead of anything
  else. The core and the CLI build for Android (new CI job); verified headless on an Android 16 emulator.
- `services[].platforms` (`desktop`, `android`, `server`) and `services[].background` (`keep` / `pause`).
- App lifecycle events (`foreground`, `background`, `low_memory`, `power_save`, `network`) via `Shell::lifecycle`,
  `POST /v1/lifecycle` and `openshell lifecycle`; paused services resume in the foreground.
- **Remote gateway** (`runtime.remote`): other devices use the app's functions over MCP with a pairing token;
  `openshell remote` prints URL, token and an MCP client configuration. See `docs/platforms.md`.
- Landing page in `website/`: static, ASCII field of code glyphs, architecture diagram, product gallery and a
  platforms section (desktop, server, Android, remote gateway). `dev.html` adds a customize bar to try colour themes
  (green, light blue, red, purple, amber, mono), hero variants, surfaces and background settings; `index.html` is the
  production page.

## [0.2.1] - 2026-11-19

### Fixed
- HTML5 drag and drop works in the app window: Tauri's drag and drop handler is off, so `dragover` and `drop` reach
  the page (on Windows they did not). Files dropped on the window arrive as in a browser (`dataTransfer.files`).

## [0.2.0] - 2026-11-19

### Added
- `window.title_bar: "overlay"`: the frontend draws its own title bar. Windows and Linux get a window without the
  system title bar (edges still resize), macOS keeps its window buttons over the content. The desktop window offers
  `window.__OPENSHELL__.window` (`minimize`, `toggleMaximize`, `close`, `isMaximized`, `showMenu`, plus `title_bar`
  and `os`); elements with `data-tauri-drag-region` move the window. The window permissions are granted at runtime to
  the app's frontend origin only. OpenShell's loading screen shows a minimal title bar in this mode.
- The title bar menu (services, reload, window actions) is available on every platform via `showMenu()`.

## [0.1.0] - 2026-11-17

First public alpha.

### Added
- Tauri 2 desktop shell: window, WebView, loading screen, runtime panel, tray, notifications, approval dialogs,
  single instance, title bar right-click menu with live service monitoring (Windows).
- Runtime manager: services in dependency order, setup and build steps, HTTP/TCP health checks, restart policies with
  backoff, logs, dynamic ports and `${...}` variables, static frontend server with reverse proxy, file watching,
  CPU/memory metrics, process-tree cleanup.
- Agent Interaction Layer: 28 tools over MCP (stdio and HTTP) and HTTP/JSON, developer and app profiles, semantic UI
  bridge, capabilities (config, OpenAPI import, frontend actions), permission scopes, approvals, audit log, AI provider
  discovery.
- CLI: `init`, `create`, `dev`, `start`, `stop`, `run`, `restart`, `status`, `logs`, `doctor`, `build`, `package`,
  `mcp`, `tools`, `call`, `approvals`, `approve`, `deny`, `audit`, `schema`, `guide`.
- `openshell package`: NSIS/MSI, DMG, DEB/RPM/AppImage installers with app name and icon.
- Templates: vanilla-python, react-python, react-node, vue-python, svelte-go, vanilla-java, vanilla-cpp, custom.
- Frontend SDK `@openshell/sdk`.
- AI-harness support: `AGENTS.md`, `CLAUDE.md`, `llms.txt` / `llms-full.txt`, the AI app development guide
  (`openshell guide`) and a Claude Code skill; every new project gets its own `AGENTS.md`.
- Release workflow with prebuilt binaries for Windows, macOS (arm64, x86_64) and Linux, plus install scripts.

[Unreleased]: https://github.com/RobinsSuperSchmiede/EDA/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/RobinsSuperSchmiede/EDA/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/RobinsSuperSchmiede/EDA/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/RobinsSuperSchmiede/EDA/releases/tag/v0.1.0
