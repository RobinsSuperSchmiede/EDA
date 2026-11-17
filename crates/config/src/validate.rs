//! Semantic validation of a parsed configuration.

use crate::interpolate::variables;
use crate::model::*;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// A validation finding. Errors prevent loading, warnings are informational.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Issue {
    pub path: String,
    pub message: String,
    pub error: bool,
}

impl Issue {
    fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { path: path.into(), message: message.into(), error: true }
    }
    fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { path: path.into(), message: message.into(), error: false }
    }
    pub fn is_error(&self) -> bool {
        self.error
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = if self.error { "error" } else { "warning" };
        write!(f, "{level} at `{}`: {}", self.path, self.message)
    }
}

/// Reserved service name used for the frontend.
pub const FRONTEND_SERVICE: &str = "frontend";

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && name.chars().next().map(|c| c.is_ascii_alphanumeric()).unwrap_or(false)
}

/// Validate a configuration and return all findings.
pub fn validate(config: &Config) -> Vec<Issue> {
    let mut issues = Vec::new();

    if config.version == 0 || config.version > crate::CONFIG_VERSION {
        issues.push(Issue::error(
            "version",
            format!(
                "unsupported config version {} (this OpenShell supports version {})",
                config.version,
                crate::CONFIG_VERSION
            ),
        ));
    }
    if config.app.name.trim().is_empty() {
        issues.push(Issue::error("app.name", "must not be empty"));
    }
    if config.window.width < 100.0 || config.window.height < 100.0 {
        issues.push(Issue::error("window", "width and height must be at least 100"));
    }

    // --- services -----------------------------------------------------------
    let mut names = BTreeSet::new();
    for (i, s) in config.services.iter().enumerate() {
        let p = format!("services[{i}]");
        if !valid_name(&s.name) {
            issues.push(Issue::error(
                format!("{p}.name"),
                format!("`{}` is not a valid name (use lowercase letters, digits, `-`, `_`)", s.name),
            ));
        }
        if s.name == FRONTEND_SERVICE {
            issues.push(Issue::error(
                format!("{p}.name"),
                "`frontend` is reserved; configure the frontend in the `frontend` section",
            ));
        }
        if !names.insert(s.name.clone()) {
            issues.push(Issue::error(format!("{p}.name"), format!("duplicate service `{}`", s.name)));
        }
        match (&s.command, s.external) {
            (None, false) => issues.push(Issue::error(
                format!("{p}.command"),
                "a service needs a `command` (or `external: true` with a `url`/`healthcheck`)",
            )),
            (Some(c), false) if c.is_empty() => {
                issues.push(Issue::error(format!("{p}.command"), "must not be empty"))
            }
            (Some(_), true) => issues
                .push(Issue::warning(format!("{p}.command"), "ignored because the service is `external`")),
            (None, true) if s.url.is_none() && s.healthcheck.is_none() => issues
                .push(Issue::error(p.to_string(), "an external service needs a `url` or a `healthcheck`")),
            _ => {}
        }
        if let Some(port) = &s.port {
            check_port(port, &format!("{p}.port"), &mut issues);
        }
        if let Some(hc) = &s.healthcheck {
            check_healthcheck(hc, &format!("{p}.healthcheck"), &mut issues);
        }
        for (j, d) in s.depends_on.iter().enumerate() {
            if d == &s.name {
                issues
                    .push(Issue::error(format!("{p}.depends_on[{j}]"), "a service cannot depend on itself"));
            }
        }
        if let Some(b) = &s.build {
            if b.normalized().command.is_empty() {
                issues.push(Issue::error(format!("{p}.build"), "must not be empty"));
            }
            if s.external {
                issues.push(Issue::warning(format!("{p}.build"), "ignored for external services"));
            }
        }
        if let Some(o) = &s.openapi {
            if o.normalized().location.trim().is_empty() {
                issues.push(Issue::error(format!("{p}.openapi"), "must not be empty"));
            }
        }
        if !s.watch.is_empty() && s.external {
            issues.push(Issue::warning(format!("{p}.watch"), "ignored for external services"));
        }
    }

    // dependency references (after collecting names)
    let deps_of = |deps: &[String], p: &str, issues: &mut Vec<Issue>| {
        for (j, d) in deps.iter().enumerate() {
            if !names.contains(d) {
                issues.push(Issue::error(format!("{p}.depends_on[{j}]"), format!("unknown service `{d}`")));
            }
        }
    };
    for (i, s) in config.services.iter().enumerate() {
        deps_of(&s.depends_on, &format!("services[{i}]"), &mut issues);
    }
    if let Some(cycle) = find_cycle(&config.services) {
        issues.push(Issue::error("services", format!("dependency cycle: {}", cycle.join(" -> "))));
    }

    // --- frontend -----------------------------------------------------------
    if let Some(fe) = &config.frontend {
        deps_of(&fe.depends_on, "frontend", &mut issues);
        if let Some(port) = &fe.port {
            check_port(port, "frontend.port", &mut issues);
        }
        if let Some(hc) = &fe.healthcheck {
            check_healthcheck(hc, "frontend.healthcheck", &mut issues);
        }
        if fe.command.is_some() && fe.url.is_none() && fe.port.is_none() {
            issues.push(Issue::error(
                "frontend",
                "`command` needs a `url` (or a `port`) so OpenShell knows what to load",
            ));
        }
        if fe.plan(Mode::Development).is_none() && fe.plan(Mode::Production).is_none() {
            issues.push(Issue::error(
                "frontend",
                "set `url`, `command` + `url`, or `dist` so OpenShell knows what to load",
            ));
        }
        for prefix in fe.proxy.keys() {
            if !prefix.starts_with('/') {
                issues.push(Issue::error(
                    format!("frontend.proxy.{prefix}"),
                    "proxy prefixes must start with `/`",
                ));
            }
        }
    }

    // --- variables ----------------------------------------------------------
    let mut check_vars = |text: &str, p: String, has_port: bool| {
        for var in variables(text) {
            if let Some(msg) = check_variable(&var, &names, has_port) {
                issues.push(Issue::error(p.clone(), msg));
            }
        }
    };
    for (i, s) in config.services.iter().enumerate() {
        let p = format!("services[{i}]");
        let has_port = s.port.is_some();
        if let Some(c) = &s.command {
            check_vars(&c.display(), format!("{p}.command"), has_port);
        }
        for a in &s.args {
            check_vars(a, format!("{p}.args"), has_port);
        }
        for (k, v) in &s.env {
            check_vars(v, format!("{p}.env.{k}"), has_port);
        }
        if let Some(u) = &s.url {
            check_vars(u, format!("{p}.url"), has_port);
        }
    }
    if let Some(fe) = &config.frontend {
        let has_port = fe.port.is_some();
        if let Some(c) = &fe.command {
            check_vars(&c.display(), "frontend.command".into(), has_port);
        }
        if let Some(u) = &fe.url {
            check_vars(u, "frontend.url".into(), has_port);
        }
        for (k, v) in &fe.env {
            check_vars(v, format!("frontend.env.{k}"), has_port);
        }
        for (k, v) in &fe.proxy {
            check_vars(v, format!("frontend.proxy.{k}"), has_port);
        }
    }

    // --- capabilities -------------------------------------------------------
    let mut cap_names = BTreeSet::new();
    for (i, c) in config.capabilities.iter().enumerate() {
        let p = format!("capabilities[{i}]");
        if !valid_name(&c.name) || c.name.contains('-') {
            issues.push(Issue::error(
                format!("{p}.name"),
                format!("`{}` is not a valid capability name (use lowercase letters, digits, `_`)", c.name),
            ));
        }
        if !cap_names.insert(c.name.clone()) {
            issues.push(Issue::error(format!("{p}.name"), format!("duplicate capability `{}`", c.name)));
        }
        match (&c.http, &c.task) {
            (None, None) => {
                issues.push(Issue::error(p.clone(), "a capability needs an `http` or `task` target"))
            }
            (Some(_), Some(_)) => {
                issues.push(Issue::error(p.clone(), "use either `http` or `task`, not both"))
            }
            _ => {}
        }
        if let Some(h) = &c.http {
            match (&h.service, &h.url) {
                (Some(s), _) if !names.contains(s) => {
                    issues.push(Issue::error(format!("{p}.http.service"), format!("unknown service `{s}`")))
                }
                (None, None) => {
                    issues.push(Issue::error(format!("{p}.http"), "set `service` (+ `path`) or `url`"))
                }
                _ => {}
            }
            let m = h.method.to_ascii_uppercase();
            if !["GET", "POST", "PUT", "PATCH", "DELETE"].contains(&m.as_str()) {
                issues.push(Issue::error(
                    format!("{p}.http.method"),
                    format!("unsupported method `{}`", h.method),
                ));
            }
        }
        if let Some(t) = &c.task {
            if !config.tasks.contains_key(t) {
                issues.push(Issue::error(format!("{p}.task"), format!("unknown task `{t}`")));
            }
        }
        if let Some(schema) = &c.input_schema {
            if !schema.is_object() {
                issues.push(Issue::error(format!("{p}.input_schema"), "must be a JSON Schema object"));
            }
        }
    }

    // --- tasks --------------------------------------------------------------
    for (name, t) in &config.tasks {
        if !valid_name(name) {
            issues.push(Issue::error(format!("tasks.{name}"), "invalid task name"));
        }
        if t.normalized().command.is_empty() {
            issues.push(Issue::error(format!("tasks.{name}.command"), "must not be empty"));
        }
    }

    // --- package ------------------------------------------------------------
    for (i, t) in config.package.targets.iter().enumerate() {
        if !["nsis", "msi", "app", "dmg", "deb", "rpm", "appimage"].contains(&t.as_str()) {
            issues.push(Issue::error(
                format!("package.targets[{i}]"),
                format!("unknown installer format `{t}` (nsis, msi, app, dmg, deb, rpm, appimage)"),
            ));
        }
    }
    if let Some(v) = &config.app.version {
        let core = v.split(['-', '+']).next().unwrap_or("");
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 || parts.iter().any(|p| p.parse::<u64>().is_err()) {
            issues.push(Issue::warning(
                "app.version",
                format!("`{v}` is not a semantic version (MAJOR.MINOR.PATCH); installers require one"),
            ));
        }
    }

    // --- agents -------------------------------------------------------------
    let mut provider_names = BTreeSet::new();
    for (i, p) in config.agents.providers.iter().enumerate() {
        if !provider_names.insert(p.name.clone()) {
            issues.push(Issue::error(
                format!("agents.providers[{i}].name"),
                format!("duplicate provider `{}`", p.name),
            ));
        }
    }
    let prod = config.agents.permissions_for(Mode::Production);
    for risky in [Permission::Code, Permission::System] {
        if prod.contains(&risky) && !config.agents.approval_required().contains(&risky) {
            issues.push(Issue::warning(
                "agents.permissions",
                format!("`{risky}` is granted in production without requiring approval"),
            ));
        }
    }

    issues
}

fn check_port(port: &PortSpec, path: &str, issues: &mut Vec<Issue>) {
    match port {
        PortSpec::Fixed(0) => issues.push(Issue::error(path, "port 0 is not allowed; use \"auto\"")),
        PortSpec::Named(s) if s != "auto" => {
            issues.push(Issue::error(path, format!("expected a number or \"auto\", got \"{s}\"")))
        }
        _ => {}
    }
}

fn check_healthcheck(hc: &HealthCheckSpec, path: &str, issues: &mut Vec<Issue>) {
    let hc = hc.normalized();
    match (&hc.http, &hc.tcp) {
        (None, None) => issues.push(Issue::error(path, "set `http` or `tcp`")),
        (Some(u), _) if !(u.starts_with("http://") || u.starts_with("https://")) => {
            issues.push(Issue::error(path, format!("`{u}` is not an http(s) URL")))
        }
        (_, Some(t)) if !t.contains(':') => {
            issues.push(Issue::error(path, format!("`{t}` must be `host:port`")))
        }
        _ => {}
    }
    if hc.interval_ms < 100 {
        issues.push(Issue::error(path, "interval_ms must be at least 100"));
    }
}

fn check_variable(var: &str, services: &BTreeSet<String>, has_port: bool) -> Option<String> {
    let parts: Vec<&str> = var.split('.').collect();
    match parts.as_slice() {
        ["port"] if has_port => None,
        ["port"] => Some("`${port}` is used but no `port` is configured".into()),
        ["mode"] | ["project", "root"] | ["frontend", "url"] => None,
        ["env", name] if !name.is_empty() => None,
        ["services", name, "port" | "url"] if services.contains(*name) => None,
        ["services", name, _] if !services.contains(*name) => {
            Some(format!("`${{{var}}}` references unknown service `{name}`"))
        }
        _ => Some(format!("unknown variable `${{{var}}}`")),
    }
}

/// Returns a dependency cycle if one exists.
pub(crate) fn find_cycle(services: &[ServiceConfig]) -> Option<Vec<String>> {
    let graph: BTreeMap<&str, Vec<&str>> = services
        .iter()
        .map(|s| (s.name.as_str(), s.depends_on.iter().map(String::as_str).collect()))
        .collect();
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Visiting,
        Done,
    }
    fn visit<'a>(
        n: &'a str,
        graph: &BTreeMap<&'a str, Vec<&'a str>>,
        marks: &mut BTreeMap<&'a str, Mark>,
        stack: &mut Vec<&'a str>,
    ) -> Option<Vec<String>> {
        match marks.get(n) {
            Some(Mark::Done) => return None,
            Some(Mark::Visiting) => {
                let start = stack.iter().position(|s| *s == n).unwrap_or(0);
                let mut cycle: Vec<String> = stack[start..].iter().map(|s| s.to_string()).collect();
                cycle.push(n.to_string());
                return Some(cycle);
            }
            None => {}
        }
        marks.insert(n, Mark::Visiting);
        stack.push(n);
        for d in graph.get(n).into_iter().flatten() {
            if graph.contains_key(d) {
                if let Some(c) = visit(d, graph, marks, stack) {
                    return Some(c);
                }
            }
        }
        stack.pop();
        marks.insert(n, Mark::Done);
        None
    }
    let mut marks = BTreeMap::new();
    for n in graph.keys() {
        if let Some(c) = visit(n, &graph, &mut marks, &mut Vec::new()) {
            return Some(c);
        }
    }
    None
}

/// Order services so that dependencies come first (stable w.r.t. declaration order).
/// Assumes the configuration has been validated (no cycles).
pub fn startup_order(services: &[ServiceConfig]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut placed = BTreeSet::new();
    while order.len() < services.len() {
        // pick the first (in declaration order) service whose dependencies are placed
        let next = services.iter().find(|s| {
            !placed.contains(&s.name)
                && s.depends_on.iter().all(|d| placed.contains(d) || !services.iter().any(|o| &o.name == d))
        });
        if let Some(s) = next {
            placed.insert(s.name.clone());
            order.push(s.name.clone());
        } else {
            // cycle: append the rest in declaration order
            for s in services {
                if placed.insert(s.name.clone()) {
                    order.push(s.name.clone());
                }
            }
        }
    }
    order
}
