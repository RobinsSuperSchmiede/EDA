/**
 * @openshell/sdk – optional frontend SDK for applications running in OpenShell.
 *
 *   import { OpenShell } from "@openshell/sdk";
 *
 *   if (OpenShell.available) {
 *     const { services } = await OpenShell.runtime.status();
 *     await OpenShell.window.minimize();
 *     await OpenShell.notify("Export finished", "Saved to reports/q3.pdf");
 *     OpenShell.agent.registerAction("add_todo", {
 *       description: "Add a todo item",
 *       input_schema: { type: "object", properties: { title: { type: "string" } }, required: ["title"] },
 *     }, async ({ title }) => addTodo(title));
 *   }
 *
 * The SDK talks to the local OpenShell control API with the app token that
 * the shell injects as `window.__OPENSHELL__`. Which calls are allowed is
 * governed by `agents.sdk_permissions` in openshell.json (default: read, ui).
 * Outside of OpenShell (plain browser) `OpenShell.available` is false and all
 * calls reject, so the same frontend code works everywhere.
 *
 * No dependencies, no build step. Also usable as `<script type="module" src="http://127.0.0.1:<port>/sdk.js">`.
 */

const cfg = (typeof window !== "undefined" && window.__OPENSHELL__) || null;

class OpenShellError extends Error {
  constructor(message) {
    super(message);
    this.name = "OpenShellError";
  }
}

async function call(tool, args) {
  if (!cfg) throw new OpenShellError("not running inside OpenShell");
  const res = await fetch(`${cfg.api}/v1/sdk/call/${tool}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${cfg.token}` },
    body: JSON.stringify(args || {}),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw new OpenShellError(data.error || `HTTP ${res.status}`);
  if (!data.ok) throw new OpenShellError((data.result && data.result.error) || "call failed");
  return data.result;
}

function actionRegistry() {
  window.__OPENSHELL_ACTIONS__ = window.__OPENSHELL_ACTIONS__ || {};
  return window.__OPENSHELL_ACTIONS__;
}

function actionsChanged() {
  window.dispatchEvent(new Event("openshell:actions-changed"));
}

export const OpenShell = {
  /** True when the page runs inside OpenShell (desktop window or bridged browser tab). */
  available: !!cfg,
  /** `development` | `production` (null outside OpenShell). */
  mode: cfg ? cfg.mode : null,
  /** `desktop` | `browser` (null outside OpenShell). */
  host: cfg ? cfg.host : null,

  /** Application and permission info. */
  async info() {
    if (!cfg) throw new OpenShellError("not running inside OpenShell");
    const res = await fetch(`${cfg.api}/v1/sdk/info`, { headers: { Authorization: `Bearer ${cfg.token}` } });
    return res.json();
  },

  runtime: {
    /** Status of all services. Requires `read`. */
    status: () => call("runtime_status"),
    /** Recent log lines. Requires `read`. */
    logs: (opts) => call("logs_read", opts),
    /** Requires `services`. */
    start: (name) => call("service_start", { name }),
    /** Requires `services`. */
    stop: (name) => call("service_stop", { name }),
    /** Requires `services`. */
    restart: (name) => call("service_restart", { name }),
  },

  window: {
    show: () => call("window_control", { action: "show" }),
    hide: () => call("window_control", { action: "hide" }),
    focus: () => call("window_control", { action: "focus" }),
    minimize: () => call("window_control", { action: "minimize" }),
    maximize: () => call("window_control", { action: "maximize" }),
    unmaximize: () => call("window_control", { action: "unmaximize" }),
  },

  /** Native desktop notification. Requires `ui`. */
  notify: (title, body) => call("notify", { title, body: body || "" }),

  /** AI providers configured for the app / detected locally. Requires `read`. */
  providers: () => call("providers_list"),

  /**
   * The app's built-in databases (SQLite, `databases` in openshell.json). Lets a frontend-only
   * app store data without a backend. Reads need `read`, writes need `actions` in
   * `agents.sdk_permissions` (`openshell add database` sets that up).
   */
  db: {
    /** `{ databases: [{ name, tables: [{ name, rows }] }] }` */
    list: () => call("db_list"),
    /** Tables, columns and CREATE statements. */
    schema: (table, database) => call("db_schema", { table, database }),
    /** Read-only query: `{ columns, rows: [{col: value}], row_count, truncated }`. Params: array for `?`, object for `:name`. */
    query: (sql, params, database) => call("db_query", { sql, params, database }),
    /** INSERT / UPDATE / DELETE / DDL: `{ changes, last_insert_rowid }`. */
    execute: (sql, params, database) => call("db_execute", { sql, params, database }),
  },

  /** Call one of the app's functions (capabilities) – the same ones AI agents use. */
  invoke: (name, input) => call("app_invoke", { name, input: input || {} }),

  agent: {
    /**
     * Expose a frontend function to AI agents. Agents discover it via
     * `capabilities_list` and call it with `app_invoke` (or the `cap_<name>` tool).
     * @param {string} name  lowercase identifier, e.g. "add_todo"
     * @param {{description?: string, input_schema?: object, permission?: "read"|"actions"}} meta
     * @param {(input: any) => any} handler  may return a Promise; the result is sent to the agent
     * @returns {() => void} unregister function
     */
    registerAction(name, meta, handler) {
      if (typeof meta === "function") {
        handler = meta;
        meta = {};
      }
      actionRegistry()[name] = Object.assign({}, meta, { handler });
      actionsChanged();
      return () => OpenShell.agent.unregisterAction(name);
    },
    unregisterAction(name) {
      delete actionRegistry()[name];
      actionsChanged();
    },
  },

  /** Low-level access to any tool allowed by `agents.sdk_permissions`. */
  call,
};

if (typeof window !== "undefined") {
  window.OpenShell = window.OpenShell || OpenShell;
}

export { OpenShellError };
export default OpenShell;
