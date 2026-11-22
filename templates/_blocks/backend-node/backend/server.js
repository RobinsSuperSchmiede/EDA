// Backend service of {{app_name}} – Node.js standard library only.
//
// OpenShell starts it with the port in $PORT, checks /health, restarts it on crashes and
// (in development mode) whenever a file in backend/ changes.
import { createServer } from "node:http";

const PORT = Number(process.env.PORT || 8000);
// Set when the app has a database (`openshell add database`): path of the SQLite file.
const DB_PATH = process.env.OPENSHELL_DB_MAIN;

function send(res, status, body) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(body));
}

const server = createServer((req, res) => {
  const { pathname } = new URL(req.url, "http://localhost");
  if (pathname !== "/health") console.log(`${req.method} ${pathname}`);
  if (pathname === "/health") return send(res, 200, { status: "ok" });
  if (pathname === "/api/hello") {
    return send(res, 200, {
      message: "Hello from the backend",
      time: new Date().toISOString(),
      database: Boolean(DB_PATH),
    });
  }
  send(res, 404, { error: "not found" });
});

server.listen(PORT, "127.0.0.1", () => console.log(`backend listening on http://127.0.0.1:${PORT}`));
