// Example backend for OpenShell – Java standard library only (JDK 17+), no Maven/Gradle needed.
//
// OpenShell compiles it (`javac`, the `build` step), starts it with the port in $PORT,
// checks /health, restarts it on crashes and - in dev mode - recompiles and restarts it
// whenever a file in backend/src changes.

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;

import java.io.IOException;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

public class Main {
    record Todo(int id, String title, boolean done) {
        String json() {
            return "{\"id\":" + id + ",\"title\":" + quote(title) + ",\"done\":" + done + "}";
        }
    }

    static final List<Todo> todos = new ArrayList<>();
    static Path dataFile;

    static String quote(String s) {
        StringBuilder b = new StringBuilder("\"");
        for (char c : s.toCharArray()) {
            switch (c) {
                case '"' -> b.append("\\\"");
                case '\\' -> b.append("\\\\");
                case '\n' -> b.append("\\n");
                case '\r' -> b.append("\\r");
                case '\t' -> b.append("\\t");
                default -> {
                    if (c < 0x20) b.append(String.format("\\u%04x", (int) c));
                    else b.append(c);
                }
            }
        }
        return b.append('"').toString();
    }

    static String listJson() {
        List<String> items = new ArrayList<>();
        for (Todo t : todos) items.add(t.json());
        return "[" + String.join(",", items) + "]";
    }

    static final Pattern TODO = Pattern.compile(
            "\\{\"id\":(\\d+),\"title\":\"((?:[^\"\\\\]|\\\\.)*)\",\"done\":(true|false)\\}");
    static final Pattern TITLE = Pattern.compile("\"title\"\\s*:\\s*\"((?:[^\"\\\\]|\\\\.)*)\"");

    static String unescape(String s) {
        return s.replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\");
    }

    static void load() {
        try {
            Matcher m = TODO.matcher(Files.readString(dataFile));
            while (m.find()) {
                todos.add(new Todo(Integer.parseInt(m.group(1)), unescape(m.group(2)), Boolean.parseBoolean(m.group(3))));
            }
        } catch (IOException e) {
            todos.add(new Todo(1, "Start the app with `openshell dev`", true));
            todos.add(new Todo(2, "Connect an AI agent with `openshell mcp`", false));
        }
    }

    static void save() throws IOException {
        Files.createDirectories(dataFile.getParent());
        Files.writeString(dataFile, listJson());
    }

    static void send(HttpExchange ex, int status, String body) throws IOException {
        byte[] bytes = body.getBytes(StandardCharsets.UTF_8);
        ex.getResponseHeaders().set("Content-Type", "application/json");
        ex.sendResponseHeaders(status, bytes.length);
        try (OutputStream out = ex.getResponseBody()) {
            out.write(bytes);
        }
    }

    static synchronized void handle(HttpExchange ex) throws IOException {
        String method = ex.getRequestMethod();
        String path = ex.getRequestURI().getPath();
        if (!path.equals("/health")) System.out.println(method + " " + path);
        if (path.equals("/health")) {
            send(ex, 200, "{\"status\":\"ok\"}");
        } else if (path.equals("/api/todos") && method.equals("GET")) {
            send(ex, 200, listJson());
        } else if (path.equals("/api/stats") && method.equals("GET")) {
            long done = todos.stream().filter(Todo::done).count();
            send(ex, 200, "{\"total\":" + todos.size() + ",\"done\":" + done + ",\"open\":" + (todos.size() - done) + "}");
        } else if (path.equals("/api/todos") && method.equals("POST")) {
            String body = new String(ex.getRequestBody().readAllBytes(), StandardCharsets.UTF_8);
            Matcher m = TITLE.matcher(body);
            String title = m.find() ? unescape(m.group(1)).trim() : "";
            if (title.isEmpty()) {
                send(ex, 400, "{\"error\":\"title is required\"}");
                return;
            }
            int id = todos.stream().mapToInt(Todo::id).max().orElse(0) + 1;
            Todo t = new Todo(id, title, false);
            todos.add(t);
            save();
            send(ex, 201, t.json());
        } else if (path.matches("/api/todos/\\d+/toggle") && method.equals("POST")) {
            int id = Integer.parseInt(path.split("/")[3]);
            for (int i = 0; i < todos.size(); i++) {
                Todo t = todos.get(i);
                if (t.id() == id) {
                    Todo updated = new Todo(id, t.title(), !t.done());
                    todos.set(i, updated);
                    save();
                    send(ex, 200, updated.json());
                    return;
                }
            }
            send(ex, 404, "{\"error\":\"todo not found\"}");
        } else if (path.matches("/api/todos/\\d+") && method.equals("DELETE")) {
            int id = Integer.parseInt(path.split("/")[3]);
            boolean removed = todos.removeIf(t -> t.id() == id);
            save();
            send(ex, 200, "{\"deleted\":" + (removed ? 1 : 0) + "}");
        } else {
            send(ex, 404, "{\"error\":\"not found\"}");
        }
    }

    public static void main(String[] args) throws IOException {
        int port = Integer.parseInt(System.getenv().getOrDefault("PORT", "8000"));
        String dataDir = System.getenv().getOrDefault("OPENSHELL_DATA_DIR", "../data");
        dataFile = Path.of(dataDir, "todos.json");
        load();
        HttpServer server = HttpServer.create(new InetSocketAddress("127.0.0.1", port), 0);
        server.createContext("/", Main::handle);
        server.start();
        System.out.println("backend (Java " + Runtime.version().feature() + ") listening on http://127.0.0.1:" + port);
    }
}
