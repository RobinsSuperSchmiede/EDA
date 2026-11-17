# Roadmap

Legend: ✅ done · 🚧 partial · ⏳ planned

## Phase 1 – Foundation ✅
- ✅ Cargo workspace (config, runtime, agent, core, cli, desktop)
- ✅ Tauri 2 desktop shell with WebView, loading screen, window settings from config
- ✅ Versioned configuration (`version: 1`), JSON + TOML, validation with precise paths, JSON Schema

## Phase 2 – Runtime Manager ✅
- ✅ Process start/stop/restart, process-tree termination, Windows job objects
- ✅ HTTP / TCP health checks, readiness, unhealthy detection
- ✅ Restart policies (`never`, `on-failure`, `always`), backoff, crash-loop limit
- ✅ Dependencies and startup order, reverse-order shutdown
- ✅ Dynamic ports, `${...}` variables, external services
- ✅ Log store (ring buffer + files), log queries
- ✅ Static frontend server with SPA fallback and reverse proxy

## Phase 3 – Developer Experience 🚧
- ✅ CLI: init, create, dev, start, stop, restart, run, status, logs, doctor, build, schema
- ✅ Templates: vanilla-python, react-python, react-node, vue-python, svelte-go, vanilla-java, vanilla-cpp, custom
- ✅ Build steps for compiled services (C++, Java, Go, Rust, .NET), rebuild on change, detection in `openshell init`
- ✅ Resource monitoring (CPU/memory per service process tree) in CLI, runtime panel, title bar and tray menus
- ✅ Development mode with file watching (service restart / window reload)
- ⏳ Publish CLI binaries (GitHub releases, `cargo install`, npm wrapper `npx openshell`)
- ⏳ Interactive `openshell init`

## Phase 4 – Production 🚧
- ✅ Production mode, tray, close-to-tray / background mode, headless mode (`--headless`, `--detach`)
- ✅ `openshell package`: per-app installers (NSIS, MSI, DMG, DEB, RPM, AppImage) with name, icon (exe re-branding)
  and identifier; per-user data directory; single instance; `--mcp` / `--headless` modes of installed apps
- ⏳ Code signing settings, bundled runtimes (JRE via jlink, embedded Python/Node)
- ⏳ Runtime bundling (embedded Python/Node), auto-updates (Tauri updater), autostart
- ⏳ Single-instance handling, deep links

## Phase 5 – Agent Interaction Layer 🚧
- ✅ Tool catalogue (28 built-in tools), HTTP/JSON API, MCP over stdio and Streamable HTTP
- ✅ Semantic UI bridge (snapshot, click, fill, press, read, wait, navigate, reload, eval)
- ✅ Capability discovery (config, OpenAPI import, frontend-registered actions)
- ✅ App profile ("use the app"): only the application's functions as MCP tools; installed apps start headless on demand
- ✅ Permission scopes per mode, approvals (dialog/CLI/API), audit log
- ✅ Local provider discovery (Ollama, LM Studio) and configured providers
- ⏳ Screenshot / vision fallback for canvas-heavy UIs
- ⏳ Agent sessions with individual permissions and tokens (instead of one agent policy)
- ⏳ Agent runner integration (hand tasks to a configured agent, stream progress into the UI)
- ⏳ MCP resources (logs, config) and notifications (`tools/list_changed` when pages register actions)

## Phase 6 – Ecosystem ⏳
- 🚧 SDK (`@openshell/sdk`) – runtime, window, notifications, agent actions; publish to npm
- ⏳ Extension system (shell plugins)
- ⏳ More templates (Next.js, FastAPI with venv, Django, Rails, .NET)
- ⏳ Documentation website, demo video ("an agent adds a feature to a running desktop app")
