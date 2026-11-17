//! Turns the declarative configuration into concrete, fully resolved service
//! definitions: ports are allocated, variables interpolated, probes derived.

use crate::health::{Probe, ProbeConfig};
use crate::process::free_port;
use anyhow::{anyhow, bail, Result};
use openshell_config::{
    interpolate, startup_order, BuildWhen, CommandSpec, FrontendPlan, HealthCheckSpec, Mode, PortSpec,
    Project, RestartPolicy, FRONTEND_SERVICE,
};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum DefKind {
    /// A local process managed by the runtime.
    Process { command: CommandSpec, args: Vec<String> },
    /// Running elsewhere; only monitored.
    External,
    /// Static files served by the built-in web server.
    Static { dist: PathBuf, proxy: Vec<(String, String)> },
}

#[derive(Debug, Clone)]
pub struct SetupDef {
    pub command: CommandSpec,
    pub creates: Option<PathBuf>,
    pub cwd: PathBuf,
}

/// Build step of a compiled service.
#[derive(Debug, Clone)]
pub struct BuildDef {
    pub command: CommandSpec,
    pub cwd: PathBuf,
    /// Run automatically before each start (depends on mode and `on_start`).
    pub on_start: bool,
}

#[derive(Debug, Clone)]
pub struct ServiceDef {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub kind: DefKind,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub probe: ProbeConfig,
    pub restart: RestartPolicy,
    pub max_restarts: u32,
    pub depends_on: Vec<String>,
    pub autostart: bool,
    pub setup: Option<SetupDef>,
    pub build: Option<BuildDef>,
    pub watch: Vec<PathBuf>,
    pub stop_timeout: Duration,
    pub is_frontend: bool,
}

impl ServiceDef {
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            DefKind::Process { .. } => "process",
            DefKind::External => "external",
            DefKind::Static { .. } => "static",
        }
    }

    pub fn command_display(&self) -> Option<String> {
        match &self.kind {
            DefKind::Process { command, args } => {
                let mut s = command.display();
                for a in args {
                    s.push(' ');
                    s.push_str(a);
                }
                Some(s)
            }
            DefKind::Static { dist, .. } => Some(format!("static {}", dist.display())),
            DefKind::External => None,
        }
    }
}

/// Resolved plan for a project in a mode, in startup order (frontend last).
pub fn build_plan(project: &Project, mode: Mode) -> Result<Vec<ServiceDef>> {
    let cfg = &project.config;
    let root = project.root.clone();
    let shutdown = Duration::from_millis(cfg.runtime.shutdown_timeout_ms);

    let enabled: Vec<_> = cfg.services.iter().filter(|s| s.enabled_in(mode)).cloned().collect();
    let enabled_names: Vec<String> = enabled.iter().map(|s| s.name.clone()).collect();

    // 1. ports
    let mut ports: HashMap<String, u16> = HashMap::new();
    let alloc = |spec: &Option<PortSpec>| -> Result<Option<u16>> {
        match spec {
            None => Ok(None),
            Some(p) if p.is_auto() => Ok(Some(free_port()?)),
            Some(p) => Ok(p.fixed()),
        }
    };
    for s in &enabled {
        if let Some(p) = alloc(&s.port)? {
            ports.insert(s.name.clone(), p);
        }
    }
    let fe_plan = cfg.frontend.as_ref().and_then(|f| f.plan(mode));
    if let (Some(fe), Some(plan)) = (&cfg.frontend, &fe_plan) {
        let port = match plan {
            FrontendPlan::Static { .. } => Some(alloc(&fe.port)?.unwrap_or(free_port()?)),
            FrontendPlan::Process { .. } => alloc(&fe.port)?,
            FrontendPlan::External { .. } => None,
        };
        if let Some(p) = port {
            ports.insert(FRONTEND_SERVICE.into(), p);
        }
    }

    // 2. URLs (may reference each other -> fixpoint)
    let root_str = root.display().to_string();
    let base_lookup = |own: Option<u16>, urls: &HashMap<String, String>, key: &str| -> Option<String> {
        let parts: Vec<&str> = key.split('.').collect();
        match parts.as_slice() {
            ["port"] => own.map(|p| p.to_string()),
            ["mode"] => Some(mode.to_string()),
            ["project", "root"] => Some(root_str.clone()),
            ["env", name] => std::env::var(name).ok().or(Some(String::new())),
            ["services", name, "port"] => ports.get(*name).map(|p| p.to_string()),
            ["services", name, "url"] => urls.get(*name).cloned(),
            ["frontend", "url"] => urls.get(FRONTEND_SERVICE).cloned(),
            _ => None,
        }
    };

    let mut url_templates: Vec<(String, Option<String>)> =
        enabled.iter().map(|s| (s.name.clone(), s.url.clone())).collect();
    if let Some(plan) = &fe_plan {
        let t = match plan {
            FrontendPlan::Process { url, .. } | FrontendPlan::External { url } => Some(url.clone()),
            FrontendPlan::Static { .. } => None,
        };
        url_templates.push((FRONTEND_SERVICE.into(), t));
    }
    let mut urls: HashMap<String, String> = HashMap::new();
    for (name, template) in &url_templates {
        if template.is_none() {
            if let Some(p) = ports.get(name) {
                let suffix = if name == FRONTEND_SERVICE { "/" } else { "" };
                urls.insert(name.clone(), format!("http://127.0.0.1:{p}{suffix}"));
            }
        }
    }
    loop {
        let mut progress = false;
        for (name, template) in &url_templates {
            if urls.contains_key(name) {
                continue;
            }
            if let Some(t) = template {
                let own = ports.get(name).copied();
                if let Ok(v) = interpolate(t, |k| base_lookup(own, &urls, k)) {
                    urls.insert(name.clone(), v);
                    progress = true;
                }
            }
        }
        if !progress {
            break;
        }
    }
    for (name, template) in &url_templates {
        if let (Some(t), false) = (template, urls.contains_key(name)) {
            let own = ports.get(name).copied();
            let err = interpolate(t, |k| base_lookup(own, &urls, k)).unwrap_err();
            bail!("cannot resolve url of `{name}`: {err}");
        }
    }

    let resolve = |own: Option<u16>, text: &str, what: &str| -> Result<String> {
        interpolate(text, |k| base_lookup(own, &urls, k)).map_err(|e| anyhow!("{what}: {e}"))
    };
    let resolve_cmd = |own: Option<u16>, c: &CommandSpec, what: &str| -> Result<CommandSpec> {
        Ok(match c {
            CommandSpec::Shell(s) => CommandSpec::Shell(resolve(own, s, what)?),
            CommandSpec::Argv(v) => {
                CommandSpec::Argv(v.iter().map(|a| resolve(own, a, what)).collect::<Result<_>>()?)
            }
        })
    };
    let resolve_path = |p: &Option<String>| -> PathBuf {
        match p {
            Some(p) => {
                let pb = PathBuf::from(p);
                if pb.is_absolute() {
                    pb
                } else {
                    root.join(pb)
                }
            }
            None => root.clone(),
        }
    };
    let probe_from = |own: Option<u16>,
                      hc: &Option<HealthCheckSpec>,
                      fallback: Probe,
                      what: &str|
     -> Result<ProbeConfig> {
        match hc {
            Some(spec) => {
                let h = spec.normalized();
                let probe = if let Some(u) = &h.http {
                    Probe::Http { url: resolve(own, u, what)?, accept_any: false }
                } else if let Some(t) = &h.tcp {
                    Probe::Tcp { addr: resolve(own, t, what)? }
                } else {
                    fallback
                };
                Ok(ProbeConfig {
                    probe,
                    interval: Duration::from_millis(h.interval_ms),
                    timeout: Duration::from_millis(h.timeout_ms),
                    failure_threshold: h.failure_threshold.max(1),
                    ready_timeout: Duration::from_millis(h.ready_timeout_ms),
                })
            }
            None => Ok(ProbeConfig { probe: fallback, ..ProbeConfig::none() }),
        }
    };

    // 3. service definitions
    let mut defs = Vec::new();
    for name in startup_order(&enabled) {
        let s = enabled.iter().find(|s| s.name == name).expect("service");
        let own = ports.get(&s.name).copied();
        let what = |f: &str| format!("services.{}.{f}", s.name);
        let mut env = BTreeMap::new();
        for (k, v) in &s.env {
            env.insert(k.clone(), resolve(own, v, &what(&format!("env.{k}")))?);
        }
        if let Some(p) = own {
            env.entry("PORT".into()).or_insert_with(|| p.to_string());
        }
        let url = urls.get(&s.name).cloned();
        let kind = if s.external {
            DefKind::External
        } else {
            DefKind::Process {
                command: resolve_cmd(own, s.command.as_ref().expect("validated"), &what("command"))?,
                args: s.args.iter().map(|a| resolve(own, a, &what("args"))).collect::<Result<_>>()?,
            }
        };
        let fallback = match (own, &url, s.external) {
            (Some(p), _, false) => Probe::Tcp { addr: format!("127.0.0.1:{p}") },
            (_, Some(u), true) => Probe::Http { url: u.clone(), accept_any: true },
            (Some(p), None, true) => Probe::Tcp { addr: format!("127.0.0.1:{p}") },
            _ => Probe::None,
        };
        let cwd = resolve_path(&s.cwd);
        let build = match (&s.build, s.external) {
            (Some(b), false) => {
                let b = b.normalized();
                Some(BuildDef {
                    command: resolve_cmd(own, &b.command, &what("build"))?,
                    cwd: b.cwd.as_ref().map(|c| root.join(c)).unwrap_or_else(|| cwd.clone()),
                    on_start: match b.on_start {
                        BuildWhen::Always => true,
                        BuildWhen::Development => mode == Mode::Development,
                        BuildWhen::Never => false,
                    },
                })
            }
            _ => None,
        };
        defs.push(ServiceDef {
            build,
            name: s.name.clone(),
            description: s.description.clone(),
            category: s.kind.clone(),
            kind,
            env,
            port: own,
            url,
            probe: probe_from(own, &s.healthcheck, fallback, &what("healthcheck"))?,
            restart: s.restart_policy(),
            max_restarts: s.max_restarts,
            depends_on: s.depends_on.iter().filter(|d| enabled_names.contains(d)).cloned().collect(),
            autostart: s.autostart,
            setup: s.setup.as_ref().map(|st| SetupDef {
                command: st.command.clone(),
                creates: st.creates.as_ref().map(|c| root.join(c)),
                cwd: st.cwd.as_ref().map(|c| root.join(c)).unwrap_or_else(|| cwd.clone()),
            }),
            watch: s.watch.iter().map(|w| root.join(w)).collect(),
            stop_timeout: s.stop_timeout_ms.map(Duration::from_millis).unwrap_or(shutdown),
            cwd,
            is_frontend: false,
        });
    }

    // 4. frontend
    if let (Some(fe), Some(plan)) = (&cfg.frontend, fe_plan) {
        let own = ports.get(FRONTEND_SERVICE).copied();
        let url = urls.get(FRONTEND_SERVICE).cloned();
        let mut env = BTreeMap::new();
        for (k, v) in &fe.env {
            env.insert(k.clone(), resolve(own, v, &format!("frontend.env.{k}"))?);
        }
        if let Some(p) = own {
            env.entry("PORT".into()).or_insert_with(|| p.to_string());
        }
        // dev servers like Vite / CRA should not open a browser tab
        env.entry("BROWSER".into()).or_insert_with(|| "none".into());
        let cwd = resolve_path(&fe.cwd);
        let (kind, fallback) = match plan {
            FrontendPlan::Process { command, .. } => (
                DefKind::Process { command: resolve_cmd(own, &command, "frontend.command")?, args: vec![] },
                Probe::Http { url: url.clone().unwrap_or_default(), accept_any: true },
            ),
            FrontendPlan::Static { dist } => (
                DefKind::Static {
                    dist: root.join(dist),
                    proxy: fe
                        .proxy
                        .iter()
                        .map(|(k, v)| Ok((k.clone(), resolve(own, v, &format!("frontend.proxy.{k}"))?)))
                        .collect::<Result<_>>()?,
                },
                Probe::None,
            ),
            FrontendPlan::External { .. } => {
                (DefKind::External, Probe::Http { url: url.clone().unwrap_or_default(), accept_any: true })
            }
        };
        defs.push(ServiceDef {
            name: FRONTEND_SERVICE.into(),
            description: Some("Web frontend".into()),
            category: Some("frontend".into()),
            kind,
            env,
            port: own,
            url,
            probe: probe_from(own, &fe.healthcheck, fallback, "frontend.healthcheck")?,
            restart: RestartPolicy::OnFailure,
            max_restarts: 5,
            depends_on: fe.depends_on.iter().filter(|d| enabled_names.contains(d)).cloned().collect(),
            autostart: true,
            setup: fe.setup.as_ref().map(|st| SetupDef {
                command: st.command.clone(),
                creates: st.creates.as_ref().map(|c| root.join(c)),
                cwd: st.cwd.as_ref().map(|c| root.join(c)).unwrap_or_else(|| cwd.clone()),
            }),
            build: None,
            watch: fe.watch.iter().map(|w| root.join(w)).collect(),
            stop_timeout: shutdown,
            cwd,
            is_frontend: true,
        });
    }
    Ok(defs)
}
