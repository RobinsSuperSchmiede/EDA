# Architecture

OpenShell separates **the shell** (generic, built once) from **the application** (frontend, backend, data – owned by
the developer and changeable at any time by humans or agents). The shell contains no application logic; it reads a
configuration and manages external components.

## Components

```
┌──────────────────────────── openshell-desktop (Tauri 2) ─────────────────────────────┐
│  main window ── WebView ── initialization script (window.__OPENSHELL__ + bridge.js)   │
│  tray menu · notifications · approval dialogs · loading screen · runtime panel       │
│                                   │ implements `Host`                                │
│  ┌────────────────────────────── openshell-core ───────────────────────────────────┐ │
│  │ Shell = Runtime + UiHub + Approvals + AuditLog + Policies + control API          │ │
│  │ control API (127.0.0.1 only): REST /v1/* · MCP /mcp · WebSocket /v1/ui/bridge    │ │
│  │ tool execution (exec.rs) · project-scoped file access (fs.rs)                    │ │
│  └──────────────┬───────────────────────────────────────────┬─────────────────────┘ │
│                 │                                           │                       │
│  ┌──── openshell-runtime ─────┐            ┌──────── openshell-agent ────────────┐  │
│  │ plan: ports, ${vars}, probes│            │ policy · approvals · audit          │  │
│  │ supervisor per service      │            │ UiHub (bridge sessions, actions)    │  │
│  │ health checks · log store   │            │ tool catalogue · MCP JSON-RPC       │  │
│  │ static server + proxy       │            │ provider discovery · bridge.js      │  │
│  │ file watcher · job objects  │            └─────────────────────────────────────┘  │
│  └─────────────────────────────┘                                                     │
│                 openshell-config: schema · parsing · validation · interpolation      │
└──────────────────────────────────────────────────────────────────────────────────────┘
        ▲ HTTP (token)                    ▲ MCP stdio→HTTP                ▲ WebSocket
   openshell CLI                   openshell mcp (agents)           pages running bridge.js
```

The same `openshell-core::Shell` runs inside the desktop app and inside the CLI (`--headless`). All three operating
modes therefore share one runtime implementation; they differ only in the `Host` (desktop window vs. headless) and the
permission defaults (development vs. production).

## Key decisions

| Decision | Rationale |
|---|---|
| **Tauri 2 + Rust** for the shell | Small binaries, system WebView (WebView2/WebKit), cross-platform, strong process control in Rust. |
| **Generic shell binary** (`openshell-desktop`) instead of generating a Tauri project per app | No fork, no per-app Rust code, improvements reach every app. Per-app branding/packaging is a later phase. |
| **Services are external processes** | Any language; code changes never require rebuilding the shell; crash isolation. |
| **Supervisor task per service** with a desired-state channel (`watch`) | Start/stop/restart are just changes of the desired state; the supervisor reconciles. Races (start while stopping) resolve naturally. |
| **Ports allocated up front** (`"port": "auto"`), `${...}` variables | All URLs are known before anything starts, so services can reference each other and the frontend can proxy to the backend. |
| **Control API on 127.0.0.1 with tokens** | One interface for CLI, SDK and agents. `runtime.json` (in `.openshell/`) tells local tools where the shell runs. |
| **Two tokens**: admin (CLI/agents) and app (frontend SDK) | The frontend gets only `/v1/sdk/*` and the bridge, limited by `agents.sdk_permissions`. |
| **Host + Origin checks, CORS allow-list** | Protects the loopback API against DNS rebinding and against other websites in the user's browser. |
| **MCP as adapter, own tool API as core** | MCP is the primary integration for agents (`openshell mcp`, `POST /mcp`), but every tool is also plain HTTP/JSON, so non-MCP agents and scripts work too. |
| **Semantic UI bridge (DOM/accessibility) over pixels** | Element refs, roles and names are robust and cheap for models. The bridge is injected into the WebView and the static server, so it works for any framework and in browser mode. Screenshots/vision can be added later as a fallback. |
| **Capabilities from config and from the frontend** | Backends expose functions declaratively (HTTP/task); frontends register UI-level actions at runtime via the SDK. Agents prefer them over UI automation. |
| **Permission scopes + approval queue + audit log** | Agents never get blanket OS access. Production defaults to read/ui/actions. Critical scopes require a human decision (dialog or CLI); everything is logged to `.openshell/audit.jsonl`. |
| **Project-scoped file tools** | `fs_*` tools resolve paths relative to the project root, reject `..`, absolute paths, symlink escapes and `.openshell/`. |
| **No arbitrary shell commands for agents** | Only tasks declared in the config (`task_run`, scope `system`). |
| **Two agent profiles** (`/mcp` full, `/mcp/app` app) | Agents that *use* an application should see a small, domain-specific tool list (its capabilities, OpenAPI operations, frontend actions), not 28 developer tools. |
| **OpenAPI import** | Most backend frameworks already describe their API; importing it makes apps agent-usable without extra code. |
| **Installers re-brand the generic binary** (`openshell-package`) | Copy `openshell-desktop`, replace the exe icon (editpe), put the app into `app/`, run the Tauri bundler. No per-app compilation, so every app gets shell updates by re-packaging. |
| **Per-user state for installed apps** | `runtime.json`, logs, audit and `OPENSHELL_DATA_DIR` go to the user's local data directory (bundle identifier), never into the install directory. |
| **Title bar menu via window subclassing (Windows)** | Right-clicking the caption (`WM_NCRBUTTONUP`/`HTCAPTION`) opens a Tauri popup menu with live service data instead of the system menu; Alt+Space keeps the system menu. Tray (all platforms) and menu bar (macOS) show the same menu. |
| **Build steps run inside the supervisor** | Compile → start → watch → recompile is one lifecycle; build errors become a `failed` state with compiler output in the logs, retried on the next change. |

## Lifecycle of `openshell dev`

1. CLI loads and validates `openshell.json`, checks that no instance runs, launches `openshell-desktop --project … --mode development` in its own process group.
2. Desktop: `Shell::launch` builds the plan (ports, URLs, probes), binds the control API, writes `.openshell/runtime.json`, creates the main window with the loading page and the initialization script.
3. `Shell::start` sets all autostart services to "run". Supervisors wait for dependencies, run `setup` steps, spawn processes, pipe logs, probe health.
4. When the frontend is healthy the main window navigates to its URL; `bridge.js` connects back over WebSocket.
5. File changes → backend restart / window reload. Crashes → restart with backoff, `failed` after N crashes per minute.
6. The CLI follows logs and approval requests via the control API. Ctrl+C or `openshell stop` → `POST /v1/shutdown` → services stop in reverse dependency order → window closes.

## Security model (summary)

- Threats considered: other websites in the user's browser (CSRF / DNS rebinding against the loopback API), a frontend
  page trying to escalate, agents acting outside their remit, path traversal.
- Not a sandbox against local malware: any local process running as the user can read `runtime.json`. The permission
  system is a **policy layer for cooperating agents**, the audit log provides accountability.

See [docs/agents.md](docs/agents.md#security) for details.
