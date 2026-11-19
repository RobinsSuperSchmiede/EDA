# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/RobinsSuperSchmiede/EDA/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/RobinsSuperSchmiede/EDA/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/RobinsSuperSchmiede/EDA/releases/tag/v0.1.0
