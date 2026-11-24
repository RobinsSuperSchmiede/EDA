//! # openshell-android
//!
//! The bridge between the OpenShell Android app (Kotlin, `apps/android`) and the unchanged
//! core. The app loads `libopenshell_android.so` and calls the functions of
//! `dev.openshell.android.NativeRuntime`; every call takes and returns JSON strings, so the
//! Kotlin side stays small.
//!
//! | Kotlin (`NativeRuntime`)                         | Rust                    |
//! |--------------------------------------------------|-------------------------|
//! | `start(projectDir, stateDir, nativeLibDir, mode)` | [`Runtime::start`]      |
//! | `lifecycle(eventJson)`                           | [`Runtime::lifecycle`]  |
//! | `status()`                                       | [`Runtime::status`]     |
//! | `stop()`                                         | [`Runtime::stop`]       |
//! | `installBundle(bundlePath, appsDir)`             | [`install_bundle`]      |
//! | `listApps(appsDir)`                              | [`list_apps`]           |
//!
//! Only programs inside the APK may run on Android: the adapter allows the native library
//! directory, where the app ships `libopenshell_android.so` and optional runtimes
//! (`libnode.so`, `libpython.so` registered as `node` / `python`).

use anyhow::{anyhow, Context, Result};
use openshell_core::config::{Mode, Project};
use openshell_core::runtime::{AndroidAdapter, LifecycleEvent, RuntimeAdapter};
use openshell_core::{HeadlessHost, Shell};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

/// Bundled runtimes the adapter registers when the APK contains them.
const BUNDLED: &[(&str, &str)] =
    &[("node", "libnode.so"), ("python", "libpython.so"), ("sqlite3", "libsqlite3.so")];

/// The adapter for the app: system tools, the APK's native library directory and bundled
/// runtimes found there.
pub fn adapter_for(native_lib_dir: &Path) -> AndroidAdapter {
    let mut a = AndroidAdapter::new().with_executable_dir(native_lib_dir);
    for (name, file) in BUNDLED {
        let p = native_lib_dir.join(file);
        if p.is_file() {
            a = a.with_program(*name, p);
        }
    }
    a
}

/// One running app inside the Android process.
pub struct Runtime {
    rt: tokio::runtime::Runtime,
    shell: Option<Shell>,
}

impl Runtime {
    pub fn new() -> Result<Self> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("openshell")
            .build()?;
        Ok(Self { rt, shell: None })
    }

    /// Start the app in `project_dir` with its state (databases, logs, tokens) in `state_dir`.
    /// Stops a previously started app first.
    pub fn start(
        &mut self,
        project_dir: &Path,
        state_dir: &Path,
        adapter: Arc<dyn RuntimeAdapter>,
        mode: Mode,
    ) -> Result<Value> {
        self.stop();
        let project = Project::load(project_dir)
            .with_context(|| format!("no OpenShell app in {}", project_dir.display()))?
            .with_state_dir(state_dir);
        let shell = self.rt.block_on(async move {
            let shell = Shell::launch_with(project, mode, Arc::new(HeadlessHost::default()), adapter).await?;
            shell.start();
            anyhow::Ok(shell)
        })?;
        let info = describe(&shell);
        self.shell = Some(shell);
        Ok(info)
    }

    fn shell(&self) -> Result<&Shell> {
        self.shell.as_ref().ok_or_else(|| anyhow!("no app is running"))
    }

    /// Forward an Activity / system event; returns the paused or resumed services and the
    /// services that need the foreground service (`keep_alive`).
    pub fn lifecycle(&self, event: &str) -> Result<Value> {
        let shell = self.shell()?;
        let ev: LifecycleEvent = serde_json::from_str(event).context("invalid lifecycle event")?;
        let services = shell.lifecycle(ev);
        Ok(json!({ "services": services, "keep_alive": shell.runtime().keep_alive() }))
    }

    pub fn status(&self) -> Value {
        match &self.shell {
            Some(s) => describe(s),
            None => json!({ "running": false }),
        }
    }

    pub fn stop(&mut self) {
        if let Some(shell) = self.shell.take() {
            self.rt.block_on(shell.shutdown());
        }
    }
}

fn describe(shell: &Shell) -> Value {
    let rt = shell.runtime();
    json!({
        "running": true,
        "app": rt.app_name(),
        "frontend_url": rt.frontend_url(),
        "control_url": shell.control_url(),
        "keep_alive": rt.keep_alive(),
        "services": rt.status(),
        "remote": shell.remote(),
        "platform": rt.capabilities(),
    })
}

/// Unpack an app bundle (`.osapp`) into `apps_dir`.
pub fn install_bundle(bundle: &Path, apps_dir: &Path) -> Result<Value> {
    let (m, dir) = openshell_core::bundle::install(bundle, apps_dir)?;
    Ok(json!({ "app": m.app, "identifier": m.identifier, "version": m.version, "dir": dir }))
}

/// Installed apps.
pub fn list_apps(apps_dir: &Path) -> Value {
    let apps: Vec<Value> = openshell_core::bundle::installed(apps_dir)
        .into_iter()
        .map(|(m, dir)| json!({ "app": m.app, "identifier": m.identifier, "version": m.version, "dir": dir }))
        .collect();
    json!({ "apps": apps })
}

/// `{"ok": true, …}` or `{"ok": false, "error": "…"}` for the Kotlin side.
fn reply(r: Result<Value>) -> String {
    match r {
        Ok(Value::Object(mut m)) => {
            m.insert("ok".into(), Value::Bool(true));
            Value::Object(m).to_string()
        }
        Ok(v) => json!({ "ok": true, "result": v }).to_string(),
        Err(e) => json!({ "ok": false, "error": format!("{e:#}") }).to_string(),
    }
}

fn runtime() -> &'static Mutex<Option<Runtime>> {
    static RT: OnceLock<Mutex<Option<Runtime>>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(None))
}

fn with_runtime<T>(f: impl FnOnce(&mut Runtime) -> Result<T>) -> Result<T> {
    let mut guard = runtime().lock().map_err(|_| anyhow!("runtime lock poisoned"))?;
    if guard.is_none() {
        *guard = Some(Runtime::new()?);
    }
    f(guard.as_mut().unwrap())
}

/// Entry points used by the JNI functions (also usable from tests and other hosts).
pub mod api {
    use super::*;

    pub fn start(project_dir: &str, state_dir: &str, native_lib_dir: &str, mode: &str) -> String {
        let mode = if mode == "development" { Mode::Development } else { Mode::Production };
        let adapter: Arc<dyn RuntimeAdapter> = Arc::new(adapter_for(Path::new(native_lib_dir)));
        reply(with_runtime(|rt| rt.start(Path::new(project_dir), Path::new(state_dir), adapter, mode)))
    }
    pub fn lifecycle(event: &str) -> String {
        reply(with_runtime(|rt| rt.lifecycle(event)))
    }
    pub fn status() -> String {
        reply(with_runtime(|rt| Ok(rt.status())))
    }
    pub fn stop() -> String {
        reply(with_runtime(|rt| {
            rt.stop();
            Ok(json!({}))
        }))
    }
    pub fn install_bundle(bundle: &str, apps_dir: &str) -> String {
        reply(super::install_bundle(Path::new(bundle), Path::new(apps_dir)))
    }
    pub fn list_apps(apps_dir: &str) -> String {
        reply(Ok(super::list_apps(Path::new(apps_dir))))
    }
}

/// JNI exports for `dev.openshell.android.NativeRuntime`.
mod jni_exports {
    use jni::objects::{JClass, JString};
    use jni::sys::jstring;
    use jni::JNIEnv;

    fn arg(env: &mut JNIEnv, s: &JString) -> String {
        env.get_string(s).map(|s| s.into()).unwrap_or_default()
    }

    fn out(env: &mut JNIEnv, s: String) -> jstring {
        env.new_string(s).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
    }

    #[no_mangle]
    pub extern "system" fn Java_dev_openshell_android_NativeRuntime_start<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        project_dir: JString<'l>,
        state_dir: JString<'l>,
        native_lib_dir: JString<'l>,
        mode: JString<'l>,
    ) -> jstring {
        let (p, s, n, m) = (
            arg(&mut env, &project_dir),
            arg(&mut env, &state_dir),
            arg(&mut env, &native_lib_dir),
            arg(&mut env, &mode),
        );
        let r = super::api::start(&p, &s, &n, &m);
        out(&mut env, r)
    }

    #[no_mangle]
    pub extern "system" fn Java_dev_openshell_android_NativeRuntime_lifecycle<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        event: JString<'l>,
    ) -> jstring {
        let e = arg(&mut env, &event);
        let r = super::api::lifecycle(&e);
        out(&mut env, r)
    }

    #[no_mangle]
    pub extern "system" fn Java_dev_openshell_android_NativeRuntime_status<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
    ) -> jstring {
        let r = super::api::status();
        out(&mut env, r)
    }

    #[no_mangle]
    pub extern "system" fn Java_dev_openshell_android_NativeRuntime_stop<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
    ) -> jstring {
        let r = super::api::stop();
        out(&mut env, r)
    }

    #[no_mangle]
    pub extern "system" fn Java_dev_openshell_android_NativeRuntime_installBundle<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        bundle: JString<'l>,
        apps_dir: JString<'l>,
    ) -> jstring {
        let (b, a) = (arg(&mut env, &bundle), arg(&mut env, &apps_dir));
        let r = super::api::install_bundle(&b, &a);
        out(&mut env, r)
    }

    #[no_mangle]
    pub extern "system" fn Java_dev_openshell_android_NativeRuntime_listApps<'l>(
        mut env: JNIEnv<'l>,
        _class: JClass<'l>,
        apps_dir: JString<'l>,
    ) -> jstring {
        let a = arg(&mut env, &apps_dir);
        let r = super::api::list_apps(&a);
        out(&mut env, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openshell_core::runtime::DesktopAdapter;

    #[test]
    fn adapter_registers_bundled_runtimes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("libnode.so"), "").unwrap();
        let a = adapter_for(dir.path());
        assert_eq!(a.programs().collect::<Vec<_>>(), vec!["node"]);
        assert!(!a.capabilities().toolchains);
    }

    #[test]
    fn start_lifecycle_stop() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        std::fs::create_dir_all(app.join("web")).unwrap();
        std::fs::write(app.join("web/index.html"), "<h1>hi</h1>").unwrap();
        std::fs::write(
            app.join("openshell.json"),
            r#"{"app":{"name":"Phone App"},"frontend":{"dist":"web"},"databases":[{"name":"main"}]}"#,
        )
        .unwrap();
        let state = dir.path().join("state");
        let mut rt = Runtime::new().unwrap();
        // the desktop adapter stands in for the Android one on the test host
        let info = rt.start(&app, &state, Arc::new(DesktopAdapter::desktop()), Mode::Production).unwrap();
        assert_eq!(info["app"], "Phone App");
        let url = info["frontend_url"].as_str().unwrap().to_string();
        assert!(url.starts_with("http://127.0.0.1:"), "{url}");
        assert!(state.join("data").join("main.db").is_file() || state.join("data").is_dir());

        let r = rt.lifecycle(r#"{"kind":"background"}"#).unwrap();
        assert!(r["keep_alive"].is_array());
        assert!(rt.lifecycle(r#"{"kind":"nap"}"#).is_err());
        assert_eq!(rt.status()["running"], true);
        rt.stop();
        assert_eq!(rt.status()["running"], false);
        assert!(rt.lifecycle(r#"{"kind":"foreground"}"#).is_err());
    }

    #[test]
    fn replies_are_json_with_ok() {
        let ok: Value = serde_json::from_str(&reply(Ok(json!({ "a": 1 })))).unwrap();
        assert_eq!(ok, json!({ "a": 1, "ok": true }));
        let err: Value = serde_json::from_str(&reply(Err(anyhow!("boom")))).unwrap();
        assert_eq!(err["ok"], false);
        let apps: Value = serde_json::from_str(&api::list_apps("/does/not/exist")).unwrap();
        assert_eq!(apps["apps"], json!([]));
    }
}
