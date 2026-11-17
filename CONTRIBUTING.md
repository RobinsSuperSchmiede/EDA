# Contributing to OpenShell

Thanks for your interest! OpenShell aims to stay small: a generic shell plus a reliable runtime and agent layer.
Please open an issue or discussion before large changes.

## Development setup

```bash
cargo build                      # all crates incl. the desktop shell
cargo test --workspace --exclude openshell-desktop
cargo run -p openshell-cli -- --help
```

- Windows: WebView2 (preinstalled on Windows 11).
- macOS: Xcode command line tools.
- Linux: `libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf` (see the Tauri prerequisites).

Run the example while developing:

```bash
cargo build
cd examples/todo-app
../../target/debug/openshell dev
```

## Where things live

| Change | Crate |
|---|---|
| Config fields, validation | `crates/config` |
| Process supervision, health, logs, static server | `crates/runtime` |
| Permissions, approvals, audit, UI bridge (`assets/bridge.js`), MCP, tool definitions | `crates/agent` |
| Tool implementations, control API routes | `crates/core` |
| CLI commands, templates | `crates/cli`, `templates/` (`templates/_shared` is added to every project) |
| Installers | `crates/package` |
| Docs for AI agents | `docs/ai/`, `AGENTS.md`, `llms.txt` |
| Window, tray, dialogs, loading screen | `apps/desktop` |
| Frontend SDK | `packages/sdk` |

When you add a config field, regenerate the schema: `cargo run -p openshell-cli -- schema > schema/openshell.schema.json`.
When you change documentation, regenerate `llms-full.txt`: `python scripts/gen-llms-full.py`. CI checks both.

Using an AI coding agent to contribute? [AGENTS.md](AGENTS.md) contains the project map, commands and conventions.
Changes that affect app developers must also update [docs/ai/app-development.md](docs/ai/app-development.md) and
[templates/_shared/AGENTS.md](templates/_shared/AGENTS.md) – that is what AI agents read when they build apps.

## Guidelines

- Keep the shell technology-agnostic: no assumptions about frontend frameworks, languages or AI vendors.
- Every agent tool declares exactly one permission scope and is audited.
- Tests for runtime behaviour use portable shell commands (`echo`, `exit`, `ping`/`sleep`).
- Run `cargo fmt` and `cargo clippy` before opening a PR.
- Write user-facing docs for new commands, tools and config fields.

## Reporting security issues

Please do not open public issues for vulnerabilities; contact the maintainers privately via GitHub security advisories.
