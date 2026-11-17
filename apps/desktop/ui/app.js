// OpenShell's own pages: loading screen (index.html) and runtime panel (runtime.html).
const invoke = window.__TAURI__.core.invoke;
const isPanel = document.body.classList.contains("panel");
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

async function tick() {
  try {
    const s = await invoke("get_state", { since: lastSeq || null });
    $("#app").textContent = s.app || "OpenShell";
    document.title = isPanel ? (s.app || "OpenShell") + " – Runtime" : s.app || "OpenShell";
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
    if (isPanel && s.mcp_commands && $("#mcp-app")) {
      $("#mcp-app").textContent = s.mcp_commands[0];
      $("#mcp-full").textContent = s.mcp_commands[1];
    }
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
tick();
