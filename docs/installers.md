# Installers (`openshell package`)

`openshell package` turns an OpenShell project into a native installer – without writing or compiling any desktop
code for your app.

```bash
cargo build --release -p openshell-desktop   # once: the generic shell binary
openshell package                            # build + installer for the current platform
openshell package --target msi --out dist    # other formats, copy results to dist/
```

## What happens

1. `openshell build` runs (skip with `--skip-build`): compiled services (`services[].build`, e.g. Maven, CMake) and
   the frontend (`frontend.build`).
2. The application files are staged into `app/` (see *Which files are shipped*).
3. The generic `openshell-desktop` binary is copied, renamed to your app (`my-app.exe`) and **re-branded with your
   icon** (`app.icon`, PNG – OpenShell's icon if unset). No compiler is involved.
4. The Tauri bundler produces the installer(s):

| Platform | Formats (`--target` / `package.targets`) | Default |
|---|---|---|
| Windows | `nsis` (setup.exe, per-user install, no admin rights), `msi` | `nsis` |
| macOS | `app`, `dmg` | `app`, `dmg` |
| Linux | `deb`, `rpm`, `appimage` | `deb`, `appimage` |

The bundler downloads its tools (NSIS, WiX, appimagetool) on first use. The staging directory
`.openshell/package/bin/` is also a **portable build**: run `my-app.exe` there without installing.

## Installed app behaviour

```
%LOCALAPPDATA%\Programs\My App\        (Windows, NSIS per-user install)
  my-app.exe                           the shell, with your icon
  app\openshell.json                   your application
  app\...
```

- Starts in **production mode** (agent permissions from `agents.permissions.production`).
- State lives in a per-user directory named after the bundle identifier, e.g.
  `%LOCALAPPDATA%\com.example.myapp\` (Windows), `~/Library/Application Support/com.example.myapp/` (macOS),
  `~/.local/share/com.example.myapp/` (Linux): logs, audit log, `runtime.json` and **`data/`**, which services get as
  `OPENSHELL_DATA_DIR`. Store your database/files there so they survive updates and uninstalls.
- Single instance: starting the app again focuses the running window. A background instance started for an agent
  (`--mcp`) is replaced by the window.
- `my-app.exe --mcp` is an MCP server for AI agents on the user's machine (see [agents.md](agents.md#using-an-installed-app)).
- `my-app.exe --headless` runs the services without a window.

## Configuration

```json
{
  "app": { "name": "My App", "id": "com.example.myapp", "version": "1.2.0", "icon": "assets/icon.png" },
  "package": {
    "publisher": "Example Inc.",
    "homepage": "https://example.com",
    "copyright": "© 2026 Example Inc.",
    "license_file": "LICENSE",
    "targets": ["nsis", "msi"],
    "exclude": ["docs", "**/*.test.js"],
    "include": []
  }
}
```

`app.version` must be a semantic version (`1.2.0`). `package.identifier` overrides `app.id`.

### Which files are shipped

By default the whole project **except** `.git`, `.openshell`, `node_modules`, `__pycache__`, `*.pyc`, virtualenvs,
`.gradle`, IDE folders. Add `exclude` patterns (globs, relative to the project), or list exactly what to ship with
`include` (the config file is always included):

```json
"package": { "include": ["openshell.json", "web", "backend/build/bin"] }   // C++: only the compiled binary
"package": { "exclude": ["backend/src"] }                                 // Java: ship classes, not sources
"package": { "exclude": ["frontend/src", "frontend/public"] }             // ship only frontend/dist
```

## Runtimes

The installer contains the shell and your app files. Interpreters and runtimes your services need – Python, Node.js,
a JRE – must exist on the target machine (or be shipped by you inside the app, e.g. a `jlink` runtime image or an
embedded Python, referenced by a relative `command`). Compiled services (C++, Rust, Go, GraalVM native images) need
nothing extra. If a runtime is missing, the loading screen shows the error (`java was not found in PATH`).
Bundled runtimes are on the [roadmap](../ROADMAP.md).

## Signing

Unsigned installers trigger SmartScreen / Gatekeeper warnings. Sign `bundle/nsis/*.exe` with `signtool` (Windows) or
use `codesign` + notarization (macOS) after packaging. Built-in signing settings are planned.
