// Starter frontend. It adapts to the building blocks the app has: a backend (/api),
// a database (through the OpenShell SDK) and functions for AI agents.

const $ = (s) => document.querySelector(s);

function showError(err) {
  $("#error").hidden = !err;
  $("#error").textContent = err ? String(err.message || err) : "";
}

// The OpenShell SDK is available inside OpenShell (window and browser mode);
// in a plain browser this resolves to null and the page still works.
async function openshell() {
  if (!window.__OPENSHELL__) return null;
  const { OpenShell } = await import(/* @vite-ignore */ `${window.__OPENSHELL__.api}/sdk.js`);
  return OpenShell;
}

// --- backend ----------------------------------------------------------------
async function setupBackend() {
  const res = await fetch("/api/hello").catch(() => null);
  // no backend: the request fails, or OpenShell's static server answers with index.html (SPA fallback)
  if (!res || !res.ok || !(res.headers.get("content-type") || "").includes("json")) return;
  $("#backend").hidden = false;
  $("#hello").addEventListener("click", async () => {
    const r = await fetch("/api/hello");
    $("#hello-result").textContent = JSON.stringify(await r.json(), null, 2);
  });
}

// --- database ---------------------------------------------------------------
async function setupNotes(os) {
  if (!os) return;
  const { databases } = await os.call("db_list").catch(() => ({ databases: [] }));
  const hasNotes = databases.some((d) => d.tables.some((t) => t.name === "notes"));
  if (!hasNotes) return;
  $("#notes").hidden = false;

  async function refresh() {
    const { rows } = await os.db.query("SELECT id, title FROM notes ORDER BY id DESC");
    $("#note-list").replaceChildren(
      ...rows.map((n) => {
        const li = document.createElement("li");
        li.textContent = n.title;
        return li;
      }),
    );
  }

  $("#note-form").addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const input = ev.target.elements.note;
    if (!input.value.trim()) return;
    try {
      await os.db.execute("INSERT INTO notes (title) VALUES (?)", [input.value.trim()]);
      input.value = "";
      await refresh();
    } catch (e) {
      showError(e);
    }
  });
  await refresh();
}

// --- functions --------------------------------------------------------------
async function setupFunctions(os) {
  if (!os) return;
  const { capabilities } = await os.call("capabilities_list").catch(() => ({ capabilities: [] }));
  if (!capabilities.length) return;
  $("#functions").hidden = false;
  $("#function-list").replaceChildren(
    ...capabilities.map((c) => {
      const li = document.createElement("li");
      li.innerHTML = `<code></code> <span class="muted"></span>`;
      li.querySelector("code").textContent = c.name;
      li.querySelector("span").textContent = c.description;
      return li;
    }),
  );
}

const os = await openshell();
await Promise.all([setupBackend(), setupNotes(os), setupFunctions(os)]).catch(showError);
