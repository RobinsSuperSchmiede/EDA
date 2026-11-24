use super::*;

const FULL: &str = r#"{
  "$schema": "https://openshell.dev/schema/v1.json",
  "version": 1,
  "app": { "name": "My App", "version": "1.0.0" },
  "window": { "width": 1400, "height": 900 },
  "frontend": {
    "command": "npm run dev -- --port ${port}",
    "cwd": "frontend",
    "port": "auto",
    "url": "http://localhost:${port}",
    "dist": "frontend/dist",
    "proxy": { "/api": "${services.backend.url}" }
  },
  "services": [
    { "name": "db", "kind": "database", "external": true, "healthcheck": { "tcp": "127.0.0.1:5432" } },
    {
      "name": "backend",
      "command": ["python", "backend/main.py"],
      "port": 8000,
      "env": { "DATABASE": "${services.db.url}", "PORT": "${port}" },
      "healthcheck": "http://127.0.0.1:${port}/health",
      "restart": true,
      "depends_on": ["db"],
      "watch": ["backend"]
    }
  ],
  "capabilities": [
    { "name": "list_items", "description": "List items", "http": { "service": "backend", "method": "GET", "path": "/api/items" } },
    { "name": "run_tests", "description": "Run tests", "permission": "write", "task": "test" }
  ],
  "tasks": { "test": "pytest" },
  "agents": { "permissions": { "production": ["read"] }, "approval_required": ["code", "system"] }
}"#;

#[test]
fn parses_full_example() {
    let cfg = parse_json(FULL).expect("parse");
    let issues = validate(&cfg);
    assert!(issues.iter().all(|i| !i.is_error()), "{issues:?}");
    assert_eq!(cfg.services.len(), 2);
    let backend = cfg.service("backend").unwrap();
    assert_eq!(backend.restart_policy(), RestartPolicy::OnFailure);
    assert_eq!(backend.port, Some(PortSpec::Fixed(8000)));
    assert_eq!(cfg.capabilities[1].permission, Permission::Actions);
    assert_eq!(
        cfg.agents.permissions_for(Mode::Production).into_iter().collect::<Vec<_>>(),
        vec![Permission::Read]
    );
    assert!(cfg.agents.permissions_for(Mode::Development).contains(&Permission::Code));
}

#[test]
fn frontend_plan_depends_on_mode() {
    let cfg = parse_json(FULL).unwrap();
    let fe = cfg.frontend.unwrap();
    assert!(matches!(fe.plan(Mode::Development), Some(FrontendPlan::Process { .. })));
    assert_eq!(fe.plan(Mode::Production), Some(FrontendPlan::Static { dist: "frontend/dist".into() }));
    let external = FrontendConfig { url: Some("https://example.com".into()), ..Default::default() };
    assert_eq!(
        external.plan(Mode::Production),
        Some(FrontendPlan::External { url: "https://example.com".into() })
    );
}

#[test]
fn unknown_fields_report_path() {
    let err = parse_json(r#"{"app":{"name":"x"},"services":[{"name":"a","comand":"x"}]}"#).unwrap_err();
    assert!(err.contains("services[0]"), "{err}");
    assert!(err.contains("comand"), "{err}");
}

#[test]
fn detects_semantic_errors() {
    let cfg = parse_json(
        r#"{
        "app": {"name": "x"},
        "services": [
            {"name": "a", "command": "run a", "depends_on": ["b"]},
            {"name": "b", "command": "run b", "depends_on": ["a"]},
            {"name": "c", "command": "run c", "depends_on": ["missing"], "env": {"X": "${services.nope.url}"}},
            {"name": "Bad Name", "command": "x"},
            {"name": "frontend", "command": "x"},
            {"name": "d"},
            {"name": "e", "command": "x", "port": "random", "env": {"P": "${port}"}}
        ],
        "capabilities": [{"name": "do", "description": "d"}]
    }"#,
    )
    .unwrap();
    let issues = validate(&cfg);
    let text = issues.iter().map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
    for needle in [
        "dependency cycle",
        "unknown service `missing`",
        "unknown service `nope`",
        "not a valid name",
        "reserved",
        "needs a `command`",
        "expected a number or \"auto\"",
        "needs an `http`, `task` or `run` target",
    ] {
        assert!(text.contains(needle), "missing `{needle}` in:\n{text}");
    }
}

#[test]
fn version_is_checked() {
    let cfg = parse_json(r#"{"version": 99, "app": {"name": "x"}}"#).unwrap();
    assert!(validate(&cfg).iter().any(|i| i.path == "version"));
}

#[test]
fn startup_order_respects_dependencies() {
    let cfg = parse_json(
        r#"{"app":{"name":"x"},"services":[
        {"name":"web","command":"x","depends_on":["api"]},
        {"name":"api","command":"x","depends_on":["db"]},
        {"name":"db","command":"x"},
        {"name":"worker","command":"x"}]}"#,
    )
    .unwrap();
    assert_eq!(startup_order(&cfg.services), vec!["db", "api", "web", "worker"]);
}

#[test]
fn interpolation() {
    let lookup = |k: &str| match k {
        "port" => Some("1234".to_string()),
        "services.api.url" => Some("http://127.0.0.1:9".to_string()),
        _ => None,
    };
    assert_eq!(
        interpolate("http://localhost:${port}/x?a=$$b&u=${ services.api.url }", lookup).unwrap(),
        "http://localhost:1234/x?a=$b&u=http://127.0.0.1:9"
    );
    assert_eq!(interpolate("${nope}", lookup), Err(InterpolationError::Unknown("nope".into())));
    assert!(matches!(interpolate("${port", lookup), Err(InterpolationError::Unterminated(_))));
    assert_eq!(variables("a ${x} $${y} ${z.w}"), vec!["x", "z.w"]);
}

#[test]
fn toml_config() {
    let cfg = parse_toml(
        r#"
version = 1
[app]
name = "Toml App"
[frontend]
url = "http://localhost:3000"
[[services]]
name = "api"
command = "node server.js"
restart = "always"
"#,
    )
    .unwrap();
    assert!(validate(&cfg).iter().all(|i| !i.is_error()));
    assert_eq!(cfg.services[0].restart_policy(), RestartPolicy::Always);
}

#[test]
fn project_discovery_walks_up() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("openshell.json"), r#"{"app":{"name":"Disc"}}"#).unwrap();
    let nested = dir.path().join("a/b");
    std::fs::create_dir_all(&nested).unwrap();
    let project = Project::discover(&nested).unwrap();
    assert_eq!(project.config.app.name, "Disc");
    assert_eq!(project.root, dir.path());
}

#[test]
fn invalid_file_reports_issues() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("openshell.json"), r#"{"app":{"name":"x"},"services":[{"name":"a"}]}"#)
        .unwrap();
    let err = Project::load(dir.path()).unwrap_err();
    assert!(matches!(err, ConfigError::Invalid { .. }));
    assert!(err.to_string().contains("services[0].command"));
}

#[test]
fn schema_generation() {
    let schema = json_schema();
    assert!(schema.to_string().contains("ServiceConfig"));
}

#[test]
fn title_bar_defaults_to_native() {
    let cfg = parse_json(r#"{ "app": { "name": "A" } }"#).unwrap();
    assert_eq!(cfg.window.title_bar, TitleBar::Native);
    let cfg = parse_json(r#"{ "app": { "name": "A" }, "window": { "title_bar": "overlay" } }"#).unwrap();
    assert_eq!(cfg.window.title_bar, TitleBar::Overlay);
    assert!(parse_json(r#"{ "app": { "name": "A" }, "window": { "title_bar": "hidden" } }"#).is_err());
}

#[test]
fn composable_apps() {
    // database + function only: no frontend, no backend
    let cfg = parse_json(
        r#"{"app":{"name":"Tools"},
            "databases":[{"name":"main","migrations":"db/migrations","agent_access":"write"}],
            "capabilities":[{"name":"greet","description":"Greet","run":["python","tools/greet.py"]},
                            {"name":"slow","description":"Slow","run":{"command":"node slow.js","timeout_ms":1000}}],
            "window":{"enabled":false}}"#,
    )
    .unwrap();
    let issues = validate(&cfg);
    assert!(issues.iter().all(|i| !i.is_error()), "{issues:?}");
    assert_eq!(cfg.databases[0].file_name(), "main.db");
    assert_eq!(cfg.databases[0].env_var(), "OPENSHELL_DB_MAIN");
    assert_eq!(cfg.databases[0].agent_access, DatabaseAccess::Write);
    assert!(!cfg.window.enabled);
    assert_eq!(cfg.capabilities[1].run.as_ref().unwrap().normalized().timeout_ms, 1000);

    let bad = parse_json(
        r#"{"app":{"name":"x"},"databases":[{"name":"a"},{"name":"a"}],
            "capabilities":[{"name":"c","description":"d","run":"x","task":"t"}],"tasks":{"t":"x"}}"#,
    )
    .unwrap();
    let text = validate(&bad).iter().map(|i| i.to_string()).collect::<Vec<_>>().join("\n");
    assert!(text.contains("duplicate database `a`"), "{text}");
    assert!(text.contains("only one of"), "{text}");

    let empty = parse_json(r#"{"app":{"name":"x"}}"#).unwrap();
    assert!(validate(&empty).iter().any(|i| !i.is_error() && i.message.contains("openshell add")));
}

#[test]
fn cidr_ranges() {
    use std::net::IpAddr;
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    let lan = parse_cidr("192.168.1.0/24").unwrap();
    assert!(cidr_contains(lan, ip("192.168.1.77")));
    assert!(!cidr_contains(lan, ip("192.168.2.1")));
    assert!(cidr_contains(lan, ip("::ffff:192.168.1.5")), "IPv4-mapped IPv6 matches IPv4 ranges");
    let single = parse_cidr("10.0.0.7").unwrap();
    assert!(cidr_contains(single, ip("10.0.0.7")) && !cidr_contains(single, ip("10.0.0.8")));
    assert!(cidr_contains(parse_cidr("0.0.0.0/0").unwrap(), ip("8.8.8.8")));
    assert!(cidr_contains(parse_cidr("fd7a:115c:a1e0::/48").unwrap(), ip("fd7a:115c:a1e0:ab::1")));
    assert!(parse_cidr("10.0.0.0/33").is_none());
    assert!(parse_cidr("phone").is_none());
}

#[test]
fn remote_security_settings_are_validated() {
    let cfg = parse_json(
        r#"{ "version": 1, "app": { "name": "x" },
             "services": [ { "name": "api", "command": "node api.js", "port": 3000 },
                           { "name": "db", "command": "postgres", "port": 5432 } ],
             "runtime": { "remote": {
               "enabled": true,
               "allow": ["192.168.1.0/24", "nope"],
               "tls": { "enabled": true, "cert": "cert.pem" },
               "expose": [
                 { "service": "api", "port": 8080 },
                 { "service": "db", "port": 8080, "protocol": "tcp", "auth": "token" },
                 { "service": "ghost", "port": 7420 },
                 { "service": "frontend", "port": 8081, "auth": "none", "allow": [] }
               ] } } }"#,
    )
    .unwrap();
    let issues = validate(&cfg);
    let has = |path: &str, error: bool| issues.iter().any(|i| i.path == path && i.error == error);
    assert!(has("runtime.remote.allow[1]", true), "{issues:?}");
    assert!(has("runtime.remote.tls", true));
    assert!(has("runtime.remote.expose[1].port", true), "duplicate port");
    assert!(has("runtime.remote.expose[1].auth", true), "tcp + token");
    assert!(has("runtime.remote.expose[2].service", true));
    assert!(has("runtime.remote.expose[2].port", true), "gateway port");
    assert!(has("runtime.remote.expose[3].allow", false), "no login, no allowlist");
    assert!(!issues.iter().any(|i| i.path.starts_with("runtime.remote.expose[0]")), "{issues:?}");
    assert_eq!(cfg.runtime.remote.expose[0].effective_auth(), ExposeAuth::Token);
    assert_eq!(cfg.runtime.remote.expose[1].effective_auth(), ExposeAuth::None);
}

#[test]
fn default_remote_config_is_not_serialized() {
    let cfg = parse_json(r#"{ "version": 1, "app": { "name": "x" } }"#).unwrap();
    let json = serde_json::to_string(&cfg).unwrap();
    assert!(!json.contains("remote"), "{json}");
}
