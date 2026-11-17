# OpenShell

**Build the shell once. Build applications on top of it. Let agents use and evolve them.**

OpenShell (project codename **EDA**) is an open-source desktop shell built on **Rust + Tauri 2**. It turns any
existing web application into a native desktop app – without writing desktop infrastructure – and gives AI agents a
standardized, permission-checked way to inspect, operate and evolve the running application.

OpenShell is not your application. It is the **runtime and management layer** around it:

```
                 you / your users                     AI agents (Claude, OpenAI, Ollama, LM Studio, MCP, custom)
                        │                                             │
                ┌───────▼─────────────────────────────────────────────▼───────┐
                │  OpenShell desktop shell (Tauri 2)                           │
                │  window · WebView · tray · notifications · approval dialogs │
                │  ┌───────────────────────┐   ┌────────────────────────────┐ │
                │  │ Runtime Manager       │   │ Agent Interaction Layer    │ │
                │  │ processes · health    │   │ tools · MCP · UI bridge    │ │
                │  │ restarts · logs       │   │ permissions · approvals    │ │
                │  │ ports · file watching │   │ audit log · capabilities   │ │
                │  └──────────┬────────────┘   └────────────────────────────┘ │
                └─────────────┼───────────────────────────────────────────────┘
                              │ manages (separate processes, never compiled into the shell)
          ┌──────────────┬────┴─────────┬──────────────┬─────────────┐
          ▼              ▼              ▼              ▼             ▼
      frontend        backend        database        worker      local AI server
   (React, Vue,    (Python, Node,   (SQLite,        (any)        (Ollama, ...)
    Svelte, ...)    Go, Rust, ...)   Postgres, ...)
```

Change the frontend or backend – by hand or by an agent – and the running app picks it up. The shell never needs to be
rebuilt.

## Features

- **One config file** (`openshell.json` or `.toml`), versioned, validated, with a JSON Schema for editor completion.
- **Runtime Manager** – starts services in dependency order, one-time `setup` steps (e.g. `npm install`), HTTP/TCP
  health checks, restart policies with backoff and crash-loop protection, dynamic ports (`"port": "auto"`),
  `${services.backend.url}` variables, log collection, process-tree cleanup (Windows job objects, Unix process groups).
- **Any frontend** – dev server with HMR, production build served by OpenShell (with `/api` reverse proxy), or any URL.
- **Any backend** – every executable works: Python, Node.js, Go, Rust, **Java**, **C++**, .NET, … or services that
  already run elsewhere. Compiled services get a `build` step that re-runs on every code change in development mode
  ([languages.md](docs/languages.md)).
- **Native installers** – `openshell package` creates NSIS/MSI, DMG or DEB/AppImage installers with your app's name
  and icon, without compiling desktop code ([installers.md](docs/installers.md)).
- **Service monitoring in the window** – right-click the title bar (Windows) or use the tray / macOS menu bar: every
  service with state, CPU, memory, pid, port, uptime and restart/stop/logs actions.
- **Development mode** – file watching restarts backends and reloads the window automatically.
- **Agents can use the app, not only build it** – the *app profile* exposes just the application's own functions
  (declared capabilities, imported OpenAPI operations, frontend actions) as MCP tools. Installed apps are MCP servers
  themselves (`my-app.exe --mcp`) and start in the background on demand – no UI automation needed.
- **Agent Interaction Layer** – 28 built-in tools (status, logs, services, files, UI automation, app capabilities) over **MCP**
  (stdio and Streamable HTTP) and a plain **HTTP/JSON API**. Model- and vendor-agnostic.
- **Semantic UI automation** – agents get an accessibility-style snapshot with element refs and can click, fill, press,
  navigate and wait – in the desktop window or a browser tab. No screenshots or pixel coordinates needed.
- **Application capabilities** – declare functions in the config (HTTP endpoint or task) or register them from the
  frontend with the SDK; agents discover and call them directly.
- **Security** – permission scopes (`read`, `ui`, `actions`, `services`, `files`, `code`, `system`) per mode,
  human approval for critical actions (desktop dialog or CLI), append-only audit log, project-scoped file access,
  loopback-only control API with token, Host and Origin checks.
- **CLI** – `init`, `create`, `dev`, `start`, `stop`, `restart`, `status`, `logs`, `doctor`, `mcp`, `call`, `approve`, …
- **Three modes, one runtime** – development, production and headless (no window, browser access).

## Quick start

Requirements: Rust (stable), and on Linux the Tauri system dependencies (WebKitGTK 4.1). Windows 11 and macOS work out
of the box.

```bash
git clone https://github.com/RobinsSuperSchmiede/EDA.git
cd EDA
cargo build --release          # builds `openshell` (CLI) and `openshell-desktop` (shell)
```

Add `target/release` to your `PATH` (or copy both binaries somewhere on it), then:

```bash
openshell create my-app        # zero-dependency template (HTML/JS + Python stdlib)
cd my-app
openshell dev                  # native window, hot reload, logs in the terminal
```

Other templates: `openshell create my-app --template react-python` (also `react-node`, `vue-python`, `svelte-go`,
`vanilla-java`, `vanilla-cpp`, `custom`). Ship it with `openshell package`. For an existing project run `openshell init` – it detects Vite/Next/CRA/Angular/Vue frontends and
Python/Node/Go/Rust backends.

Try it right away with the example in this repository:

```bash
cd examples/todo-app
openshell dev                  # or: openshell dev --headless (open the printed URL in a browser)
```

## Let an agent use or develop your app

```bash
# use the app (only its own functions, starts it in the background if needed)
claude mcp add my-app -- openshell mcp --app --start --project /path/to/my-app
# installed app: command "C:/…/My App/my-app.exe", args ["--mcp"]

# develop the app (runtime, logs, files, UI automation)
claude mcp add my-app-dev -- openshell mcp --project /path/to/my-app
```

The agent sees the running application: `app_info`, `runtime_status`, `logs_read`, `ui_snapshot`, `ui_click`,
`ui_fill`, `service_restart`, `cap_add_todo`, … Ask it *"Add a 'clear completed' button"* – it edits the code, the
shell reloads the window, and the agent verifies the result through the UI bridge. See [docs/agents.md](docs/agents.md).

Without MCP, the same tools are available as HTTP/JSON (`POST /v1/tools/{name}`) or from the terminal:

```bash
openshell call ui_snapshot
openshell call ui_fill '{"label": "New todo", "value": "Buy milk", "submit": true}'
openshell call service_restart '{"name": "backend"}'
```

## Repository layout

```
crates/config      openshell-config   – config schema, parsing, validation, variables
crates/runtime     openshell-runtime  – process/service/frontend manager, health, logs, static server, watcher
crates/agent       openshell-agent    – permissions, approvals, audit, UI bridge hub, tool catalogue, MCP
crates/core        openshell-core     – wires runtime + agent layer, control API (HTTP/WS/MCP), tool execution
crates/cli         openshell          – command line interface (also the headless runtime and MCP stdio server)
crates/package     openshell-package  – native installers (branded shell binary + app files + Tauri bundler)
apps/desktop       openshell-desktop  – Tauri 2 shell: window, WebView, tray, dialogs, loading screen, runtime panel
packages/sdk       @openshell/sdk     – optional frontend SDK (no dependencies)
templates/         project templates (embedded into the CLI)
examples/todo-app  runnable example
schema/            JSON Schema of openshell.json
docs/              documentation
```

## Documentation

- [Getting started](docs/getting-started.md)
- [Configuration reference](docs/configuration.md)
- [Agent Interaction Layer](docs/agents.md) – MCP, tools, UI automation, capabilities, permissions
- [CLI reference](docs/cli.md)
- [Frontend SDK](docs/sdk.md)
- [C++, Java and other languages](docs/languages.md)
- [Installers](docs/installers.md)
- [Control API](docs/control-api.md)
- [Architecture](ARCHITECTURE.md) and [Roadmap](ROADMAP.md)
- [Contributing](CONTRIBUTING.md)
- Original concept (German): [docs/concept.de.md](docs/concept.de.md), [Plan.md](Plan.md), [Prompt.md](Prompt.md)

## Status

Alpha (v0.1). Works end to end on Windows: desktop shell, WebView, backend supervision (Python, Node, Java, C++),
health checks, resource monitoring, logs, CLI, MCP (app and developer profiles), OpenAPI import, UI automation,
approvals, NSIS installers. macOS/Linux builds are untested so far. Auto-updates, bundled runtimes and autostart are
on the [roadmap](ROADMAP.md).

## License

[MIT](LICENSE)
