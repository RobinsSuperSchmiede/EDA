// Example backend for OpenShell – C++17 and the platform socket API only (no libraries).
//
// OpenShell builds it with CMake (the `build` step), starts it with the port in $PORT,
// checks /health, restarts it on crashes and - in dev mode - rebuilds and restarts it
// whenever a file in backend/src changes.

#ifdef _WIN32
#define NOMINMAX                 // keep std::min/std::max usable
#define WIN32_LEAN_AND_MEAN
#define _CRT_SECURE_NO_WARNINGS  // std::getenv is fine here
#endif

#include <algorithm>
#include <cctype>
#include <cstdlib>
#include <iostream>
#include <mutex>
#include <sstream>
#include <string>
#include <vector>

#ifdef _WIN32
#include <winsock2.h>
#include <ws2tcpip.h>
using socket_t = SOCKET;
static void close_socket(socket_t s) { closesocket(s); }
#else
#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>
using socket_t = int;
static void close_socket(socket_t s) { close(s); }
#endif

struct Todo {
    int id;
    std::string title;
    bool done;
};

static std::vector<Todo> todos = {
    {1, "Start the app with `openshell dev`", true},
    {2, "Connect an AI agent with `openshell mcp`", false},
};

static std::string quote(const std::string& s) {
    std::string out = "\"";
    for (char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default: out += c;
        }
    }
    return out + "\"";
}

static std::string to_json(const Todo& t) {
    return "{\"id\":" + std::to_string(t.id) + ",\"title\":" + quote(t.title) +
           ",\"done\":" + (t.done ? "true" : "false") + "}";
}

static std::string list_json() {
    std::string out = "[";
    for (size_t i = 0; i < todos.size(); ++i) out += (i ? "," : "") + to_json(todos[i]);
    return out + "]";
}

/// Extract the string value of "title" from a small JSON object.
static std::string parse_title(const std::string& body) {
    auto key = body.find("\"title\"");
    if (key == std::string::npos) return "";
    auto start = body.find('"', body.find(':', key) + 1);
    if (start == std::string::npos) return "";
    std::string out;
    for (size_t i = start + 1; i < body.size(); ++i) {
        char c = body[i];
        if (c == '\\' && i + 1 < body.size()) {
            char n = body[++i];
            out += n == 'n' ? '\n' : n;
        } else if (c == '"') {
            break;
        } else {
            out += c;
        }
    }
    return out;
}

struct Response {
    int status;
    std::string body;
};

static Response route(const std::string& method, const std::string& path, const std::string& body) {
    if (path == "/health") return {200, "{\"status\":\"ok\"}"};
    if (path == "/api/todos" && method == "GET") return {200, list_json()};
    if (path == "/api/stats" && method == "GET") {
        auto done = std::count_if(todos.begin(), todos.end(), [](const Todo& t) { return t.done; });
        return {200, "{\"total\":" + std::to_string(todos.size()) + ",\"done\":" + std::to_string(done) +
                         ",\"open\":" + std::to_string(todos.size() - done) + "}"};
    }
    if (path == "/api/todos" && method == "POST") {
        std::string title = parse_title(body);
        if (title.empty()) return {400, "{\"error\":\"title is required\"}"};
        int id = 1;
        for (auto& t : todos) id = std::max(id, t.id + 1);
        todos.push_back({id, title, false});
        return {201, to_json(todos.back())};
    }
    const std::string prefix = "/api/todos/";
    if (path.rfind(prefix, 0) == 0) {
        std::string rest = path.substr(prefix.size());
        bool toggle = rest.size() > 7 && rest.substr(rest.size() - 7) == "/toggle";
        int id = std::atoi(rest.c_str());
        for (size_t i = 0; i < todos.size(); ++i) {
            if (todos[i].id != id) continue;
            if (toggle && method == "POST") {
                todos[i].done = !todos[i].done;
                return {200, to_json(todos[i])};
            }
            if (!toggle && method == "DELETE") {
                todos.erase(todos.begin() + static_cast<long>(i));
                return {200, "{\"deleted\":1}"};
            }
        }
        return {404, "{\"error\":\"todo not found\"}"};
    }
    return {404, "{\"error\":\"not found\"}"};
}

static const char* reason(int status) {
    switch (status) {
        case 200: return "OK";
        case 201: return "Created";
        case 400: return "Bad Request";
        default: return "Not Found";
    }
}

static void serve_client(socket_t client) {
    std::string request;
    char buf[4096];
    size_t header_end = std::string::npos;
    size_t content_length = 0;
    while (true) {
        int n = recv(client, buf, sizeof(buf), 0);
        if (n <= 0) break;
        request.append(buf, static_cast<size_t>(n));
        if (header_end == std::string::npos) {
            header_end = request.find("\r\n\r\n");
            if (header_end != std::string::npos) {
                std::string lower = request.substr(0, header_end);
                std::transform(lower.begin(), lower.end(), lower.begin(),
                               [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
                auto cl = lower.find("content-length:");
                if (cl != std::string::npos) content_length = std::strtoul(lower.c_str() + cl + 15, nullptr, 10);
            }
        }
        if (header_end != std::string::npos && request.size() >= header_end + 4 + content_length) break;
    }
    if (header_end == std::string::npos) {
        close_socket(client);
        return;
    }
    std::istringstream line(request.substr(0, request.find("\r\n")));
    std::string method, target;
    line >> method >> target;
    std::string path = target.substr(0, target.find('?'));
    std::string body = request.substr(header_end + 4, content_length);
    Response res = route(method, path, body);
    if (path != "/health") std::cout << method << " " << path << " -> " << res.status << std::endl;
    std::string out = "HTTP/1.1 " + std::to_string(res.status) + " " + reason(res.status) +
                      "\r\nContent-Type: application/json\r\nContent-Length: " + std::to_string(res.body.size()) +
                      "\r\nConnection: close\r\n\r\n" + res.body;
    send(client, out.c_str(), static_cast<int>(out.size()), 0);
    close_socket(client);
}

int main() {
#ifdef _WIN32
    WSADATA wsa;
    WSAStartup(MAKEWORD(2, 2), &wsa);
#endif
    const char* port_env = std::getenv("PORT");
    int port = port_env ? std::atoi(port_env) : 8000;
    socket_t server = socket(AF_INET, SOCK_STREAM, 0);
    int yes = 1;
    setsockopt(server, SOL_SOCKET, SO_REUSEADDR, reinterpret_cast<const char*>(&yes), sizeof(yes));
    sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_port = htons(static_cast<unsigned short>(port));
    inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
    if (bind(server, reinterpret_cast<sockaddr*>(&addr), sizeof(addr)) != 0 || listen(server, 16) != 0) {
        std::cerr << "cannot listen on port " << port << std::endl;
        return 1;
    }
    std::cout << "backend (C++) listening on http://127.0.0.1:" << port << std::endl;
    while (true) {
        socket_t client = accept(server, nullptr, nullptr);
        serve_client(client);
    }
}
