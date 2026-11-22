"""Backend service of {{app_name}} – Python standard library only.

OpenShell starts it with the port in $PORT, checks /health, restarts it on crashes and
(in development mode) whenever a file in backend/ changes.
"""

import json
import os
import sqlite3
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

PORT = int(os.environ.get("PORT", "8000"))
# Set when the app has a database (`openshell add database`).
DB_PATH = os.environ.get("OPENSHELL_DB_MAIN")


class Handler(BaseHTTPRequestHandler):
    def send_json(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        path = urlparse(self.path).path
        if path == "/health":
            return self.send_json(200, {"status": "ok"})
        if path == "/api/hello":
            return self.send_json(200, {
                "message": "Hello from the backend",
                "time": datetime.now(timezone.utc).isoformat(),
                "database": bool(DB_PATH),
            })
        if path == "/api/notes" and DB_PATH:
            with sqlite3.connect(DB_PATH) as con:
                con.row_factory = sqlite3.Row
                rows = con.execute("SELECT * FROM notes ORDER BY id DESC").fetchall()
            return self.send_json(200, [dict(r) for r in rows])
        self.send_json(404, {"error": "not found"})

    def log_message(self, fmt, *args):
        if self.path != "/health":
            print(f"{self.command} {self.path} -> {args[1] if len(args) > 1 else ''}", flush=True)


if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"backend listening on http://127.0.0.1:{PORT}", flush=True)
    server.serve_forever()
