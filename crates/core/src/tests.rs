use super::*;
use crate::state::ControlClient;
use openshell_config::parse_json;
use serde_json::{json, Value};
use std::time::Duration;

fn sleep_cmd() -> &'static str {
    if cfg!(windows) {
        "ping -n 60 127.0.0.1 > NUL"
    } else {
        "sleep 60"
    }
}

/// The tests exercise the agent layer, which is opt-in (`agents.enabled`).
fn with_agents(mut config: openshell_config::Config) -> openshell_config::Config {
    config.agents.enabled = true;
    config
}

async fn launch(extra: &str, mode: Mode) -> (tempfile::TempDir, Shell, ControlClient) {
    let dir = tempfile::tempdir().unwrap();
    let json = format!(
        r#"{{"app":{{"name":"Test App"}},"services":[{{"name":"api","command":"{}"}}]{extra}}}"#,
        sleep_cmd()
    );
    let config = with_agents(parse_json(&json).unwrap());
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config,
        state_dir_override: None,
    };
    let shell = Shell::launch(project, mode, Arc::new(HeadlessHost::default())).await.unwrap();
    shell.start();
    shell.runtime().wait_ready("api", Duration::from_secs(10)).await.unwrap();
    let client =
        ControlClient::connect(&dir.path().join(".openshell")).await.unwrap().with_agent("test-agent");
    (dir, shell, client)
}

async fn tool(client: &ControlClient, name: &str, args: Value) -> Value {
    client.post(&format!("/v1/tools/{name}"), &args).await.unwrap()
}

#[tokio::test]
async fn control_api_and_tools() {
    let (dir, shell, client) = launch("", Mode::Development).await;

    let status = client.get("/v1/status").await.unwrap();
    assert_eq!(status["app"], "Test App");
    assert_eq!(status["services"][0]["state"], "running");

    let r = tool(&client, "runtime_status", json!({ "service": "api" })).await;
    assert_eq!(r["ok"], true);
    assert_eq!(r["result"]["state"], "running");

    let r = tool(&client, "fs_write", json!({ "path": "src/hello.txt", "content": "hi" })).await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(std::fs::read_to_string(dir.path().join("src/hello.txt")).unwrap(), "hi");

    let r = tool(&client, "fs_read", json!({ "path": "../secret" })).await;
    assert_eq!(r["ok"], false);

    let r = tool(&client, "task_run", json!({ "name": "x" })).await;
    assert_eq!(r["ok"], false);
    assert!(r["result"]["error"].as_str().unwrap().contains("not granted"));

    let r = tool(&client, "service_restart", json!({ "name": "api" })).await;
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["status"]["restarts"], 1);

    let r = tool(&client, "ui_snapshot", json!({})).await;
    assert_eq!(r["ok"], false);
    assert!(r["result"]["error"].as_str().unwrap().contains("no application page"));

    let audit = client.get("/v1/audit").await.unwrap();
    let entries = audit["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["tool"] == "fs_write" && e["agent"] == "test-agent"));
    assert!(entries.iter().any(|e| e["tool"] == "task_run" && e["outcome"] == "denied"));
    assert!(dir.path().join(".openshell/audit.jsonl").exists());

    shell.shutdown().await;
    assert!(!dir.path().join(".openshell/runtime.json").exists());
}

#[tokio::test]
async fn mcp_over_http() {
    let (_dir, shell, client) = launch("", Mode::Development).await;
    let init = client
        .post(
            "/mcp",
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"claude-code","version":"1"}}}),
        )
        .await
        .unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "openshell");
    let list = client.post("/mcp", &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await.unwrap();
    assert!(list["result"]["tools"].as_array().unwrap().len() > 20);
    let call = client
        .post(
            "/mcp",
            &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"logs_read","arguments":{"service":"api"}}}),
        )
        .await
        .unwrap();
    assert_eq!(call["result"]["isError"], false);
    assert!(call["result"]["content"][0]["text"].as_str().unwrap().contains("starting"));
    shell.shutdown().await;
}

#[tokio::test]
async fn production_policy_and_approvals() {
    let (dir, shell, client) = launch(
        r#","agents":{"permissions":{"production":["read","code"]},"approval_required":["code"]}"#,
        Mode::Production,
    )
    .await;

    let r = tool(&client, "service_restart", json!({ "name": "api" })).await;
    assert_eq!(r["ok"], false);
    assert!(r["result"]["error"].as_str().unwrap().contains("production"));

    // fs_write needs approval: approve it from "another client"
    let c2 = client.clone();
    let call = tokio::spawn(async move {
        tool(&c2, "fs_write", json!({ "path": "a.txt", "content": "approved" })).await
    });
    let id = loop {
        let pending = client.get("/v1/approvals").await.unwrap();
        if let Some(id) = pending["pending"][0]["id"].as_str() {
            assert_eq!(pending["pending"][0]["summary"], "write a.txt (8 bytes)");
            break id.to_string();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    client.post(&format!("/v1/approvals/{id}/approve"), &json!({})).await.unwrap();
    let r = call.await.unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "approved");

    // deny the next one
    let c2 = client.clone();
    let call =
        tokio::spawn(async move { tool(&c2, "fs_write", json!({ "path": "b.txt", "content": "x" })).await });
    let id = loop {
        let pending = client.get("/v1/approvals").await.unwrap();
        if let Some(id) = pending["pending"][0]["id"].as_str() {
            break id.to_string();
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    client.post(&format!("/v1/approvals/{id}/deny"), &json!({})).await.unwrap();
    let r = call.await.unwrap();
    assert_eq!(r["ok"], false);
    assert!(!dir.path().join("b.txt").exists());
    shell.shutdown().await;
}

#[tokio::test]
async fn auth_and_origin_checks() {
    let (_dir, shell, client) = launch("", Mode::Development).await;
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let base = &client.info.url;

    let r = http.get(format!("{base}/v1/status")).send().await.unwrap();
    assert_eq!(r.status(), 401);

    let r = http.get(format!("{base}/v1/status")).bearer_auth(&shell.inner.app_token).send().await.unwrap();
    assert_eq!(r.status(), 403);

    let r = http.get(format!("{base}/v1/sdk/info")).bearer_auth(&shell.inner.app_token).send().await.unwrap();
    assert_eq!(r.status(), 200);

    let r = http
        .get(format!("{base}/v1/status"))
        .bearer_auth(&client.info.token)
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    let r = http.get(format!("{base}/v1/health")).header("Host", "evil.example").send().await.unwrap();
    assert_eq!(r.status(), 403);

    // the app token can call permitted SDK tools only
    let r: Value = http
        .post(format!("{base}/v1/sdk/call/runtime_status"))
        .bearer_auth(&shell.inner.app_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["ok"], true);
    let r: Value = http
        .post(format!("{base}/v1/sdk/call/service_stop"))
        .bearer_auth(&shell.inner.app_token)
        .json(&json!({ "name": "api" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["ok"], false);
    shell.shutdown().await;
}

/// A tiny backend with an OpenAPI document, served by axum inside the test.
async fn openapi_backend() -> u16 {
    use axum::routing::get;
    let doc = json!({
        "openapi": "3.0.3",
        "paths": {
            "/items/{id}": { "get": {
                "operationId": "getItem", "summary": "Get an item",
                "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "integer" } }]
            } },
            "/items": { "post": {
                "operationId": "createItem", "summary": "Create an item",
                "requestBody": { "content": { "application/json": { "schema": {
                    "type": "object", "required": ["name"], "properties": { "name": { "type": "string" } } } } } }
            } }
        }
    });
    let app = axum::Router::new()
        .route(
            "/openapi.json",
            get(move || {
                let d = doc.clone();
                async move { axum::Json(d) }
            }),
        )
        .route(
            "/items/{id}",
            get(|axum::extract::Path(id): axum::extract::Path<u32>| async move {
                axum::Json(json!({ "id": id, "name": format!("item {id}") }))
            }),
        )
        .route(
            "/items",
            axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
                axum::Json(json!({ "created": body["name"] }))
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

#[tokio::test]
async fn openapi_import_and_app_profile() {
    let port = openapi_backend().await;
    let extra = r#","capabilities":[{"name":"ping","description":"Ping","task":"ping"}],"tasks":{"ping":"echo pong"}}"#.to_string();
    // second service: an external backend with an OpenAPI document
    let dir = tempfile::tempdir().unwrap();
    let json = format!(
        r#"{{"app":{{"name":"Api App","description":"demo"}},"services":[
            {{"name":"api","command":"{}"}},
            {{"name":"backend","external":true,"url":"http://127.0.0.1:{port}","healthcheck":{{"http":"http://127.0.0.1:{port}/openapi.json","interval_ms":200}},"openapi":"/openapi.json"}}
        ]{extra}"#,
        sleep_cmd()
    );
    let config = with_agents(parse_json(&json).unwrap());
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config,
        state_dir_override: None,
    };
    let shell = Shell::launch(project, Mode::Production, Arc::new(HeadlessHost::default())).await.unwrap();
    shell.start();
    shell.runtime().wait_ready("backend", Duration::from_secs(10)).await.unwrap();
    shell.runtime().wait_ready("api", Duration::from_secs(10)).await.unwrap();
    let client = ControlClient::connect(&dir.path().join(".openshell")).await.unwrap();

    // wait for the import
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let tools = loop {
        let list =
            client.post("/mcp/app", &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await.unwrap();
        let names: Vec<String> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        if names.contains(&"get_item".to_string()) || std::time::Instant::now() > deadline {
            break names;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(tools.contains(&"app_status".to_string()), "{tools:?}");
    assert!(tools.contains(&"get_item".to_string()), "{tools:?}");
    assert!(tools.contains(&"create_item".to_string()), "{tools:?}");
    assert!(tools.contains(&"ping".to_string()), "{tools:?}");
    assert!(!tools.contains(&"fs_write".to_string()), "developer tools must not be in the app profile");

    let call = client
        .post("/mcp/app", &json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_item","arguments":{"id":7}}}))
        .await
        .unwrap();
    assert_eq!(call["result"]["structuredContent"]["name"], "item 7", "{call}");
    let call = client
        .post("/mcp/app", &json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create_item","arguments":{"name":"pen"}}}))
        .await
        .unwrap();
    assert_eq!(call["result"]["structuredContent"]["created"], "pen", "{call}");
    // `ping` runs a task -> needs `system`, which production does not grant
    let call = client
        .post("/mcp/app", &json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"app_status","arguments":{}}}))
        .await
        .unwrap();
    assert_eq!(call["result"]["structuredContent"]["healthy"], true, "{call}");
    let init = client
        .post("/mcp/app", &json!({"jsonrpc":"2.0","id":5,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"x","version":"1"}}}))
        .await
        .unwrap();
    assert!(init["result"]["instructions"].as_str().unwrap().contains("Api App"));
    shell.shutdown().await;
}

/// An app without frontend and backend: a database plus script functions only.
#[tokio::test]
async fn database_and_functions_without_services() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("db")).unwrap();
    std::fs::write(dir.path().join("db/001.sql"), "CREATE TABLE notes (id INTEGER PRIMARY KEY, title TEXT);")
        .unwrap();
    let script = "import sys, json, os, sqlite3\n\
                  data = json.load(sys.stdin)\n\
                  con = sqlite3.connect(os.environ['OPENSHELL_DB_MAIN'])\n\
                  con.execute('INSERT INTO notes (title) VALUES (?)', (data['title'],))\n\
                  con.commit()\n\
                  print(json.dumps({'saved': data['title'], 'count': con.execute('SELECT COUNT(*) FROM notes').fetchone()[0]}))";
    std::fs::write(dir.path().join("save.py"), script).unwrap();
    std::fs::write(dir.path().join("fail.py"), "import sys\nprint('boom', file=sys.stderr)\nsys.exit(3)")
        .unwrap();
    let cfg = json!({
        "app": { "name": "Data App" },
        "databases": [{ "name": "main", "migrations": "db", "agent_access": "write" }],
        "capabilities": [
            { "name": "save_note", "description": "Save a note", "permission": "actions", "run": ["python", "save.py"] },
            { "name": "fail", "description": "Always fails", "run": ["python", "fail.py"] }
        ]
    });
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config: with_agents(parse_json(&cfg.to_string()).unwrap()),
        state_dir_override: None,
    };
    let shell = Shell::launch(project, Mode::Production, Arc::new(HeadlessHost::default())).await.unwrap();
    shell.start();
    let client = ControlClient::connect(&dir.path().join(".openshell")).await.unwrap();

    // function through the app profile
    let call = client
        .post("/mcp/app", &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"save_note","arguments":{"title":"hello"}}}))
        .await
        .unwrap();
    assert_eq!(call["result"]["structuredContent"]["saved"], "hello", "{call}");
    assert_eq!(call["result"]["structuredContent"]["count"], 1);

    let fail = tool(&client, "cap_fail", json!({})).await;
    assert_eq!(fail["ok"], false);
    assert!(fail["result"]["error"].as_str().unwrap().contains("boom"), "{fail}");

    // database tools (app profile lists them because agent_access = write)
    let list = client.post("/mcp/app", &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await.unwrap();
    let names: Vec<&str> =
        list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["app_status", "save_note", "db_query", "db_execute", "db_schema"] {
        assert!(names.contains(&n), "{names:?}");
    }
    let q = tool(&client, "db_query", json!({ "sql": "SELECT title FROM notes" })).await;
    assert_eq!(q["result"]["rows"][0]["title"], "hello", "{q}");
    let bad = tool(&client, "db_query", json!({ "sql": "DELETE FROM notes" })).await;
    assert_eq!(bad["ok"], false);
    let w = tool(
        &client,
        "db_execute",
        json!({ "sql": "INSERT INTO notes (title) VALUES (?)", "params": ["two"] }),
    )
    .await;
    assert_eq!(w["result"]["changes"], 1, "{w}");

    // resources
    let res =
        client.post("/mcp/app", &json!({"jsonrpc":"2.0","id":3,"method":"resources/list"})).await.unwrap();
    assert!(res["result"]["resources"].to_string().contains("openshell://db/main/schema"));
    let read = client
        .post("/mcp", &json!({"jsonrpc":"2.0","id":4,"method":"resources/read","params":{"uri":"openshell://db/main/schema"}}))
        .await
        .unwrap();
    assert!(read["result"]["contents"][0]["text"].as_str().unwrap().contains("notes"));

    let status = client.get("/v1/status").await.unwrap();
    assert_eq!(status["databases"][0]["tables"][0]["rows"], 2);
    assert_eq!(status["capabilities"].as_array().unwrap().len(), 2);
    shell.shutdown().await;
}

#[tokio::test]
async fn database_agent_access_is_enforced() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = json!({ "app": { "name": "x" }, "databases": [{ "name": "secret", "agent_access": "none" }, { "name": "main" }] });
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config: with_agents(parse_json(&cfg.to_string()).unwrap()),
        state_dir_override: None,
    };
    let shell = Shell::launch(project, Mode::Development, Arc::new(HeadlessHost::default())).await.unwrap();
    let client = ControlClient::connect(&dir.path().join(".openshell")).await.unwrap();
    let r = tool(&client, "db_query", json!({ "sql": "SELECT 1", "database": "secret" })).await;
    assert!(r["result"]["error"].as_str().unwrap().contains("not accessible"), "{r}");
    let r = tool(&client, "db_execute", json!({ "sql": "CREATE TABLE t (x)" })).await;
    assert!(r["result"]["error"].as_str().unwrap().contains("read-only"), "{r}");
    let list = tool(&client, "db_list", json!({})).await;
    assert_eq!(list["result"]["databases"].as_array().unwrap().len(), 1);

    // the app's own frontend (SDK channel) may write – governed by sdk_permissions
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let r: Value = http
        .post(format!("{}/v1/sdk/call/db_query", client.info.url))
        .bearer_auth(&shell.inner.app_token)
        .json(&json!({ "sql": "SELECT 1 AS one", "database": "secret" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["result"]["rows"][0]["one"], 1, "{r}");
    shell.shutdown().await;
}

#[tokio::test]
async fn platform_and_lifecycle_in_the_control_api() {
    let (_dir, shell, client) = launch("", Mode::Development).await;
    let status = client.get("/v1/status").await.unwrap();
    assert_eq!(status["platform"]["platform"], "desktop");
    assert_eq!(status["platform"]["processes"], "full");
    assert_eq!(status["in_background"], false);
    assert_eq!(status["remote"], Value::Null);

    let r = client.post("/v1/lifecycle", &json!({ "kind": "background" })).await.unwrap();
    assert_eq!(r["ok"], true);
    assert_eq!(r["services"], json!([]));
    assert_eq!(r["keep_alive"], json!(["api"]));
    assert!(shell.runtime().in_background());
    let r =
        client.post("/v1/lifecycle", &json!({ "kind": "network", "connected": true, "metered": true })).await;
    assert!(r.is_ok());
    assert!(client.post("/v1/lifecycle", &json!({ "kind": "nap" })).await.is_err());

    let r = client.get("/v1/remote").await.unwrap();
    assert_eq!(r["enabled"], false);

    let info = tool(&client, "app_info", json!({})).await;
    assert_eq!(info["result"]["platform"]["platform"], "desktop", "{info}");
    shell.shutdown().await;
}

#[tokio::test]
async fn remote_gateway_requires_the_pairing_token() {
    let port = openshell_runtime::process::free_port().unwrap();
    let extra = format!(
        r#","runtime":{{"remote":{{"enabled":true,"bind":"127.0.0.1","port":{port}}}}},
        "capabilities":[{{"name":"ping","description":"Answer pong","permission":"read","run":{{"command":"echo pong"}}}}]"#
    );
    let (dir, shell, client) = launch(&extra, Mode::Production).await;
    let remote = client.get("/v1/remote").await.unwrap();
    assert_eq!(remote["enabled"], true);
    let token = remote["token"].as_str().unwrap().to_string();
    assert!(dir.path().join(".openshell/data").join(remote::TOKEN_FILE).is_file());
    let base = format!("http://127.0.0.1:{port}");
    assert_eq!(remote["urls"][0], base);

    let http = reqwest::Client::new();
    let health: Value = http.get(format!("{base}/v1/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["app"], "Test App");

    let r = http.get(format!("{base}/v1/remote/info")).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = http.get(format!("{base}/v1/remote/info")).bearer_auth("wrong").send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = http
        .get(format!("{base}/v1/remote/info"))
        .bearer_auth(&token)
        .header("origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    // the admin token of the local control API is not valid remotely
    let admin = std::fs::read_to_string(dir.path().join(".openshell/runtime.json")).unwrap();
    let admin: Value = serde_json::from_str(&admin).unwrap();
    let r = http
        .get(format!("{base}/v1/remote/info"))
        .bearer_auth(admin["token"].as_str().unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    // developer routes do not exist on the gateway
    let r = http.get(format!("{base}/v1/status")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status(), 404);

    let info: Value = http
        .get(format!("{base}/v1/remote/info"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(info["app"]["name"], "Test App");
    assert_eq!(info["functions"][0]["name"], "ping");

    let mcp = |body: Value| http.post(format!("{base}/mcp/app")).bearer_auth(&token).json(&body).send();
    let init: Value = mcp(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"laptop-agent","version":"1"}}}))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(init["result"]["serverInfo"].is_object(), "{init}");
    let list: Value =
        mcp(json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).await.unwrap().json().await.unwrap();
    let names: Vec<&str> =
        list["result"]["tools"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"ping"), "{names:?}");
    assert!(!names.contains(&"fs_write"), "{names:?}");
    let call: Value =
        mcp(json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ping","arguments":{}}}))
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
    assert!(call.to_string().contains("pong"), "{call}");

    // calls are audited with the remote channel
    let audit = client.get("/v1/audit?limit=5").await.unwrap();
    assert!(audit["entries"].as_array().unwrap().iter().any(|e| e["channel"] == "remote"), "{audit}");
    shell.shutdown().await;
}

/// A tiny HTTP server answering every request with `hello from <name>` (stands in for a service).
async fn hello_server(name: &'static str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let body = format!("hello from {name}; cookie-sent={}", req.contains("openshell_token"));
                let _ = s
                    .write_all(
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await;
            });
        }
    });
    port
}

#[tokio::test]
async fn remote_devices_exposed_services_and_lockout() {
    let gw_port = openshell_runtime::process::free_port().unwrap();
    let http_port = openshell_runtime::process::free_port().unwrap();
    let tcp_port = openshell_runtime::process::free_port().unwrap();
    let backend = hello_server("web").await;
    let extra = format!(
        r#","runtime":{{"remote":{{"enabled":true,"bind":"127.0.0.1","port":{gw_port},
            "rate_limit":{{"max_failed_auth":3,"block_minutes":1}},
            "expose":[{{"service":"web","port":{http_port}}},
                      {{"service":"web","port":{tcp_port},"protocol":"tcp","allow":["127.0.0.1"]}}]}}}},
        "capabilities":[{{"name":"ping","description":"pong","permission":"read","run":{{"command":"echo pong"}}}}]"#
    );
    let extra = extra.replace(
        r#","runtime""#,
        &format!(
            r#","services":[{{"name":"api","command":"{}"}},{{"name":"web","external":true,"url":"http://127.0.0.1:{backend}"}}],"runtime""#,
            sleep_cmd()
        ),
    );
    // `launch` adds its own services list; build the project by hand instead
    let dir = tempfile::tempdir().unwrap();
    let json = format!(r#"{{"app":{{"name":"Mesh App"}}{extra}}}"#);
    let config = with_agents(parse_json(&json).unwrap());
    let issues = openshell_config::validate(&config);
    assert!(issues.iter().all(|i| !i.is_error()), "{issues:?}");
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config,
        state_dir_override: None,
    };
    let shell = Shell::launch(project, Mode::Production, Arc::new(HeadlessHost::default())).await.unwrap();
    shell.start();
    let client = ControlClient::connect(&dir.path().join(".openshell")).await.unwrap();
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    let base = format!("http://127.0.0.1:{gw_port}");

    // pair a device, use it, revoke it
    let pair = client.post("/v1/remote/devices", &json!({ "name": "laptop" })).await.unwrap();
    let device_token = pair["token"].as_str().unwrap().to_string();
    assert!(pair["pairing_uri"].as_str().unwrap().starts_with("openshell://pair?app=Mesh%20App&url="));
    let r = http.get(format!("{base}/v1/remote/info")).bearer_auth(&device_token).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let info: Value = r.json().await.unwrap();
    assert_eq!(info["exposed"][0]["service"], "web");
    let devices = client.get("/v1/remote/devices").await.unwrap();
    assert_eq!(devices["devices"][0]["name"], "laptop");
    assert!(devices["devices"][0]["last_seen"].is_string());

    // exposed HTTP service: token required, browser login via query sets a cookie
    let url = format!("http://127.0.0.1:{http_port}/page?x=1");
    let r = http.get(&url).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = http.get(&url).bearer_auth(&device_token).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.text().await.unwrap().starts_with("hello from web"));
    let r = http.get(format!("{url}&openshell_token={device_token}")).send().await.unwrap();
    assert_eq!(r.status(), 302);
    assert_eq!(r.headers()["location"], "/page?x=1");
    assert!(r.headers()["set-cookie"].to_str().unwrap().contains("HttpOnly"));
    let r = http.get(&url).header("cookie", format!("openshell_token={device_token}")).send().await.unwrap();
    assert_eq!(r.status(), 200);

    // exposed TCP port: bytes are forwarded unchanged (allowlist only)
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", tcp_port)).await.unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        assert!(out.contains("hello from web"), "{out}");
    }

    let r = client.post("/v1/remote/devices/laptop/revoke", &json!({})).await.unwrap();
    assert_eq!(r["ok"], true);
    let r = http.get(format!("{base}/v1/remote/info")).bearer_auth(&device_token).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = http.get(&url).bearer_auth(&device_token).send().await.unwrap();
    assert_eq!(r.status(), 401);

    // failed logins are audited (loopback itself is never blocked)
    let audit = client.get("/v1/audit?limit=50").await.unwrap();
    let entries = audit["entries"].as_array().unwrap();
    assert!(entries.iter().any(|e| e["channel"] == "remote" && e["outcome"] == "denied"), "{audit}");
    assert!(
        entries.iter().any(|e| e["agent"].as_str().unwrap_or("").starts_with("device:laptop")),
        "{audit}"
    );
    shell.shutdown().await;
}

#[tokio::test]
async fn remote_gateway_over_tls_with_shared_token_off() {
    let port = openshell_runtime::process::free_port().unwrap();
    let extra = format!(
        r#","runtime":{{"remote":{{"enabled":true,"bind":"127.0.0.1","port":{port},"shared_token":false,"tls":{{"enabled":true}}}}}}"#
    );
    let (_dir, shell, client) = launch(&extra, Mode::Production).await;
    let remote = client.get("/v1/remote").await.unwrap();
    assert_eq!(remote["token"], Value::Null, "shared token is off");
    assert!(remote["urls"][0].as_str().unwrap().starts_with("https://"));
    let fp = remote["tls_fingerprint"].as_str().unwrap();
    assert_eq!(fp.len(), 95);
    assert_eq!(remote["self_signed"], true);

    let shared = shell.remote().unwrap().token.clone();
    let device = client.post("/v1/remote/devices", &json!({ "name": "phone" })).await.unwrap();
    let device = device["token"].as_str().unwrap().to_string();
    // self-signed: a real client pins the fingerprint; here certificate checks are off
    let https = reqwest::Client::builder().danger_accept_invalid_certs(true).build().unwrap();
    let base = format!("https://127.0.0.1:{port}");
    let r = https.get(format!("{base}/v1/health")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let r = https.get(format!("{base}/v1/remote/info")).bearer_auth(&shared).send().await.unwrap();
    assert_eq!(r.status(), 401, "shared token disabled");
    let r = https.get(format!("{base}/v1/remote/info")).bearer_auth(&device).send().await.unwrap();
    assert_eq!(r.status(), 200);
    // plain HTTP on the TLS port does not work
    assert!(reqwest::get(format!("http://127.0.0.1:{port}/v1/health")).await.is_err());
    shell.shutdown().await;
}

#[tokio::test]
async fn agents_off_closes_the_agent_layer() {
    let dir = tempfile::tempdir().unwrap();
    let json =
        format!(r#"{{"app":{{"name":"Quiet"}},"services":[{{"name":"api","command":"{}"}}]}}"#, sleep_cmd());
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config: parse_json(&json).unwrap(),
        state_dir_override: None,
    };
    let shell = Shell::launch(project, Mode::Development, Arc::new(HeadlessHost::default())).await.unwrap();
    assert!(!shell.agents_enabled());
    assert!(!shell.webview_init_script().contains("__OPENSHELL_ACTIONS__"), "no UI bridge");
    let client = ControlClient::connect(&dir.path().join(".openshell")).await.unwrap();
    for path in ["/mcp", "/mcp/app", "/v1/tools/runtime_status"] {
        let err = client.post(path, &json!({})).await.unwrap_err().to_string();
        assert!(err.contains("agent layer is off"), "{path}: {err}");
    }
    assert!(client.get("/v1/tools").await.is_err());
    // the app itself keeps working
    assert_eq!(client.get("/v1/status").await.unwrap()["app"], "Quiet");
    let agents = client.get("/v1/agents").await.unwrap();
    assert_eq!(agents["enabled"], false);
    assert_eq!(agents["granted"], json!([]));
    shell.shutdown().await;
}

#[tokio::test]
async fn agent_sessions_have_their_own_scopes() {
    let (_dir, shell, admin) = launch("", Mode::Development).await;
    let sessions = &shell.inner.sessions;
    let app_token = sessions.create("assistant", crate::sessions::SessionProfile::App, vec![]).unwrap();
    let dev_token = sessions
        .create("reader", crate::sessions::SessionProfile::Full, vec![openshell_config::Permission::Read])
        .unwrap();
    let assistant = admin.clone().with_token(&app_token).with_agent("claude");
    let reader = admin.clone().with_token(&dev_token);

    // app profile: /mcp/app only
    let list =
        assistant.post("/mcp/app", &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await.unwrap();
    assert!(list["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "app_status"));
    assert!(assistant.post("/mcp", &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await.is_err());
    assert!(assistant.get("/v1/status").await.is_err(), "admin API is closed to sessions");

    // full profile, but only `read`: reading works, writing files is denied
    let r = reader.post("/v1/tools/runtime_status", &json!({ "service": "api" })).await.unwrap();
    assert_eq!(r["ok"], true, "{r}");
    let r = reader.post("/v1/tools/fs_write", &json!({ "path": "x.txt", "content": "x" })).await.unwrap();
    assert_eq!(r["ok"], false);
    let tools = reader.get("/v1/tools").await.unwrap();
    let access = |name: &str| {
        tools["tools"].as_array().unwrap().iter().find(|t| t["name"] == name).unwrap()["access"].clone()
    };
    assert_eq!(access("fs_write"), "denied");
    assert_eq!(access("logs_read"), "allowed");

    // audited under the session's name
    let audit = admin.get("/v1/audit?limit=5").await.unwrap();
    assert!(audit["entries"].as_array().unwrap().iter().any(|e| e["agent"] == "session:reader"), "{audit}");

    // effective permissions for the runtime panel
    let agents = admin.get("/v1/agents").await.unwrap();
    assert_eq!(agents["enabled"], true);
    let reader_info =
        agents["sessions"].as_array().unwrap().iter().find(|s| s["name"] == "reader").unwrap().clone();
    assert_eq!(reader_info["effective"], json!(["read"]));

    // revoked tokens stop working at once
    assert!(sessions.revoke("reader").unwrap());
    assert!(reader.get("/v1/tools").await.is_err());
    shell.shutdown().await;
}
