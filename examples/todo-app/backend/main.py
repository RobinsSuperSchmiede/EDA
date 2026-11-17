"""Example backend for OpenShell – Python standard library only, no install needed.

OpenShell starts this process, passes the port in $PORT, checks /health,
restarts it on crashes and (in dev mode) whenever a file in backend/ changes.
"""

import json
import os
import re
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

PORT = int(os.environ.get("PORT", "8000"))
# OpenShell provides a per-app data directory (per user for installed apps). Keeping data outside
# backend/ also avoids triggering the dev-mode restart watcher.
DATA_DIR = os.environ.get("OPENSHELL_DATA_DIR") or os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "data")
DATA_FILE = os.path.join(DATA_DIR, "todos.json")
LOCK = threading.Lock()


def load_todos():
    try:
        with open(DATA_FILE, encoding="utf-8") as f:
            return json.load(f)
    except (FileNotFoundError, json.JSONDecodeError):
        return [
            {"id": 1, "title": "Start the app with `openshell dev`", "done": True},
            {"id": 2, "title": "Connect an AI agent with `openshell mcp`", "done": False},
            {"id": 3, "title": "Let the agent add a feature", "done": False},
        ]


def save_todos(todos):
    os.makedirs(os.path.dirname(DATA_FILE), exist_ok=True)
    with open(DATA_FILE, "w", encoding="utf-8") as f:
        json.dump(todos, f, indent=2)


class Handler(BaseHTTPRequestHandler):
    def send_json(self, status, body):
        data = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def read_json(self):
        length = int(self.headers.get("Content-Length") or 0)
        if not length:
            return {}
        try:
            return json.loads(self.rfile.read(length))
        except json.JSONDecodeError:
            return {}

    def do_GET(self):
        path = urlparse(self.path).path
        if path == "/health":
            return self.send_json(200, {"status": "ok"})
        if path == "/api/todos":
            return self.send_json(200, load_todos())
        if path == "/api/stats":
            todos = load_todos()
            done = sum(1 for t in todos if t["done"])
            return self.send_json(200, {"total": len(todos), "done": done, "open": len(todos) - done})
        self.send_json(404, {"error": "not found"})

    def do_POST(self):
        path = urlparse(self.path).path
        with LOCK:
            todos = load_todos()
            if path == "/api/todos":
                title = str(self.read_json().get("title", "")).strip()
                if not title:
                    return self.send_json(400, {"error": "title is required"})
                todo = {"id": max([t["id"] for t in todos] + [0]) + 1, "title": title, "done": False}
                todos.append(todo)
                save_todos(todos)
                return self.send_json(201, todo)
            m = re.fullmatch(r"/api/todos/(\d+)/toggle", path)
            if m:
                for t in todos:
                    if t["id"] == int(m.group(1)):
                        t["done"] = not t["done"]
                        save_todos(todos)
                        return self.send_json(200, t)
                return self.send_json(404, {"error": "todo not found"})
        self.send_json(404, {"error": "not found"})

    def do_DELETE(self):
        m = re.fullmatch(r"/api/todos/(\d+)", urlparse(self.path).path)
        if not m:
            return self.send_json(404, {"error": "not found"})
        with LOCK:
            todos = load_todos()
            remaining = [t for t in todos if t["id"] != int(m.group(1))]
            save_todos(remaining)
        self.send_json(200, {"deleted": len(todos) - len(remaining)})

    def log_message(self, fmt, *args):
        if self.path == "/health":
            return
        print(f"{self.command} {self.path} -> {args[1] if len(args) > 1 else ''}", flush=True)


if __name__ == "__main__":
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"backend listening on http://127.0.0.1:{PORT}", flush=True)
    server.serve_forever()
