// Example backend for OpenShell – Node.js standard library only, no install needed.
import { createServer } from "node:http";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join } from "node:path";

const PORT = Number(process.env.PORT || 8000);
// OpenShell provides a per-app data directory (per user for installed apps).
const DATA_DIR = process.env.OPENSHELL_DATA_DIR || fileURLToPath(new URL("../data/", import.meta.url));
const DATA_FILE = join(DATA_DIR, "todos.json");

function load() {
  try {
    return JSON.parse(readFileSync(DATA_FILE, "utf8"));
  } catch {
    return [
      { id: 1, title: "Start the app with `openshell dev`", done: true },
      { id: 2, title: "Connect an AI agent with `openshell mcp`", done: false },
    ];
  }
}
const save = (todos) => {
  mkdirSync(DATA_DIR, { recursive: true });
  writeFileSync(DATA_FILE, JSON.stringify(todos, null, 2));
};

function send(res, status, body) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(body));
}

async function readJson(req) {
  let data = "";
  for await (const chunk of req) data += chunk;
  try {
    return data ? JSON.parse(data) : {};
  } catch {
    return {};
  }
}

const server = createServer(async (req, res) => {
  const { pathname } = new URL(req.url, "http://localhost");
  if (pathname !== "/health") console.log(`${req.method} ${pathname}`);
  const todos = load();
  if (req.method === "GET" && pathname === "/health") return send(res, 200, { status: "ok" });
  if (req.method === "GET" && pathname === "/api/todos") return send(res, 200, todos);
  if (req.method === "POST" && pathname === "/api/todos") {
    const title = String((await readJson(req)).title || "").trim();
    if (!title) return send(res, 400, { error: "title is required" });
    const todo = { id: Math.max(0, ...todos.map((t) => t.id)) + 1, title, done: false };
    save([...todos, todo]);
    return send(res, 201, todo);
  }
  let m = pathname.match(/^\/api\/todos\/(\d+)\/toggle$/);
  if (req.method === "POST" && m) {
    const todo = todos.find((t) => t.id === Number(m[1]));
    if (!todo) return send(res, 404, { error: "todo not found" });
    todo.done = !todo.done;
    save(todos);
    return send(res, 200, todo);
  }
  m = pathname.match(/^\/api\/todos\/(\d+)$/);
  if (req.method === "DELETE" && m) {
    const remaining = todos.filter((t) => t.id !== Number(m[1]));
    save(remaining);
    return send(res, 200, { deleted: todos.length - remaining.length });
  }
  send(res, 404, { error: "not found" });
});

server.listen(PORT, "127.0.0.1", () => console.log(`backend listening on http://127.0.0.1:${PORT}`));
