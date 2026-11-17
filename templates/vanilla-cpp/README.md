# {{app_name}}

An [OpenShell](https://github.com/RobinsSuperSchmiede/EDA) application.

```
web/          static frontend (served by OpenShell, /api is proxied to the backend)
backend/      C++17 backend (src/main.cpp + CMakeLists.txt, built by OpenShell)
openshell.json
```

## Run

```bash
openshell dev            # desktop window, hot reload, file watching
openshell dev --headless # no window – open the printed URL in your browser
openshell start          # production mode
```

## Let an AI agent work on it

Register OpenShell as an MCP server in your agent, e.g. Claude Code:

```bash
claude mcp add {{project_name}} -- openshell mcp --project .
```

The agent can now inspect the running app (`ui_snapshot`, `logs_read`, `runtime_status`),
use it (`ui_click`, `ui_fill`, `cap_add_todo`), and restart services after changing code.
Try: *"Add a 'clear completed' button to the todo app."*
