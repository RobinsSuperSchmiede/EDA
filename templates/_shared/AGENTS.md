# AGENTS.md – {{app_name}}

This is an **OpenShell application**. OpenShell (desktop shell + runtime) runs the web frontend in a native window
and supervises the services listed in `openshell.json`. You edit the application; OpenShell starts, restarts and
reloads it. Full guide: run `openshell guide` (or read docs/ai/app-development.md in the OpenShell repository).

## Commands

```bash
openshell dev --headless      # run without window (start it in the background – it blocks); prints the frontend URL
openshell dev                 # run with the native window
openshell status              # all services must be "running"
openshell logs --errors       # stderr + runtime messages (compiler errors, crashes)
openshell logs <service> -n 50
openshell restart <service>   # usually not needed: watched files restart services automatically
openshell call <tool> '<json>'  # e.g. ui_snapshot, ui_click, cap_<capability>, capabilities_list
openshell doctor              # config + toolchain check
openshell add frontend | backend [--lang node] | database [name] | function <name>   # add a building block
openshell stop
openshell build && openshell package   # production build + native installer
```

## Rules for this app

1. Services read their port from `PORT` and bind to `127.0.0.1`; keep `"port": "auto"` in `openshell.json`.
2. HTTP services keep answering `GET /health` with 200.
3. Store data in the directory from `OPENSHELL_DATA_DIR` – never next to the code (watched folders restart services).
4. Log to stdout/stderr, unbuffered.
5. The frontend calls the backend with relative URLs (`/api/...`); OpenShell proxies them.
6. Every user-facing function should also be an agent capability: add it to `capabilities` in `openshell.json`
   (HTTP endpoint or task), import it via `"openapi"`, or register it in the frontend with
   `OpenShell.agent.registerAction`. Test with `openshell call cap_<name> '<json>'`.
7. Do not edit `.openshell/` (runtime state).
8. Use only the building blocks the task needs: a frontend can store data in the built-in database
   (`OpenShell.db`), agent functions can be plain scripts (`"run"`), no backend is required for either.
   Schema changes go into new files in the migrations folder.

## Verify a change

1. `openshell status` – everything `running`; otherwise `openshell logs --errors`.
2. Call the endpoint or capability (`openshell call cap_<name> ...`).
3. Check the UI: `openshell call ui_snapshot`, then `ui_fill` / `ui_click` with the element refs (requires the window,
   or a browser tab for frontends served by OpenShell).

## Project layout

See `openshell.json` for the authoritative list of services, ports, health checks, watch paths and capabilities.
