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
- ✅ Building blocks: `openshell add frontend|backend|database|function`, `openshell create --with …` (interactive)
- ✅ Built-in SQLite databases with migrations, SDK and agent access; script functions (`run`) without a server
- ✅ Apps without frontend (console window) or without window (tray only); launch at login

## Phase 4 – Production 🚧
- ✅ Production mode, tray, close-to-tray / background mode, headless mode (`--headless`, `--detach`)
- ✅ `openshell package`: per-app installers (NSIS, MSI, DMG, DEB, RPM, AppImage) with name, icon (exe re-branding)
  and identifier; per-user data directory; single instance; `--mcp` / `--headless` modes of installed apps
- ✅ Code signing settings (`package.signing`: Windows thumbprint / sign command, macOS identity + notarisation)
- ✅ Portable app bundles (`.osapp`), `openshell install`, release workflow for every platform with
  `downloads.json` and a download page (`openshell release`)
- 🚧 Updates: update check (`package.update_url`, `GET /v1/update`, `openshell update`); automatic install ⏳
- ⏳ Bundled runtimes (JRE via jlink, embedded Python/Node)
- ⏳ Single-instance handling, deep links

## Phase 5 – Agent Interaction Layer 🚧
- ✅ Tool catalogue (32 built-in tools), HTTP/JSON API, MCP over stdio and Streamable HTTP
- ✅ Semantic UI bridge (snapshot, click, fill, press, read, wait, navigate, reload, eval)
- ✅ Capability discovery (config, OpenAPI import, frontend-registered actions)
- ✅ App profile ("use the app"): only the application's functions as MCP tools; installed apps start headless on demand
- ✅ Permission scopes per mode, approvals (dialog/CLI/API), audit log
- ✅ Local provider discovery (Ollama, LM Studio) and configured providers
- ⏳ Screenshot / vision fallback for canvas-heavy UIs
- ⏳ Agent sessions with individual permissions and tokens (instead of one agent policy)
- ⏳ Agent runner integration (hand tasks to a configured agent, stream progress into the UI)
- ✅ MCP resources (status, config, logs, capabilities, database schemas) and `tools/list_changed` notifications

## Phase 6 – Platforms: Android and connected devices 🚧
- ✅ Platform-neutral core: `RuntimeAdapter` (desktop, server, Android), capabilities in status and `app_info`
- ✅ `services[].platforms`, `services[].background` (`keep` / `pause`), lifecycle events (`openshell lifecycle`, `POST /v1/lifecycle`)
- ✅ Core and CLI build for Android (CI); verified headless on an Android 16 emulator; usable in Termux
- ✅ Remote gateway (`runtime.remote`): app profile over MCP for other devices, pairing token, `openshell remote`
- ✅ Remote security: per-device tokens with QR pairing, IP/CIDR allowlist, rate limit + lockout, TLS (self-signed
  with pinned fingerprint or own certificate), opt-in exposure of services (HTTP with token login, raw TCP), audit
- ✅ Termux: `TermuxAdapter` (toolchains from `pkg`, wake lock, install hints), Android CLI in releases
- 🚧 Android app: Kotlin host + JNI bridge (`apps/android`, `crates/android`), foreground service (`specialUse`),
  notification, app manager for `.osapp` bundles, AAB build for the Play Store in CI; not yet verified on a device
- ⏳ Play Store release (developer account, upload key, store review)
- ⏳ Bundled runtimes for Android (Node.js, Python as `lib*.so`), app manager for several apps on the phone
- 🚧 Device mesh: ✅ per-device pairing (QR), ✅ TLS; ⏳ mDNS discovery, remote control API (`--device`), project
  sync and deploy (`openshell deploy --device phone`)

## Phase 7 – Ecosystem ⏳
- 🚧 SDK (`@openshell/sdk`) – runtime, window, notifications, agent actions; publish to npm
- ⏳ Extension system (shell plugins)
- ⏳ More templates (Next.js, FastAPI with venv, Django, Rails, .NET)
- ✅ Website: landing page, pages for users, teams, developers and AI agents, download page, searchable documentation
  (`website/`, `scripts/build-website.py`); ⏳ publish it (GitHub Pages or similar)
- ⏳ Demo video ("an agent adds a feature to a running desktop app")
