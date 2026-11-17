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

const unregister = OpenShell.agent.registerAction(name, { description, input_schema, permission }, handler)
OpenShell.agent.unregisterAction(name)

await OpenShell.call(tool, args)        // any tool allowed by agents.sdk_permissions
```

Calls are checked against `agents.sdk_permissions` (default `["read", "ui"]`). To let the frontend restart services,
add `"services"`. Failures throw `OpenShellError`.

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
