# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- `AGENTS.md` / `CLAUDE.md` / `llms.txt` and an AI development guide (`openshell guide`) so AI coding agents know how
  to build applications on OpenShell; every new project gets its own `AGENTS.md`.
- Release workflow and install scripts.

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
  `mcp`, `tools`, `call`, `approvals`, `approve`, `deny`, `audit`, `schema`.
- `openshell package`: NSIS/MSI, DMG, DEB/RPM/AppImage installers with app name and icon.
- Templates: vanilla-python, react-python, react-node, vue-python, svelte-go, vanilla-java, vanilla-cpp, custom.
- Frontend SDK `@openshell/sdk`.

[Unreleased]: https://github.com/RobinsSuperSchmiede/EDA/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/RobinsSuperSchmiede/EDA/releases/tag/v0.1.0
