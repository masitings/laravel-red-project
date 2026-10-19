//! Tauri shell for the RED GUI.
//!
//! Deliberately thin: it forwards Control API requests from the webview to
//! redd's unix socket, relays daemon events to the webview, and holds no
//! business logic of its own. Everything the GUI can do, the CLI can do
//! through the same API.

mod commands;
mod tray;

use std::time::Duration;

use tauri::{Emitter, Manager};

/// Name of the webview event that carries daemon events.
pub const EVENT_CHANNEL: &str = "red://event";

/// Argument the login item starts the app with.
const HIDDEN_ARG: &str = "--hidden";

/// Webview event asking it to show a tab (the tray's "Mail", "Logs").
pub const NAVIGATE_CHANNEL: &str = "red://navigate";

/// Start the desktop app.
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .manage(commands::ApiState::discover())
        .invoke_handler(tauri::generate_handler![
            commands::api,
            commands::save_mail_file,
            commands::start_daemon,
            commands::restart_daemon,
            commands::setup_step,
            commands::app_autostart,
            commands::set_app_autostart,
            commands::cli_status,
            commands::install_cli,
            commands::updater_enabled
        ])
        .setup(|app| {
            // Release builds made with an update signing key carry the
            // updater's public key; other builds (local, forks) have no
            // updater at all rather than one that cannot verify anything.
            if updater_configured(app.config()) {
                app.handle()
                    .plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            commands::refresh_appimage_cli();
            if let Err(err) = tray::install(app.handle()) {
                // No tray on this desktop (e.g. GNOME without the
                // AppIndicator extension); the window still works.
                eprintln!("red-gui: no tray icon: {err}");
            }
            // Opened at login: stay in the menu bar until clicked.
            if cfg!(target_os = "macos")
                && std::env::args().any(|a| a == HIDDEN_ARG)
                && let Some(window) = app.get_webview_window("main")
            {
                tray::hide_window(&window.as_ref().window());
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(relay_events(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            // On macOS RED lives in the menu bar: closing the window hides
            // it. Elsewhere a tray may not be visible, so closing quits.
            if cfg!(target_os = "macos")
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                tray::hide_window(window);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building the RED GUI");
    app.run(|app, event| {
        // Clicking the Dock icon brings the window back.
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            tray::show_window(app);
        }
        let _ = (app, event);
    });
}

/// `true` when the bundle configuration has an updater public key.
pub fn updater_configured(config: &tauri::Config) -> bool {
    has_updater_key(config.plugins.0.get("updater"))
}

fn has_updater_key(updater: Option<&serde_json::Value>) -> bool {
    updater
        .and_then(|updater| updater.get("pubkey"))
        .and_then(|key| key.as_str())
        .is_some_and(|key| !key.trim().is_empty())
}

/// Subscribe to redd's event stream and re-emit every event to the webview,
/// reconnecting whenever the daemon restarts.
async fn relay_events(app: tauri::AppHandle) {
    let Ok(paths) = red_platform::Paths::discover() else {
        return;
    };
    let client = red_api::client::Client::new(paths.socket());
    let (changed, mut refresh) = tokio::sync::mpsc::unbounded_channel::<()>();
    {
        // Rebuild the tray menu after changes, outside the event callback.
        let app = app.clone();
        let client = client.clone();
        tauri::async_runtime::spawn(async move {
            while refresh.recv().await.is_some() {
                while refresh.try_recv().is_ok() {}
                tray::refresh(&app, &client).await;
            }
        });
    }
    loop {
        let result = client
            .events(|event| {
                if tray_cares(&event) {
                    let _ = changed.send(());
                }
                app.emit(EVENT_CHANNEL, &event).is_ok()
            })
            .await;
        if let Err(err) = result {
            log_debug(&err.to_string());
        }
        // Let the webview refetch: the daemon may have restarted.
        let _ = app.emit(EVENT_CHANNEL, &red_api::Event::Connected);
        let _ = changed.send(());
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Events that change what the tray menu shows.
fn tray_cares(event: &red_api::Event) -> bool {
    matches!(
        event,
        red_api::Event::Connected | red_api::Event::SitesChanged | red_api::Event::ServicesChanged
    )
}

fn log_debug(message: &str) {
    if cfg!(debug_assertions) {
        eprintln!("red-gui: event stream: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::has_updater_key;

    #[test]
    fn updater_needs_a_public_key() {
        assert!(!has_updater_key(None));
        assert!(!has_updater_key(Some(
            &serde_json::json!({ "endpoints": [] })
        )));
        assert!(!has_updater_key(Some(
            &serde_json::json!({ "pubkey": " " })
        )));
        assert!(has_updater_key(Some(
            &serde_json::json!({ "pubkey": "dW50cnVzdGVk" })
        )));
    }
}
