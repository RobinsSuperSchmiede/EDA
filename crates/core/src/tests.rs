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

async fn launch(extra: &str, mode: Mode) -> (tempfile::TempDir, Shell, ControlClient) {
    let dir = tempfile::tempdir().unwrap();
    let json = format!(
        r#"{{"app":{{"name":"Test App"}},"services":[{{"name":"api","command":"{}"}}]{extra}}}"#,
        sleep_cmd()
    );
    let config = parse_json(&json).unwrap();
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config,
        state_dir_override: None,
    };
    let shell = Shell::launch(project, mode, Arc::new(HeadlessHost::default()))
        .await
        .unwrap();
    shell.start();
    shell
        .runtime()
        .wait_ready("api", Duration::from_secs(10))
        .await
        .unwrap();
    let client = ControlClient::connect(&dir.path().join(".openshell"))
        .await
        .unwrap()
        .with_agent("test-agent");
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
    let list = client
        .post("/mcp", &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}))
        .await
        .unwrap();
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
    client
        .post(&format!("/v1/approvals/{id}/approve"), &json!({}))
        .await
        .unwrap();
    let r = call.await.unwrap();
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(std::fs::read_to_string(dir.path().join("a.txt")).unwrap(), "approved");

    // deny the next one
    let c2 = client.clone();
    let call = tokio::spawn(async move {
        tool(&c2, "fs_write", json!({ "path": "b.txt", "content": "x" })).await
    });
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

    let r = http
        .get(format!("{base}/v1/status"))
        .bearer_auth(&shell.inner.app_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    let r = http
        .get(format!("{base}/v1/sdk/info"))
        .bearer_auth(&shell.inner.app_token)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    let r = http
        .get(format!("{base}/v1/status"))
        .bearer_auth(&client.info.token)
        .header("Origin", "https://evil.example")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);

    let r = http
        .get(format!("{base}/v1/health"))
        .header("Host", "evil.example")
        .send()
        .await
        .unwrap();
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
        .route("/openapi.json", get(move || { let d = doc.clone(); async move { axum::Json(d) } }))
        .route("/items/{id}", get(|axum::extract::Path(id): axum::extract::Path<u32>| async move {
            axum::Json(json!({ "id": id, "name": format!("item {id}") }))
        }))
        .route("/items", axum::routing::post(|axum::Json(body): axum::Json<Value>| async move {
            axum::Json(json!({ "created": body["name"] }))
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    port
}

#[tokio::test]
async fn openapi_import_and_app_profile() {
    let port = openapi_backend().await;
    let extra = format!(
        r#","capabilities":[{{"name":"ping","description":"Ping","task":"ping"}}],"tasks":{{"ping":"echo pong"}}}}"#
    );
    // second service: an external backend with an OpenAPI document
    let dir = tempfile::tempdir().unwrap();
    let json = format!(
        r#"{{"app":{{"name":"Api App","description":"demo"}},"services":[
            {{"name":"api","command":"{}"}},
            {{"name":"backend","external":true,"url":"http://127.0.0.1:{port}","healthcheck":{{"http":"http://127.0.0.1:{port}/openapi.json","interval_ms":200}},"openapi":"/openapi.json"}}
        ]{extra}"#,
        sleep_cmd()
    );
    let config = parse_json(&json).unwrap();
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
        let list = client.post("/mcp/app", &json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await.unwrap();
        let names: Vec<String> = list["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
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
