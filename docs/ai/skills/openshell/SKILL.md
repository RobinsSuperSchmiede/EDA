---
name: openshell
description: Build, run, debug, extend and package desktop applications with OpenShell (Rust/Tauri 2 desktop shell that runs a web frontend plus any backend from an openshell.json). Use when the user wants a new desktop app built with OpenShell, works in a project that contains an openshell.json, or asks to make an app usable for AI agents via OpenShell capabilities or MCP.
---

# OpenShell app development

OpenShell runs a web frontend in a native window and supervises the app's services (any language) described in
`openshell.json`. You write the app; OpenShell starts, health-checks, restarts, reloads, packages it and exposes it to
agents over MCP.

## Always

1. Check the tool: `openshell --version`. Read the full guide once with `openshell guide` (authoritative, up to date
   with the installed version).
2. New app: `openshell create <name> --template <t>` (`openshell create --list`). Existing web project: `openshell init`.
3. Follow the app contract: services read `PORT`, bind `127.0.0.1`, answer `GET /health`; data in
   `OPENSHELL_DATA_DIR`; unbuffered logs; frontend uses relative `/api` URLs; keep `"port": "auto"`.
4. Run in the background with `openshell dev --headless`, then verify with `openshell status`,
   `openshell logs --errors`, `openshell call cap_<name> '<json>'` and `openshell call ui_snapshot` / `ui_click` / `ui_fill`.
   Watched files restart services automatically – check status instead of restarting by hand.
5. Expose every user-facing function as a capability (`capabilities` in `openshell.json`, `"openapi"` on a service,
   or `OpenShell.agent.registerAction` in the frontend) so agents can use the app without the UI.
6. Before finishing: `openshell doctor`, `openshell build`, `openshell start --headless` (production check), then
   `openshell stop`. Installers: `openshell package`.

## References

- Guide: `openshell guide` (same as docs/ai/app-development.md in the OpenShell repository)
- Config schema: `openshell schema`
- Agent tools and permissions: `openshell tools`
