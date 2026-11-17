//! Runtime integration tests using portable shell commands.

use super::*;
use openshell_config::parse_json;
use std::time::Duration;

fn project(json: &str) -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let config = parse_json(json).unwrap();
    let issues = openshell_config::validate(&config);
    assert!(issues.iter().all(|i| !i.is_error()), "{issues:?}");
    let project = Project {
        root: dir.path().to_path_buf(),
        config_path: dir.path().join("openshell.json"),
        config,
        state_dir_override: None,
    };
    (dir, project)
}

fn sleep_cmd() -> &'static str {
    if cfg!(windows) {
        "ping -n 60 127.0.0.1 > NUL"
    } else {
        "sleep 60"
    }
}

async fn wait_state(rt: &Runtime, name: &str, state: ServiceState, secs: u64) -> ServiceStatus {
    let deadline = Instant::now() + Duration::from_secs(secs);
    loop {
        let st = rt.service_status(name).unwrap();
        if st.state == state {
            return st;
        }
        if Instant::now() > deadline {
            panic!("{name} did not reach {state}, is {:?}", st);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn runs_process_and_captures_output() {
    let (_d, p) = project(
        r#"{"app":{"name":"t"},"services":[{"name":"hello","command":"echo hello-openshell","restart":false}]}"#,
    );
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start_all();
    let st = wait_state(&rt, "hello", ServiceState::Exited, 10).await;
    assert_eq!(st.exit_code, Some(0));
    let lines = rt.logs().query(&LogQuery { service: Some("hello".into()), ..Default::default() });
    assert!(
        lines.iter().any(|l| l.text.contains("hello-openshell") && l.stream == LogStream::Stdout),
        "{lines:?}"
    );
}

#[tokio::test]
async fn crash_loop_is_limited() {
    let (_d, p) = project(
        r#"{"app":{"name":"t"},"services":[{"name":"crash","command":"echo boom && exit 3","restart":"on-failure","max_restarts":2}]}"#,
    );
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start("crash").unwrap();
    let st = wait_state(&rt, "crash", ServiceState::Failed, 20).await;
    assert_eq!(st.restarts, 2);
    assert_eq!(st.exit_code, Some(3));
    assert!(st.last_error.unwrap().contains("giving up"));
}

#[tokio::test]
async fn crash_without_restart_is_reported() {
    let (_d, p) =
        project(r#"{"app":{"name":"t"},"services":[{"name":"crash","command":"exit 2","restart":false}]}"#);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start("crash").unwrap();
    let st = wait_state(&rt, "crash", ServiceState::Crashed, 10).await;
    assert_eq!(st.exit_code, Some(2));
}

#[tokio::test]
async fn stop_and_restart_long_running_process() {
    let json =
        format!(r#"{{"app":{{"name":"t"}},"services":[{{"name":"svc","command":"{}"}}]}}"#, sleep_cmd());
    let (_d, p) = project(&json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start_all();
    let st = rt.wait_ready("svc", Duration::from_secs(10)).await.unwrap();
    assert!(st.pid.is_some());

    rt.restart("svc").unwrap();
    // wait for the restart to be picked up
    let deadline = Instant::now() + Duration::from_secs(10);
    while rt.service_status("svc").unwrap().restarts == 0 {
        assert!(Instant::now() < deadline, "restart not observed");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    rt.wait_ready("svc", Duration::from_secs(10)).await.unwrap();

    rt.stop("svc").unwrap();
    rt.wait_stopped("svc", Duration::from_secs(15)).await.unwrap();
    let st = rt.service_status("svc").unwrap();
    assert_eq!(st.state, ServiceState::Stopped);
    assert_eq!(st.pid, None);
}

#[tokio::test]
async fn dependencies_wait_for_health() {
    let port = process::free_port().unwrap();
    let json = format!(
        r#"{{"app":{{"name":"t"}},"services":[
            {{"name":"api","command":"{}","depends_on":["db"]}},
            {{"name":"db","external":true,"healthcheck":{{"tcp":"127.0.0.1:{port}","interval_ms":200}}}}
        ]}}"#,
        sleep_cmd()
    );
    let (_d, p) = project(&json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    assert_eq!(rt.service_names(), vec!["db", "api"]);
    rt.start_all();
    wait_state(&rt, "api", ServiceState::Pending, 5).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(rt.service_status("api").unwrap().state, ServiceState::Pending);

    let _listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await.unwrap();
    rt.wait_ready("db", Duration::from_secs(10)).await.unwrap();
    rt.wait_ready("api", Duration::from_secs(10)).await.unwrap();
    rt.shutdown().await;
    assert!(rt.status().iter().all(|s| s.state.is_terminal()));
}

#[tokio::test]
async fn auto_ports_and_variables() {
    let json = format!(
        r#"{{"app":{{"name":"t"}},"services":[
            {{"name":"a","command":"{}","port":"auto","healthcheck":{{"tcp":"127.0.0.1:1","ready_timeout_ms":100}}}},
            {{"name":"b","command":"echo ${{services.a.url}} $PORT","port":"auto","restart":false,"healthcheck":{{"tcp":"127.0.0.1:1"}}}}
        ]}}"#,
        sleep_cmd()
    );
    let (_d, p) = project(&json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    let a = rt.service_status("a").unwrap();
    let port = a.port.unwrap();
    assert_eq!(a.url.as_deref(), Some(format!("http://127.0.0.1:{port}").as_str()));
    let b = rt.service_status("b").unwrap();
    assert!(b.command.unwrap().contains(&format!("http://127.0.0.1:{port}")));
}

#[tokio::test]
async fn setup_runs_once() {
    let (dir, p) = project(
        r#"{"app":{"name":"t"},"services":[{"name":"s","command":"echo started","restart":false,
            "setup":{"command":"echo prepared > marker.txt","creates":"marker.txt"}}]}"#,
    );
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start("s").unwrap();
    wait_state(&rt, "s", ServiceState::Exited, 10).await;
    assert!(dir.path().join("marker.txt").exists());
    let setup_runs = |rt: &Runtime| {
        rt.logs().query(&LogQuery { contains: Some("running setup".into()), ..Default::default() }).len()
    };
    assert_eq!(setup_runs(&rt), 1);
    rt.start("s").unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    wait_state(&rt, "s", ServiceState::Exited, 10).await;
    assert_eq!(setup_runs(&rt), 1);
}

#[tokio::test]
async fn serves_static_frontend_with_injection() {
    let (dir, p) = project(r#"{"app":{"name":"t"},"frontend":{"dist":"web"}}"#);
    std::fs::create_dir_all(dir.path().join("web/assets")).unwrap();
    std::fs::write(
        dir.path().join("web/index.html"),
        "<html><head><title>T</title></head><body>app</body></html>",
    )
    .unwrap();
    std::fs::write(dir.path().join("web/assets/app.js"), "console.log(1)").unwrap();
    let rt = Runtime::new(&p, Mode::Production).unwrap();
    rt.set_html_injection(Some("<script>/*bridge*/</script>".into()));
    rt.start_all();
    rt.wait_ready("frontend", Duration::from_secs(5)).await.unwrap();
    let url = rt.frontend_url().unwrap();
    let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
    assert!(html.contains("<head><script>/*bridge*/</script><title>"), "{html}");
    let spa = reqwest::get(format!("{url}some/client/route")).await.unwrap();
    assert_eq!(spa.status(), 200);
    let js = reqwest::get(format!("{url}assets/app.js")).await.unwrap();
    assert!(js.headers()["content-type"].to_str().unwrap().contains("javascript"));
    let missing = reqwest::get(format!("{url}missing.png")).await.unwrap();
    assert_eq!(missing.status(), 404);
    rt.shutdown().await;
}

#[tokio::test]
async fn events_are_emitted() {
    let (_d, p) =
        project(r#"{"app":{"name":"t"},"services":[{"name":"e","command":"echo hi","restart":false}]}"#);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    let mut rx = rt.subscribe();
    rt.start_all();
    let mut seen = vec![];
    let deadline = Instant::now() + Duration::from_secs(10);
    while !seen.contains(&ServiceState::Exited) && Instant::now() < deadline {
        if let Ok(Ok(RuntimeEvent::ServiceState { state, .. })) =
            tokio::time::timeout(Duration::from_secs(1), rx.recv()).await
        {
            seen.push(state);
        }
    }
    assert!(seen.contains(&ServiceState::Starting), "{seen:?}");
    assert!(seen.contains(&ServiceState::Exited), "{seen:?}");
}

#[tokio::test]
async fn start_while_stopping_is_not_lost() {
    let json =
        format!(r#"{{"app":{{"name":"t"}},"services":[{{"name":"svc","command":"{}"}}]}}"#, sleep_cmd());
    let (_d, p) = project(&json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start_all();
    rt.wait_ready("svc", Duration::from_secs(10)).await.unwrap();
    rt.stop("svc").unwrap();
    // start again immediately, while the process is still being terminated
    tokio::time::sleep(Duration::from_millis(20)).await;
    rt.start("svc").unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    rt.wait_ready("svc", Duration::from_secs(15)).await.unwrap();
    rt.shutdown().await;
}

#[tokio::test]
async fn restart_starts_stopped_dependencies() {
    let json = format!(
        r#"{{"app":{{"name":"t"}},"services":[
            {{"name":"db","command":"{0}"}},
            {{"name":"api","command":"{0}","depends_on":["db"]}}
        ]}}"#,
        sleep_cmd()
    );
    let (_d, p) = project(&json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start_all();
    rt.wait_ready("api", Duration::from_secs(10)).await.unwrap();
    rt.stop("db").unwrap();
    rt.wait_stopped("db", Duration::from_secs(15)).await.unwrap();
    rt.restart("api").unwrap();
    rt.wait_ready("db", Duration::from_secs(10)).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let api = rt.wait_ready("api", Duration::from_secs(10)).await.unwrap();
    assert!(api.last_error.is_none());
    rt.shutdown().await;
}

#[tokio::test]
async fn build_step_runs_before_start_in_development_only() {
    let json = r#"{"app":{"name":"t"},"services":[{"name":"svc","command":"echo running","restart":false,
        "build":"echo compiled > built.txt"}]}"#;
    let (dir, p) = project(json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start("svc").unwrap();
    wait_state(&rt, "svc", ServiceState::Exited, 15).await;
    assert!(dir.path().join("built.txt").exists());
    let logs = rt.logs().query(&LogQuery { service: Some("svc".into()), ..Default::default() });
    assert!(logs.iter().any(|l| l.text.starts_with("build finished")), "{logs:?}");

    let (dir2, p2) = project(json);
    let rt2 = Runtime::new(&p2, Mode::Production).unwrap();
    rt2.start("svc").unwrap();
    wait_state(&rt2, "svc", ServiceState::Exited, 15).await;
    assert!(!dir2.path().join("built.txt").exists());
}

#[tokio::test]
async fn failed_build_is_reported() {
    let (_d, p) = project(
        r#"{"app":{"name":"t"},"services":[{"name":"svc","command":"echo never","build":"echo error: missing semicolon && exit 1"}]}"#,
    );
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start("svc").unwrap();
    let st = wait_state(&rt, "svc", ServiceState::Failed, 15).await;
    assert!(st.last_error.unwrap().starts_with("build failed"));
    let errs =
        rt.logs().query(&LogQuery { contains: Some("missing semicolon".into()), ..Default::default() });
    assert_eq!(errs.iter().filter(|l| l.stream == LogStream::Stdout).count(), 1);
}

#[tokio::test]
async fn metrics_are_sampled() {
    let json =
        format!(r#"{{"app":{{"name":"t"}},"services":[{{"name":"svc","command":"{}"}}]}}"#, sleep_cmd());
    let (_d, p) = project(&json);
    let rt = Runtime::new(&p, Mode::Development).unwrap();
    rt.start_all();
    rt.wait_ready("svc", Duration::from_secs(10)).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let m = loop {
        if let Some(m) = rt.service_status("svc").unwrap().metrics {
            break m;
        }
        assert!(Instant::now() < deadline, "no metrics");
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert!(m.memory_bytes > 0);
    assert!(m.processes >= 1);
    rt.shutdown().await;
    assert!(rt.service_status("svc").unwrap().metrics.is_none());
}
