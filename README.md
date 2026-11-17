<div align="center">

<img src="docs/assets/logo.png" width="96" alt="OpenShell logo" />

# OpenShell

**Turn any web app into a native desktop app – managed by a runtime that people *and* AI agents can operate.**

[![CI](https://github.com/RobinsSuperSchmiede/EDA/actions/workflows/ci.yml/badge.svg)](https://github.com/RobinsSuperSchmiede/EDA/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
![Rust](https://img.shields.io/badge/Rust-stable-orange?logo=rust)
![Tauri 2](https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri)
![MCP](https://img.shields.io/badge/MCP-ready-6E56CF)
![Status](https://img.shields.io/badge/status-alpha-yellow)

[Quick start](#quick-start) · [Usage](#using-openshell) · [AI agents](#ai-agents) · [Documentation](docs/README.md) · [Roadmap](ROADMAP.md) · [Deutsch](README.de.md)

</div>

---

OpenShell is an open-source desktop shell built on **Rust and Tauri 2**. You bring a normal web frontend and any
backend – Python, Node.js, Java, C++, Go, Rust, .NET – and describe them in one `openshell.json`. OpenShell runs
everything as a native desktop app: it starts and supervises your services, shows the frontend in a native window,
reloads on code changes, and builds installers. You never write desktop or process-management code.

What makes it different: every OpenShell app is **agent-native**. AI agents (Claude, OpenAI, local models via Ollama /
LM Studio, any MCP client) can *use* the app through its own functions and *develop* it through a controlled,
permission-checked interface – without clicking through the UI.

> **Build the shell once. Build applications on top of it. Let agents use and evolve them.**

## Contents

- [Quick start](#quick-start)
- [How it works](#how-it-works)
- [Using OpenShell](#using-openshell)
- [AI agents](#ai-agents)
- [Features](#features)
- [Documentation](#documentation)
- [Project status](#project-status)
- [Repository layout](#repository-layout)
- [Contributing](#contributing) · [License](#license)

## Quick start

**1. Install** – OpenShell consists of two binaries: `openshell` (CLI) and `openshell-desktop` (the shell).

```bash
# from source (requires Rust; on Linux also the Tauri system packages, see docs/getting-started.md)
git clone https://github.com/RobinsSuperSchmiede/EDA.git openshell
cd openshell
cargo build --release
# then put target/release on your PATH (both binaries must stay in the same folder)
```

Prebuilt binaries: once a release is published, use `scripts/install.sh` (macOS/Linux) or `scripts/install.ps1`
(Windows), or download them from the [releases page](https://github.com/RobinsSuperSchmiede/EDA/releases).

**2. Create and run an app**

```bash
openshell create my-app          # HTML/JS frontend + Python backend, no other installs needed
cd my-app
openshell dev                    # opens the native window, streams logs, hot-reloads on changes
```

**3. Make it yours** – edit `web/` or `backend/`. The window reloads and the backend restarts automatically.
Other stacks: `openshell create --list` (React, Vue, Svelte, Node.js, Go, Java, C++).

**4. Ship it**

```bash
openshell package                # → Windows setup.exe / macOS .dmg / Linux .deb + .AppImage
```

**5. Let AI use it**

```bash
claude mcp add my-app -- openshell mcp --app --start --project .
# "Add a todo 'call the bank' to my app" – the agent calls the app's own add_todo function
```

## How it works

```
             people                                    AI agents (Claude, OpenAI, Ollama, LM Studio, any MCP client)
                │                                                    │
        ┌───────▼────────────────────────────────────────────────────▼────────┐
        │  OpenShell desktop shell (Tauri 2)                                    │
        │  native window + WebView · tray · notifications · approval dialogs   │
        │  ┌───────────────────────────┐   ┌─────────────────────────────────┐ │
        │  │ Runtime Manager           │   │ Agent Interaction Layer          │ │
        │  │ start · health · restart  │   │ MCP + HTTP tools · UI bridge     │ │
        │  │ build · logs · metrics    │   │ capabilities · OpenAPI import    │ │
        │  │ ports · file watching     │   │ permissions · approvals · audit  │ │
        │  └─────────────┬─────────────┘   └─────────────────────────────────┘ │
        └────────────────┼─────────────────────────────────────────────────────┘
                         │ manages separate processes – never compiled into the shell
     ┌────────────┬──────┴──────┬────────────┬──────────────┐
     ▼            ▼             ▼            ▼              ▼
  frontend     backend       database     worker      local AI server
 React/Vue/  Python/Node/   SQLite/PG/    any         Ollama, LM Studio
 Svelte/HTML Java/C++/Go…   external…
```

- **The shell is not your app.** It is the runtime and management layer around it. Your frontend and backend stay
  normal, independent projects – change them (by hand or by an agent) and the running app picks it up. The shell is
  never rebuilt.
- **One config file** describes the app: which frontend to show, which services to run, how to check their health,
  what agents may do. See the [configuration reference](docs/configuration.md).
- **One runtime, three modes:** development (hot reload, broad agent access), production (stable, restricted agents)
  and headless (no window, background service).

## Using OpenShell

### Turn an existing web app into a desktop app

```bash
cd my-existing-project
openshell init        # detects Vite/Next/CRA/Angular/Vue frontends and Python/Node/Go/Rust/Java/C++ backends
openshell doctor      # checks commands, folders, ports, toolchain
openshell dev
```

A typical `openshell.json`:

```json
{
  "version": 1,
  "app": { "name": "My App", "version": "1.0.0", "icon": "assets/icon.png" },
  "frontend": {
    "command": "npm run dev -- --port ${port} --strictPort",
    "cwd": "frontend",
    "port": "auto",
    "url": "http://127.0.0.1:${port}",
    "setup": { "command": "npm install", "creates": "frontend/node_modules" },
    "build": "npm run build",
    "dist": "frontend/dist",
    "proxy": { "/api": "${services.backend.url}" },
    "depends_on": ["backend"]
  },
  "services": [
    {
      "name": "backend",
      "command": ["python", "main.py"],
      "cwd": "backend",
      "port": "auto",
      "healthcheck": "http://127.0.0.1:${port}/health",
      "openapi": "/openapi.json",
      "watch": ["backend"]
    }
  ]
}
```

Your backend reads its port from `PORT`, stores data in `OPENSHELL_DATA_DIR` and answers `/health`. That is the whole
contract – see the [app contract](docs/ai/app-development.md#the-app-contract).

### Develop

| Task | Command |
|---|---|
| Run with window / without window (browser) | `openshell dev` / `openshell dev --headless` |
| What is running? (state, pid, port, CPU, memory) | `openshell status` |
| Logs | `openshell logs backend -f --errors` |
| Restart / stop / start a service | `openshell restart backend` · `openshell stop worker` · `openshell run worker` |
| Production mode, in the background | `openshell start --detach` |
| Stop everything | `openshell stop` |

In the window, **right-click the title bar** (Windows) or use the **tray icon** to monitor every service – state, CPU,
memory, uptime – and to restart services, open logs or copy the MCP command for agents.

### Any language

| Backend | Example `command` | `build` |
|---|---|---|
| Python | `["python", "main.py"]` | – |
| Node.js | `["node", "server.js"]` | – |
| Java / Spring Boot | `["mvnw", "spring-boot:run"]` | – (or `mvnw package` + `java -jar`) |
| C++ (CMake) | `["build/bin/server"]` | `cmake -S . -B build && cmake --build build` |
| Go / Rust / .NET | `["bin/server"]` | `go build` / `cargo build` / `dotnet build` |

Compiled services rebuild and restart automatically on every change in development mode. Details:
[docs/languages.md](docs/languages.md).

### Ship

`openshell package` builds the app (`frontend.build`, `services[].build`) and creates a native installer with your
app's name and icon – NSIS/MSI on Windows, `.app`/`.dmg` on macOS, `.deb`/`.rpm`/`.AppImage` on Linux. Installed apps
keep their data per user and are MCP servers themselves. See [docs/installers.md](docs/installers.md).

## AI agents

OpenShell offers agents two profiles over [MCP](https://modelcontextprotocol.io) (and plain HTTP/JSON):

| | **App profile** – *use* the app | **Full profile** – *develop* the app |
|---|---|---|
| For | assistants working with your application | coding agents building / fixing it |
| Tools | the app's own functions: declared capabilities, imported OpenAPI operations, frontend actions, `app_status` | 28 tools: status, logs, services, files, UI automation, tasks, capabilities |
| Connect | `openshell mcp --app --start` · installed app: `my-app.exe --mcp` | `openshell mcp` |

```bash
claude mcp add my-app     -- openshell mcp --app --start --project /path/to/my-app   # use
claude mcp add my-app-dev -- openshell mcp --project /path/to/my-app                 # develop
```

Every call is checked against permission scopes (`read`, `ui`, `actions`, `services`, `files`, `code`, `system`),
critical actions can require a human approval dialog, and everything is written to an audit log.
Read more in [docs/agents.md](docs/agents.md).

### Build a new app with an AI coding agent

This repository is prepared for AI harnesses (Claude Code, Codex, Cursor, Copilot, Aider, …):

- [`AGENTS.md`](AGENTS.md) / [`CLAUDE.md`](CLAUDE.md) – entry points for agents working in this repo,
- [`docs/ai/app-development.md`](docs/ai/app-development.md) – the complete guide for building apps on OpenShell
  (also printed by `openshell guide`),
- [`llms.txt`](llms.txt) / [`llms-full.txt`](llms-full.txt) – the documentation in LLM-friendly form,
- every project created with `openshell create` / `openshell init` gets its own `AGENTS.md` and `CLAUDE.md`.

So you can simply say to your coding agent:

> *Build me a desktop app with OpenShell: a recipe manager with a React frontend and a Python backend. Recipes should
> be searchable, and AI agents should be able to add and find recipes.*

The agent finds the contract, the templates, the verification commands (`openshell status`, `openshell call
ui_snapshot`, …) and the packaging steps on its own. See [docs/ai/README.md](docs/ai/README.md).

## Features

- **Runtime manager** – dependency-ordered start, one-time setup (`npm install`), build steps, HTTP/TCP health checks,
  restart policies with backoff and crash-loop protection, dynamic ports and `${services.x.url}` variables, logs,
  CPU/memory per process tree, process-tree cleanup (Windows job objects, Unix process groups), external services.
- **Frontend manager** – dev servers with HMR, production builds served by OpenShell with `/api` reverse proxy,
  external URLs; framework-agnostic.
- **Desktop integration** – window settings, loading screen, runtime panel, tray, notifications, close-to-tray /
  background mode, single instance, title-bar service menu, approval dialogs.
- **Agent Interaction Layer** – MCP (stdio + Streamable HTTP) and HTTP/JSON, semantic UI automation (accessibility-style
  snapshots with element refs, click/fill/press/navigate/wait), capabilities from config / OpenAPI / frontend SDK,
  AI provider discovery (Ollama, LM Studio, configured providers).
- **Security** – permission scopes per mode, human approvals, append-only audit log, project-scoped file access,
  loopback-only control API with tokens, Host and Origin checks.
- **Developer experience** – CLI (`init`, `create`, `dev`, `doctor`, `logs`, `package`, …), 8 templates, JSON Schema
  for editor completion, frontend SDK, `openshell doctor`.
- **Installers** – per-app branded installers without compiling desktop code.

## Documentation

| | |
|---|---|
| [Getting started](docs/getting-started.md) | Install, first app, existing projects |
| [Tutorial](docs/tutorial.md) | Build a complete app step by step |
| [Configuration](docs/configuration.md) | Every field of `openshell.json` |
| [Languages](docs/languages.md) | C++, Java, Go, Rust, .NET, Python, Node |
| [AI agents](docs/agents.md) | MCP profiles, tools, UI automation, capabilities, permissions |
| [AI app development](docs/ai/app-development.md) | Guide for AI coding agents building apps on OpenShell |
| [Installers](docs/installers.md) | `openshell package`, installed app behaviour |
| [CLI](docs/cli.md) · [SDK](docs/sdk.md) · [Control API](docs/control-api.md) | References |
| [Architecture](ARCHITECTURE.md) · [Roadmap](ROADMAP.md) · [Changelog](CHANGELOG.md) | Project |

## Project status

**Alpha (0.1).** Tested end to end on Windows 11: desktop shell, services in Python / Node / Java / C++, monitoring,
CLI, MCP in both profiles, OpenAPI import, UI automation, approvals, NSIS installers. macOS and Linux build in CI but
have seen little manual testing. APIs and the config format may still change before 1.0. Next: bundled runtimes,
auto-updates, code signing, screenshot fallback for UI automation – see the [roadmap](ROADMAP.md).

## Repository layout

```
apps/desktop/      openshell-desktop  Tauri 2 shell: window, WebView, tray, menus, dialogs, loading screen
crates/config      openshell-config   config schema, parsing, validation, variables
crates/runtime     openshell-runtime  services, health, builds, logs, metrics, static server, file watching
crates/agent       openshell-agent    permissions, approvals, audit, UI bridge, tool catalogue, MCP protocol
crates/core        openshell-core     runtime + agent layer, control API, tools, OpenAPI import, MCP stdio
crates/cli         openshell          command line interface
crates/package     openshell-package  native installers
packages/sdk       @openshell/sdk     optional frontend SDK
templates/         project templates (embedded into the CLI)
examples/          runnable examples
docs/              documentation (docs/ai: material for AI agents, docs/concept: original concept, German)
schema/            JSON Schema of openshell.json
```

## Contributing

Contributions are welcome – bug reports, docs, templates, platform testing (macOS/Linux!) and code. Start with
[CONTRIBUTING.md](CONTRIBUTING.md); please follow the [Code of Conduct](CODE_OF_CONDUCT.md) and report
vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE) © The OpenShell contributors
