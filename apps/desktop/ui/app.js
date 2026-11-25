// OpenShell's own pages: loading screen (index.html) and runtime panel (runtime.html).
const invoke = window.__TAURI__.core.invoke;
const isPanel = document.body.classList.contains("panel");
// console.html: the main window of apps without a frontend
const isConsole = document.body.classList.contains("console");
const $ = (s) => document.querySelector(s);
const ACTIVE = ["running", "unhealthy", "starting", "pending", "setup", "building", "backoff"];

function mb(bytes) {
  const m = bytes / 1048576;
  return m >= 1024 ? (m / 1024).toFixed(1) + " GB" : m.toFixed(0) + " MB";
}

function detail(s) {
  const parts = [];
  if (s.metrics) parts.push(s.metrics.cpu_percent.toFixed(1) + "% CPU", mb(s.metrics.memory_bytes));
  if (isPanel && s.pid) parts.push("pid " + s.pid);
  if (isPanel && s.restarts) parts.push(s.restarts + " restart(s)");
  parts.push(s.last_error || s.url || s.command || "");
  return parts.filter(Boolean).join(" · ");
}
let lastSeq = 0;
let logLines = [];

function el(tag, attrs = {}, ...children) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else e.setAttribute(k, v);
  }
  e.append(...children.filter((c) => c !== null && c !== undefined));
  return e;
}

function action(name, act) {
  return el(
    "button",
    { type: "button", onclick: () => invoke("service_action", { name, action: act }).catch((e) => alert(e)) },
    act,
  );
}

function renderServices(services) {
  $("#services").replaceChildren(
    ...services.map((s) =>
      el(
        "li",
        { class: "svc " + s.state },
        el("span", { class: "dot", "aria-hidden": "true" }),
        el("strong", {}, s.name),
        el("span", { class: "state" }, s.state),
        el("span", { class: "muted detail", title: s.last_error || s.command || "" }, detail(s)),
        el(
          "span",
          { class: "buttons" },
          ACTIVE.includes(s.state) ? action(s.name, "restart") : action(s.name, "start"),
          isPanel && ACTIVE.includes(s.state) ? action(s.name, "stop") : null,
        ),
      ),
    ),
  );
  if (isPanel) {
    const select = $("#filter");
    const names = services.map((s) => s.name);
    if (select.options.length - 1 !== names.length) {
      select.replaceChildren(el("option", { value: "" }, "all"), ...names.map((n) => el("option", { value: n }, n)));
    }
  }
}

function renderApprovals(list) {
  const box = $("#approvals");
  if (!box) return;
  box.hidden = !list.length;
  box.replaceChildren(
    ...list.map((r) =>
      el(
        "div",
        { class: "approval" },
        el("span", {}, "Agent “" + r.agent + "” wants to " + r.summary),
        el("button", { type: "button", onclick: () => invoke("resolve_approval", { id: r.id, approve: true }) }, "Allow"),
        el("button", { type: "button", onclick: () => invoke("resolve_approval", { id: r.id, approve: false }) }, "Deny"),
      ),
    ),
  );
}

function renderLogs() {
  const filter = isPanel ? $("#filter").value : "";
  const errorsOnly = isPanel && $("#errors").checked;
  const lines = logLines
    .filter((l) => (!filter || l.service === filter) && (!errorsOnly || l.stream !== "stdout"))
    .slice(isPanel ? -2000 : -14)
    .map((l) => l.ts.slice(11, 19) + " " + l.service.padStart(10) + " │ " + l.text);
  const pre = $("#logs");
  const atBottom = pre.scrollTop + pre.clientHeight >= pre.scrollHeight - 20;
  pre.textContent = lines.join("\n");
  if (atBottom) pre.scrollTop = pre.scrollHeight;
}

function subtitle(s) {
  if (isConsole) {
    const parts = [s.mode + " mode"];
    if (s.services.length) parts.push(s.services.length + " service(s)");
    if (s.databases && s.databases.length) parts.push(s.databases.length + " database(s)");
    if (s.capabilities && s.capabilities.length) parts.push(s.capabilities.length + " function(s) for AI agents");
    return parts.join(" · ");
  }
  if (isPanel) {
    return s.mode + " mode · control API " + s.control_url + " · " + s.pages + " page(s) bridged";
  }
  const fe = s.services.find((x) => x.name === "frontend");
  const failed = s.services.filter((x) => x.state === "failed" || x.state === "crashed");
  if (failed.length) return failed.map((f) => f.name).join(", ") + " failed – see the logs below";
  if (fe && fe.state === "setup") return "Installing frontend dependencies (first start only)…";
  if (!fe) return "No frontend configured – services are running in the background";
  return "Starting services…";
}

// What each scope lets an agent do (runtime panel, "AI agents").
const SCOPES = [
  ["read", "status, logs, configuration, capabilities, read-only databases"],
  ["ui", "look at and operate the app's interface, notifications"],
  ["actions", "call the app's functions that change data"],
  ["services", "start, stop and restart services"],
  ["files", "read project files"],
  ["code", "write project files"],
  ["system", "run declared tasks, evaluate JavaScript"],
];

function renderAgents(a) {
  if (!a || !$("#permissions")) return;
  const on = a.enabled;
  $("#perm-summary").textContent = on
    ? "AI agents \u2014 " + (a.granted.length ? a.granted.join(", ") : "nothing granted") + " (" + a.mode + ")"
    : "AI agents \u2014 off";
  $("#perm-note").textContent = on
    ? "What agents may do. Agent sessions and paired devices get at most this, limited to their own scopes."
    : "Agents cannot connect: MCP, the tools API and the UI bridge are closed. Set \"agents\": { \"enabled\": true } in openshell.json to open them.";
  $("#connect").hidden = !on;
  const yes = (list, s) => (list.includes(s) ? "\u2713" : "\u2013");
  $("#perm-table").hidden = !on;
  if (on) {
    $("#perm-table").replaceChildren(
      el("tr", {}, el("th", {}, "scope"), el("th", {}, "lets an agent \u2026"), el("th", {}, "dev"), el("th", {}, "prod"), el("th", {}, "approval")),
      ...SCOPES.map(([s, what]) =>
        el(
          "tr",
          { class: a.granted.includes(s) ? "on" : "" },
          el("td", {}, el("code", {}, s)),
          el("td", { class: "muted" }, what),
          el("td", {}, yes(a.by_mode.development, s)),
          el("td", {}, yes(a.by_mode.production, s)),
          el("td", {}, (a.approval_scopes || []).includes(s) ? "asks" : ""),
        ),
      ),
    );
  }
  const items = [];
  for (const s of a.sessions || []) {
    items.push(el("li", {}, el("strong", {}, "session " + s.name), " (" + s.profile + ") \u2014 ", s.effective.length ? s.effective.join(", ") : "nothing"));
  }
  for (const d of a.devices || []) {
    items.push(el("li", {}, el("strong", {}, "device " + d.name), " \u2014 ", d.effective.length ? d.effective.join(", ") : "nothing"));
  }
  if (on && !items.length) {
    items.push(el("li", { class: "muted" }, "No agent sessions: agents use the admin token. Give each its own scopes with `openshell agents token <name>`."));
  }
  $("#perm-sessions").replaceChildren(...items);
}

async function tick() {
  try {
    const s = await invoke("get_state", { since: lastSeq || null });
    $("#app").textContent = s.app || "OpenShell";
    document.title = isPanel && !isConsole ? (s.app || "OpenShell") + " – Runtime" : s.app || "OpenShell";
    $("#error").hidden = !s.error;
    if (s.error) {
      $("#error").textContent = s.error;
      $("#subtitle").textContent = "OpenShell could not start the application";
    }
    if (s.services) {
      renderServices(s.services);
      $("#subtitle").textContent = subtitle(s);
      const fe = s.services.find((x) => x.name === "frontend");
      if (!isPanel) $("#open").hidden = !(fe && fe.state === "running");
      renderApprovals(s.approvals || []);
    }
    if (isPanel && s.focus_service) {
      const select = $("#filter");
      if (![...select.options].some((o) => o.value === s.focus_service)) {
        select.append(el("option", { value: s.focus_service }, s.focus_service));
      }
      select.value = s.focus_service;
    }
    if (isPanel && s.agents) renderAgents(s.agents);
    if (isPanel && s.mcp_commands && $("#mcp-app")) {
      $("#mcp-app").textContent = s.mcp_commands[0];
      $("#mcp-full").textContent = s.mcp_commands[1];
    }
    if (isConsole && s.services) renderConsole(s);
    if (s.logs) {
      logLines = logLines.concat(s.logs).slice(-5000);
      lastSeq = s.last_seq;
      renderLogs();
    }
  } catch (e) {
    console.error(e);
  }
  setTimeout(tick, 600);
}

$("#open")?.addEventListener("click", () => invoke("open_app").catch((e) => alert(e)));
$("#panel")?.addEventListener("click", () => invoke("open_runtime_panel").catch((e) => alert(e)));
$("#filter")?.addEventListener("change", renderLogs);
$("#errors")?.addEventListener("change", renderLogs);
// Overlay title bar (`window.title_bar: "overlay"`): the system draws none on Windows and Linux, so
// this page brings a minimal one to move, minimize, maximize and close the window while the app starts.
function titleBar() {
  const win = window.__OPENSHELL__ && window.__OPENSHELL__.window;
  if ((isPanel && !isConsole) || !win || win.title_bar !== "overlay") return;
  const svg = (path) => {
    const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
    s.setAttribute("viewBox", "0 0 10 10");
    s.innerHTML = path;
    return s;
  };
  const bar = el("div", { class: "titlebar " + win.os, "data-tauri-drag-region": "deep" }, el("span", { class: "title" }, ""));
  bar.addEventListener("contextmenu", (e) => {
    e.preventDefault();
    win.showMenu().catch(console.error);
  });
  if (win.os !== "macos") {
    bar.append(
      el("button", { type: "button", "aria-label": "Minimize", onclick: () => win.minimize() }, svg('<path d="M0 5h10"/>')),
      el("button", { type: "button", "aria-label": "Maximize", onclick: () => win.toggleMaximize() }, svg('<rect x="0.5" y="0.5" width="9" height="9"/>')),
      el("button", { type: "button", class: "close", "aria-label": "Close", onclick: () => win.close() }, svg('<path d="M0 0l10 10M10 0L0 10"/>')),
    );
  }
  document.body.prepend(bar);
}

titleBar();
tick();

// --- console: databases and functions ----------------------------------------------------------
let consoleSignature = "";

function renderConsole(s) {
  $("#services-title").hidden = !s.services.length;
  const dbs = s.databases || [];
  const fns = s.capabilities || [];
  // rebuild only when the structure changes, so inputs keep their content
  const signature = JSON.stringify([dbs.map((d) => [d.name, d.tables.map((t) => [t.name, t.rows])]), fns.map((f) => f.name)]);
  if (signature === consoleSignature) return;
  consoleSignature = signature;

  $("#databases-title").hidden = !dbs.length;
  $("#databases").replaceChildren(
    ...dbs.map((d) =>
      el(
        "div",
        { class: "db" },
        el("strong", {}, d.name),
        el("span", { class: "muted" }, " · agents: " + d.agent_access + " · " + d.path),
        el(
          "div",
          { class: "chips" },
          ...(d.tables.length
            ? d.tables.map((t) =>
                el("button", { type: "button", onclick: () => showTable(d.name, t.name) }, t.name + " (" + t.rows + ")"),
              )
            : [el("span", { class: "muted" }, "no tables yet – add a migration")]),
        ),
      ),
    ),
  );

  $("#functions-title").hidden = !fns.length;
  $("#functions").replaceChildren(
    ...fns.map((f) => {
      const input = el("textarea", { rows: "2", spellcheck: "false", "aria-label": "Input for " + f.name }, "{}");
      const output = el("pre", { class: "fn-output muted" });
      const run = el(
        "button",
        {
          type: "button",
          onclick: async () => {
            output.textContent = "…";
            try {
              const result = await invoke("call_function", { name: f.name, input: JSON.parse(input.value || "{}") });
              output.textContent = typeof result === "string" ? result : JSON.stringify(result, null, 2);
            } catch (e) {
              output.textContent = "Error: " + e;
            }
          },
        },
        "Run",
      );
      return el(
        "div",
        { class: "fn" },
        el("div", {}, el("code", {}, f.name), " ", el("span", { class: "muted" }, f.description + " · " + f.permission + " · " + f.source)),
        el("div", { class: "fn-row" }, input, run),
        output,
      );
    }),
  );
}

async function showTable(database, table) {
  try {
    const r = await invoke("db_rows", { database, table, limit: 100 });
    $("#table-view").hidden = false;
    $("#table-caption").textContent = database + "." + table + " – " + r.row_count + " row(s)" + (r.truncated ? " (first 100)" : "");
    const head = el("tr", {}, ...r.columns.map((c) => el("th", {}, c)));
    const rows = r.rows.map((row) => el("tr", {}, ...r.columns.map((c) => el("td", {}, row[c] === null ? "∅" : String(row[c])))));
    $("#rows").replaceChildren(el("thead", {}, head), el("tbody", {}, ...rows));
  } catch (e) {
    alert(e);
  }
}
