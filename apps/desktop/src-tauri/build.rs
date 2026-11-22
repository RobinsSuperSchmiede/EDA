fn main() {
    // App commands get permissions (`allow-<command>`), so a frontend loaded from the app's URL can be
    // given exactly the ones it needs (`show_window_menu` for an overlay title bar). OpenShell's own
    // pages get all of them in capabilities/default.json.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "get_state",
            "service_action",
            "open_app",
            "resolve_approval",
            "open_runtime_panel",
            "show_window_menu",
            "db_rows",
            "call_function",
        ]),
    ))
    .expect("failed to run tauri-build");
}
