package dev.openshell.android

/**
 * The OpenShell core inside this process (`libopenshell_android.so`, crate `crates/android`).
 * Every function takes and returns JSON strings: `{"ok": true, ...}` or `{"ok": false, "error": "..."}`.
 * Calls block; run them off the main thread.
 */
object NativeRuntime {
    init {
        System.loadLibrary("openshell_android")
    }

    /** Start the app in [projectDir]; state (databases, logs, tokens) goes to [stateDir]. */
    @JvmStatic external fun start(projectDir: String, stateDir: String, nativeLibDir: String, mode: String): String

    /** Forward a lifecycle event, e.g. `{"kind":"background"}` or `{"kind":"network","connected":true,"metered":false}`. */
    @JvmStatic external fun lifecycle(event: String): String

    @JvmStatic external fun status(): String

    @JvmStatic external fun stop(): String

    /** Unpack an app bundle (.osapp) into [appsDir]. */
    @JvmStatic external fun installBundle(bundle: String, appsDir: String): String

    @JvmStatic external fun listApps(appsDir: String): String
}
