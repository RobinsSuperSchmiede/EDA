// Plain JavaScript frontend. OpenShell serves this folder and proxies /api to the backend.

const list = document.querySelector("#todos");
const stats = document.querySelector("#stats");
const errorBox = document.querySelector("#error");
let filter = "all";
let todos = [];

async function api(path, options = {}) {
  const res = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  const body = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(body.error || `HTTP ${res.status}`);
  return body;
}

function showError(err) {
  errorBox.hidden = !err;
  errorBox.textContent = err ? String(err.message || err) : "";
}

function render() {
  const visible = todos.filter((t) => filter === "all" || (filter === "done" ? t.done : !t.done));
  list.replaceChildren(
    ...visible.map((t) => {
      const li = document.createElement("li");
      li.className = t.done ? "done" : "";
      const label = document.createElement("label");
      const box = document.createElement("input");
      box.type = "checkbox";
      box.checked = t.done;
      box.addEventListener("change", () => toggle(t.id));
      label.append(box, " ", t.title);
      const del = document.createElement("button");
      del.type = "button";
      del.className = "delete";
      del.textContent = "×";
      del.setAttribute("aria-label", `Delete ${t.title}`);
      del.addEventListener("click", () => remove(t.id));
      li.append(label, del);
      return li;
    }),
  );
  const done = todos.filter((t) => t.done).length;
  stats.textContent = `${todos.length} todos · ${done} done · ${todos.length - done} open`;
  document.querySelectorAll("nav button").forEach((b) => b.setAttribute("aria-pressed", String(b.dataset.filter === filter)));
}

async function refresh() {
  try {
    todos = await api("/api/todos");
    showError(null);
  } catch (e) {
    showError(e);
  }
  render();
}

async function add(title) {
  await api("/api/todos", { method: "POST", body: JSON.stringify({ title }) });
  await refresh();
}

async function toggle(id) {
  await api(`/api/todos/${id}/toggle`, { method: "POST" });
  await refresh();
}

async function remove(id) {
  await api(`/api/todos/${id}`, { method: "DELETE" });
  await refresh();
}

function setFilter(f) {
  filter = f;
  render();
}

document.querySelector("#new-todo").addEventListener("submit", async (ev) => {
  ev.preventDefault();
  const input = ev.target.elements.title;
  const title = input.value.trim();
  if (!title) return;
  try {
    await add(title);
    input.value = "";
  } catch (e) {
    showError(e);
  }
});
document.querySelectorAll("nav button").forEach((b) => b.addEventListener("click", () => setFilter(b.dataset.filter)));

// Optional: expose frontend functions to AI agents through the OpenShell SDK.
// Outside OpenShell (plain browser) this is skipped and the app works the same.
if (window.__OPENSHELL__) {
  document.querySelector("#host").textContent = window.__OPENSHELL__.host === "desktop" ? "OpenShell (desktop)" : "OpenShell (browser)";
  import(/* @vite-ignore */ `${window.__OPENSHELL__.api}/sdk.js`).then(({ OpenShell }) => {
    OpenShell.agent.registerAction(
      "set_filter",
      {
        description: "Show all, only open or only done todos in the UI",
        permission: "ui",
        input_schema: { type: "object", properties: { filter: { enum: ["all", "open", "done"] } }, required: ["filter"] },
      },
      ({ filter }) => {
        setFilter(filter);
        return { filter, visible: list.children.length };
      },
    );
  });
}

refresh();
