# Frontend SDK (`@openshell/sdk`)

The SDK is optional. Your frontend works unchanged inside and outside OpenShell; the SDK adds access to the runtime,
the native window and the agent layer when available.

## Loading

The shell serves the SDK at `{control_url}/sdk.js`, so no package installation is needed:

```js
async function openshell() {
  if (!window.__OPENSHELL__) return null; // plain browser
  const { OpenShell } = await import(/* @vite-ignore */ `${window.__OPENSHELL__.api}/sdk.js`);
  return OpenShell;
}
```

Or copy `packages/sdk` into your project / install it from the repository (`npm install <path>/packages/sdk`) and
`import { OpenShell } from "@openshell/sdk"`.

## API

```ts
OpenShell.available: boolean            // running inside OpenShell?
OpenShell.mode: "development" | "production" | null
OpenShell.host: "desktop" | "browser" | null

await OpenShell.info()                  // app, mode, host, granted SDK permissions, service names

await OpenShell.runtime.status()        // read
await OpenShell.runtime.logs({ service: "backend", limit: 50 })   // read
await OpenShell.runtime.restart("backend")                         // services
await OpenShell.runtime.start("worker") / .stop("worker")          // services

await OpenShell.window.minimize() / maximize() / unmaximize() / show() / hide() / focus()   // ui
await OpenShell.notify("Export finished", "Saved to reports/q3.pdf")                       // ui
await OpenShell.providers()             // read – configured and local AI providers

// built-in databases (databases in openshell.json) – a frontend-only app can store data without a backend
await OpenShell.db.list()                                            // read
await OpenShell.db.query("SELECT * FROM notes WHERE done = ?", [0])  // read  → { columns, rows, row_count }
await OpenShell.db.execute("INSERT INTO notes (title) VALUES (:t)", { t: "Hi" })   // actions → { changes, last_insert_rowid }
await OpenShell.db.schema("notes")                                   // read

// the app's functions (capabilities) – the same ones agents use
await OpenShell.invoke("summarize_notes", { limit: 10 })            // the capability's own scope

const unregister = OpenShell.agent.registerAction(name, { description, input_schema, permission }, handler)
OpenShell.agent.unregisterAction(name)

await OpenShell.call(tool, args)        // any tool allowed by agents.sdk_permissions
```

Calls are checked against `agents.sdk_permissions` (default `["read", "ui"]`). To let the frontend write to a
database or call `actions` functions, add `"actions"` (`openshell add database` does this when the app has a
frontend); to restart services, add `"services"`. A database's `agent_access` limits agents only, not the app's own
frontend. Failures throw `OpenShellError`.

## Exposing frontend actions to agents

```js
OpenShell.agent.registerAction(
  "open_invoice",
  {
    description: "Open the invoice editor for an invoice id",
    permission: "ui",
    input_schema: { type: "object", properties: { id: { type: "string" } }, required: ["id"] },
  },
  async ({ id }) => {
    router.push(`/invoices/${id}`);
    return { opened: id };
  },
);
```

Agents discover it with `capabilities_list` and call it as `cap_open_invoice` or `app_invoke`. The handler runs in the
page; its return value (JSON) is sent back to the agent.

## Own title bar

With `"window": { "title_bar": "overlay" }` the desktop window has no system title bar (Windows, Linux) or shows
only the window buttons over the content (macOS), and the frontend draws its own. The desktop window provides:

```js
const win = window.__OPENSHELL__?.window;     // undefined in a plain browser
if (win?.title_bar === "overlay") {
  win.os;                 // "windows" | "macos" | "linux" – macOS: leave ~80px on the left for its buttons
  await win.minimize();
  await win.toggleMaximize();
  await win.close();      // respects close_to_tray
  await win.isMaximized();
  await win.showMenu();   // the runtime menu (services, logs, reload) at the cursor
}
```

Mark the bar with `data-tauri-drag-region="deep"`: dragging it moves the window, a double click maximizes it; buttons,
links and inputs inside keep working. These functions are allowed only for the app's frontend origin.
