# {{app_name}}

A blank [OpenShell](https://github.com/RobinsSuperSchmiede/EDA) project. Describe your application in `openshell.json`:

```jsonc
{
  "frontend": {
    "command": "npm run dev",          // your dev server
    "cwd": "frontend",
    "url": "http://localhost:5173",    // what the window loads
    "dist": "frontend/dist"            // production build (served by OpenShell)
  },
  "services": [
    {
      "name": "backend",
      "command": ["python", "main.py"], // any executable: python, node, go, java, ...
      "cwd": "backend",
      "port": 8000,
      "healthcheck": "http://127.0.0.1:8000/health",
      "restart": true,
      "watch": ["backend"]
    },
    { "name": "db", "external": true, "healthcheck": { "tcp": "127.0.0.1:5432" } }
  ]
}
```

Then run `openshell doctor` and `openshell dev`. Full reference: `docs/configuration.md` in the OpenShell repository.
