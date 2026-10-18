//! Tauri shell for the RED GUI.
//!
//! Deliberately thin: it forwards Control API requests from the webview to
//! redd's unix socket, relays daemon events to the webview, and holds no
//! business logic of its own. Everything the GUI can do, the CLI can do
//! through the same API.

mod commands;

use std::time::Duration;

use tauri::Emitter;

/// Name of the webview event that carries daemon events.
pub const EVENT_CHANNEL: &str = "red://event";

/// Start the desktop app.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::ApiState::discover())
        .invoke_handler(tauri::generate_handler![
            commands::api,
            commands::save_mail_file,
            commands::start_daemon,
            commands::cli_status,
            commands::install_cli
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(relay_events(handle));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running the RED GUI");
}

/// Subscribe to redd's event stream and re-emit every event to the webview,
/// reconnecting whenever the daemon restarts.
async fn relay_events(app: tauri::AppHandle) {
    let Ok(paths) = red_platform::Paths::discover() else {
        return;
    };
    let client = red_api::client::Client::new(paths.socket());
    loop {
        let result = client
            .events(|event| app.emit(EVENT_CHANNEL, &event).is_ok())
            .await;
        if let Err(err) = result {
            log_debug(&err.to_string());
        }
        // Let the webview refetch: the daemon may have restarted.
        let _ = app.emit(EVENT_CHANNEL, &red_api::Event::Connected);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

fn log_debug(message: &str) {
    if cfg!(debug_assertions) {
        eprintln!("red-gui: event stream: {message}");
    }
}
