//! Tauri shell for the RED GUI.
//!
//! Deliberately thin: it forwards Control API requests from the webview to
//! redd's unix socket and holds no business logic of its own. Everything the
//! GUI can do, the CLI can do through the same API.

mod commands;

/// Start the desktop app.
pub fn run() {
    tauri::Builder::default()
        .manage(commands::ApiState::discover())
        .invoke_handler(tauri::generate_handler![commands::api])
        .run(tauri::generate_context!())
        .expect("error while running the RED GUI");
}
