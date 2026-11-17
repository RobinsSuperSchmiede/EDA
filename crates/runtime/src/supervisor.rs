//! One supervisor task per service: a small state machine that reacts to the
//! desired state (`Control`) and to what the process actually does.

use crate::health;
use crate::logs::LogStream;
use crate::plan::{BuildDef, DefKind, SetupDef};
use crate::process::{build_command, terminate};
use crate::{static_server, Control, Inner, ServiceState, Slot};
use chrono::Utc;
use openshell_config::RestartPolicy;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::watch;

type Ctrl = watch::Receiver<Control>;

/// Why one run of a service ended.
enum Exit {
    StopRequested,
    RestartRequested,
    Exited(Option<i32>),
    Error(String),
}

pub(crate) async fn supervise(rt: Arc<Inner>, slot: Arc<Slot>) {
    let mut ctrl = slot.control.subscribe();
    loop {
        loop {
            if ctrl.borrow_and_update().run {
                break;
            }
            if ctrl.changed().await.is_err() {
                return;
            }
        }
        run_lifecycle(&rt, &slot, &mut ctrl).await;
    }
}

async fn run_lifecycle(rt: &Inner, slot: &Slot, ctrl: &mut Ctrl) {
    let name = slot.def.name.clone();
    let mut recent: VecDeque<Instant> = VecDeque::new();
    slot.update(rt, |s| {
        s.last_error = None;
        s.exit_code = None;
    });
    loop {
        if !wait_for_dependencies(rt, slot, ctrl).await {
            stopped(rt, slot);
            return;
        }
        if let Some(setup) = &slot.def.setup {
            match run_setup(rt, slot, setup, ctrl).await {
                Ok(true) => {}
                Ok(false) => {
                    stopped(rt, slot);
                    return;
                }
                Err(e) => {
                    rt.logs.system(&name, format!("setup failed: {e}"));
                    finish(rt, slot, ServiceState::Failed, Some(format!("setup failed: {e}")));
                    return;
                }
            }
        }

        if let (Some(build), DefKind::Process { .. }) = (&slot.def.build, &slot.def.kind) {
            if build.on_start {
                match run_build(rt, slot, build, ctrl).await {
                    Ok(true) => {}
                    Ok(false) => {
                        stopped(rt, slot);
                        return;
                    }
                    Err(e) => {
                        rt.logs.system(&name, format!("build failed: {e}"));
                        // a later file change (dev mode) or a manual start retries the build
                        finish(rt, slot, ServiceState::Failed, Some(format!("build failed: {e}")));
                        return;
                    }
                }
            }
        }

        let exit = match &slot.def.kind {
            DefKind::Process { .. } => run_process(rt, slot, ctrl).await,
            DefKind::External => run_external(rt, slot, ctrl).await,
            DefKind::Static { .. } => run_static(rt, slot, ctrl).await,
        };

        match exit {
            Exit::StopRequested => {
                rt.logs.system(&name, "stopped");
                stopped(rt, slot);
                return;
            }
            Exit::RestartRequested => {
                rt.logs.system(&name, "restarting");
                slot.update(rt, |s| s.restarts += 1);
                continue;
            }
            Exit::Error(e) => {
                rt.logs.system(&name, format!("error: {e}"));
                finish(rt, slot, ServiceState::Failed, Some(e));
                return;
            }
            Exit::Exited(code) => {
                let failed = code != Some(0);
                let should_restart = match slot.def.restart {
                    RestartPolicy::Never => false,
                    RestartPolicy::OnFailure => failed,
                    RestartPolicy::Always => true,
                };
                let code_text = code.map(|c| c.to_string()).unwrap_or_else(|| "signal".into());
                if !should_restart {
                    let state = if failed { ServiceState::Crashed } else { ServiceState::Exited };
                    let err = failed.then(|| format!("exited with code {code_text}"));
                    finish(rt, slot, state, err);
                    return;
                }
                let now = Instant::now();
                recent.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
                if recent.len() as u32 >= slot.def.max_restarts {
                    let msg = format!(
                        "crashed {} times within a minute (last exit code {code_text}); giving up",
                        recent.len() + 1
                    );
                    rt.logs.system(&name, &msg);
                    finish(rt, slot, ServiceState::Failed, Some(msg));
                    return;
                }
                recent.push_back(now);
                let backoff = Duration::from_millis(500 * (1u64 << (recent.len() - 1).min(5)))
                    .min(Duration::from_secs(10));
                rt.logs.system(
                    &name,
                    format!("exited with code {code_text}; restarting in {}ms", backoff.as_millis()),
                );
                slot.update(rt, |s| {
                    s.state = ServiceState::Backoff;
                    s.last_error = Some(format!("exited with code {code_text}"));
                });
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    r = ctrl.changed() => {
                        if r.is_err() || !ctrl.borrow_and_update().run {
                            stopped(rt, slot);
                            return;
                        }
                    }
                }
                slot.update(rt, |s| s.restarts += 1);
            }
        }
    }
}

/// Stopped on request. The desired state is left untouched: if the service was
/// started again while it was stopping, the supervisor starts it right away.
fn stopped(rt: &Inner, slot: &Slot) {
    slot.update(rt, |s| {
        s.state = ServiceState::Stopped;
        s.pid = None;
        s.metrics = None;
    });
}

/// Final state of a lifecycle; the service will not run again until started.
fn finish(rt: &Inner, slot: &Slot, state: ServiceState, error: Option<String>) {
    slot.set_run(false);
    slot.update(rt, |s| {
        s.state = state;
        s.pid = None;
        s.metrics = None;
        if error.is_some() {
            s.last_error = error;
        }
    });
}

async fn wait_for_dependencies(rt: &Inner, slot: &Slot, ctrl: &mut Ctrl) -> bool {
    let mut announced = false;
    loop {
        let missing: Vec<String> = slot
            .def
            .depends_on
            .iter()
            .filter(|d| rt.slot(d).map(|s| s.state() != ServiceState::Running).unwrap_or(false))
            .cloned()
            .collect();
        if missing.is_empty() {
            if announced {
                slot.update(rt, |s| s.last_error = None);
            }
            return true;
        }
        if !announced {
            let msg = format!("waiting for {}", missing.join(", "));
            rt.logs.system(&slot.def.name, &msg);
            slot.update(rt, |s| {
                s.state = ServiceState::Pending;
                s.last_error = Some(msg);
            });
            announced = true;
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
            r = ctrl.changed() => {
                if r.is_err() || !ctrl.borrow_and_update().run {
                    return false;
                }
            }
        }
    }
}

fn process_env(rt: &Inner, slot: &Slot) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("OPENSHELL".into(), "1".into());
    env.insert("OPENSHELL_MODE".into(), rt.mode.to_string());
    env.insert("OPENSHELL_SERVICE".into(), slot.def.name.clone());
    env.insert("OPENSHELL_PROJECT_ROOT".into(), rt.root.display().to_string());
    if std::env::var_os("PYTHONUNBUFFERED").is_none() {
        // make Python print logs immediately instead of buffering them
        env.insert("PYTHONUNBUFFERED".into(), "1".into());
    }
    for (k, v) in rt.extra_env.read().unwrap().iter() {
        env.insert(k.clone(), v.clone());
    }
    for (k, v) in &slot.def.env {
        env.insert(k.clone(), v.clone());
    }
    env
}

fn pipe_logs(rt: &Inner, name: &str, stream: LogStream, reader: impl AsyncRead + Unpin + Send + 'static) {
    let logs = rt.logs.clone();
    let name = name.to_string();
    tokio::spawn(async move {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => logs.push(&name, stream, String::from_utf8_lossy(&buf).into_owned()),
            }
        }
    });
}

/// Run the build step. Ok(true) when done, Ok(false) when a stop was requested.
async fn run_build(rt: &Inner, slot: &Slot, build: &BuildDef, ctrl: &mut Ctrl) -> Result<bool, String> {
    let name = &slot.def.name;
    rt.logs.system(name, format!("building: {}", build.command.display()));
    slot.update(rt, |s| {
        s.state = ServiceState::Building;
        s.metrics = None;
    });
    let started = std::time::Instant::now();
    let mut cmd =
        build_command(&build.command, &[], &build.cwd, &process_env(rt, slot)).map_err(|e| e.to_string())?;
    let mut child = cmd.spawn().map_err(|e| format!("cannot run `{}`: {e}", build.command.display()))?;
    if let Some(job) = &rt.job {
        job.assign(&child);
    }
    if let Some(out) = child.stdout.take() {
        pipe_logs(rt, name, LogStream::Stdout, out);
    }
    if let Some(err) = child.stderr.take() {
        pipe_logs(rt, name, LogStream::Stderr, err);
    }
    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(|e| e.to_string())?;
            if status.success() {
                rt.logs.system(name, format!("build finished in {:.1}s", started.elapsed().as_secs_f32()));
                Ok(true)
            } else {
                Err(format!("`{}` exited with {}", build.command.display(), status))
            }
        }
        r = ctrl.changed() => {
            terminate(&mut child, slot.def.stop_timeout).await;
            if r.is_err() || !ctrl.borrow_and_update().run { Ok(false) } else { Err("interrupted by a restart".into()) }
        }
    }
}

/// Returns Ok(true) when done, Ok(false) when a stop was requested.
async fn run_setup(rt: &Inner, slot: &Slot, setup: &SetupDef, ctrl: &mut Ctrl) -> Result<bool, String> {
    if let Some(creates) = &setup.creates {
        if creates.exists() {
            return Ok(true);
        }
    }
    let name = &slot.def.name;
    rt.logs.system(name, format!("running setup: {}", setup.command.display()));
    slot.set_state(rt, ServiceState::Setup);
    let mut cmd =
        build_command(&setup.command, &[], &setup.cwd, &process_env(rt, slot)).map_err(|e| e.to_string())?;
    let mut child = cmd.spawn().map_err(|e| format!("cannot run `{}`: {e}", setup.command.display()))?;
    if let Some(job) = &rt.job {
        job.assign(&child);
    }
    if let Some(out) = child.stdout.take() {
        pipe_logs(rt, name, LogStream::Stdout, out);
    }
    if let Some(err) = child.stderr.take() {
        pipe_logs(rt, name, LogStream::Stderr, err);
    }
    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(|e| e.to_string())?;
            if status.success() {
                rt.logs.system(name, "setup finished");
                Ok(true)
            } else {
                Err(format!("`{}` exited with {}", setup.command.display(), status))
            }
        }
        r = ctrl.changed() => {
            terminate(&mut child, slot.def.stop_timeout).await;
            if r.is_err() || !ctrl.borrow_and_update().run { Ok(false) } else { Err("interrupted".into()) }
        }
    }
}

async fn run_process(rt: &Inner, slot: &Slot, ctrl: &mut Ctrl) -> Exit {
    let DefKind::Process { command, args } = &slot.def.kind else { unreachable!() };
    let name = &slot.def.name;
    let mut cmd = match build_command(command, args, &slot.def.cwd, &process_env(rt, slot)) {
        Ok(c) => c,
        Err(e) => return Exit::Error(format!("{e:#}")),
    };
    if !slot.def.cwd.is_dir() {
        return Exit::Error(format!("working directory {} does not exist", slot.def.cwd.display()));
    }
    rt.logs.system(name, format!("starting: {}", slot.def.command_display().unwrap_or_default()));
    slot.update(rt, |s| {
        s.state = ServiceState::Starting;
        s.exit_code = None;
    });
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return Exit::Error(format!(
                "failed to start `{}`: {e}",
                slot.def.command_display().unwrap_or_default()
            ))
        }
    };
    if let Some(job) = &rt.job {
        job.assign(&child);
    }
    let pid = child.id();
    slot.update(rt, |s| {
        s.pid = pid;
        s.started_at = Some(Utc::now());
    });
    if let Some(out) = child.stdout.take() {
        pipe_logs(rt, name, LogStream::Stdout, out);
    }
    if let Some(err) = child.stderr.take() {
        pipe_logs(rt, name, LogStream::Stderr, err);
    }

    let probe = &slot.def.probe;
    let started = Instant::now();
    let mut ready = false;
    let mut failures = 0u32;
    loop {
        let delay = if ready { probe.interval } else { Duration::from_millis(250) };
        tokio::select! {
            status = child.wait() => {
                let code = status.ok().and_then(|s| s.code());
                slot.update(rt, |s| { s.pid = None; s.exit_code = code; });
                rt.logs.system(name, format!("process exited (code {})", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())));
                return Exit::Exited(code);
            }
            r = ctrl.changed() => {
                let c = r.ok().map(|_| *ctrl.borrow_and_update());
                slot.set_state(rt, ServiceState::Stopping);
                terminate(&mut child, slot.def.stop_timeout).await;
                slot.update(rt, |s| s.pid = None);
                return match c { Some(c) if c.run => Exit::RestartRequested, _ => Exit::StopRequested };
            }
            _ = tokio::time::sleep(delay) => {
                let result = health::check(&rt.http, &probe.probe, probe.timeout).await;
                health_transition(rt, slot, result, &mut ready, &mut failures, started);
            }
        }
    }
}

/// Shared health bookkeeping for processes and external services.
fn health_transition(
    rt: &Inner,
    slot: &Slot,
    result: Result<(), String>,
    ready: &mut bool,
    failures: &mut u32,
    started: Instant,
) {
    let probe = &slot.def.probe;
    let name = &slot.def.name;
    match (result, *ready) {
        (Ok(()), false) => {
            *ready = true;
            *failures = 0;
            rt.logs.system(name, format!("ready after {:.1}s", started.elapsed().as_secs_f32()));
            slot.update(rt, |s| {
                s.state = ServiceState::Running;
                s.last_error = None;
            });
        }
        (Err(e), false) => {
            if started.elapsed() > probe.ready_timeout && slot.state() != ServiceState::Unhealthy {
                let msg = format!("not healthy after {}s: {e}", probe.ready_timeout.as_secs());
                rt.logs.system(name, &msg);
                slot.update(rt, |s| {
                    s.state = ServiceState::Unhealthy;
                    s.last_error = Some(msg);
                });
            }
        }
        (Ok(()), true) => {
            *failures = 0;
            if slot.state() == ServiceState::Unhealthy {
                rt.logs.system(name, "healthy again");
                slot.update(rt, |s| {
                    s.state = ServiceState::Running;
                    s.last_error = None;
                });
            }
        }
        (Err(e), true) => {
            *failures += 1;
            if *failures >= probe.failure_threshold && slot.state() == ServiceState::Running {
                rt.logs.system(name, format!("health check failing: {e}"));
                slot.update(rt, |s| {
                    s.state = ServiceState::Unhealthy;
                    s.last_error = Some(e);
                });
            }
        }
    }
}

async fn run_external(rt: &Inner, slot: &Slot, ctrl: &mut Ctrl) -> Exit {
    let probe = &slot.def.probe;
    rt.logs.system(&slot.def.name, "monitoring external service");
    slot.set_state(rt, ServiceState::Starting);
    let started = Instant::now();
    let mut ready = false;
    let mut failures = 0;
    loop {
        let result = health::check(&rt.http, &probe.probe, probe.timeout).await;
        health_transition(rt, slot, result, &mut ready, &mut failures, started);
        let delay = if ready { probe.interval } else { Duration::from_millis(500) };
        tokio::select! {
            _ = tokio::time::sleep(delay) => {}
            r = ctrl.changed() => {
                return match r.ok().map(|_| *ctrl.borrow_and_update()) {
                    Some(c) if c.run => Exit::RestartRequested,
                    _ => Exit::StopRequested,
                };
            }
        }
    }
}

async fn run_static(rt: &Inner, slot: &Slot, ctrl: &mut Ctrl) -> Exit {
    let DefKind::Static { dist, proxy } = &slot.def.kind else { unreachable!() };
    let port = slot.def.port.unwrap_or(0);
    slot.set_state(rt, ServiceState::Starting);
    if !dist.is_dir() {
        return Exit::Error(format!(
            "frontend directory {} does not exist (build the frontend first)",
            dist.display()
        ));
    }
    let server = match static_server::start(
        port,
        dist.clone(),
        proxy.clone(),
        rt.html_injection.clone(),
        rt.http.clone(),
    )
    .await
    {
        Ok(s) => s,
        Err(e) => return Exit::Error(format!("cannot serve {}: {e}", dist.display())),
    };
    rt.logs
        .system(&slot.def.name, format!("serving {} at http://127.0.0.1:{}/", dist.display(), server.port));
    slot.update(rt, |s| {
        s.state = ServiceState::Running;
        s.started_at = Some(Utc::now());
        s.pid = Some(std::process::id());
        s.last_error = None;
    });
    let r = ctrl.changed().await;
    server.shutdown().await;
    match r.ok().map(|_| *ctrl.borrow_and_update()) {
        Some(c) if c.run => Exit::RestartRequested,
        _ => Exit::StopRequested,
    }
}
