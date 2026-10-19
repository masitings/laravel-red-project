//! The menu bar (tray) icon: what RED is serving at a glance, sites one click
//! away, services to start and stop, and the window to open again.
//!
//! The menu is rebuilt from redd's state whenever the daemon reports a
//! change; [`model`] decides what it shows, so it can be tested without a
//! window system.

use red_api::client::{Client, Method};
use red_api::{ServiceInfo, ServiceStatus, ServicesResponse, SiteInfo, StatusResponse, endpoints};
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

const TRAY_ID: &str = "red";

/// What redd reported, or `None` when it is not reachable.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub status: StatusResponse,
    pub sites: Vec<SiteInfo>,
    pub services: Vec<ServiceInfo>,
}

/// One menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Greyed-out text.
    Label(String),
    Item {
        id: String,
        text: String,
    },
    Submenu {
        text: String,
        entries: Vec<Entry>,
    },
    Separator,
}

fn item(id: impl Into<String>, text: impl Into<String>) -> Entry {
    Entry::Item {
        id: id.into(),
        text: text.into(),
    }
}

/// The menu for `snapshot`.
pub fn model(snapshot: Option<&Snapshot>) -> Vec<Entry> {
    let Some(snapshot) = snapshot else {
        return vec![
            Entry::Label("RED is not running".into()),
            item("start", "Start RED"),
            Entry::Separator,
            item("open", "Open RED"),
            item("quit", "Quit RED"),
        ];
    };
    let mut sites: Vec<Entry> = snapshot
        .sites
        .iter()
        .map(|site| item(format!("url:{}", site.url), host(&site.url)))
        .collect();
    if sites.is_empty() {
        sites.push(Entry::Label("No sites yet".into()));
    }
    let mut menu = vec![
        Entry::Label(format!("RED {} is running", snapshot.status.version)),
        Entry::Separator,
        Entry::Submenu {
            text: format!("Sites ({})", snapshot.sites.len()),
            entries: sites,
        },
    ];
    if !snapshot.services.is_empty() {
        let running = snapshot
            .services
            .iter()
            .filter(|s| s.status == ServiceStatus::Running)
            .count();
        menu.push(Entry::Submenu {
            text: format!("Services ({running}/{} running)", snapshot.services.len()),
            entries: snapshot.services.iter().map(service_entry).collect(),
        });
    }
    menu.extend([
        Entry::Separator,
        item("open", "Open RED"),
        item("nav:Mail", "Mail"),
        item("nav:Logs", "Logs"),
        Entry::Separator,
        item("quit", "Quit RED"),
    ]);
    menu
}

fn service_entry(service: &ServiceInfo) -> Entry {
    let name = &service.name;
    match service.status {
        ServiceStatus::Running => item(
            format!("service:stop:{name}"),
            format!("Stop {name} (port {})", service.port),
        ),
        ServiceStatus::Starting => Entry::Label(format!("{name} is starting…")),
        ServiceStatus::Stopped => item(format!("service:start:{name}"), format!("Start {name}")),
        ServiceStatus::Failed => item(
            format!("service:restart:{name}"),
            format!("Restart {name} (failed)"),
        ),
    }
}

/// `https://blog.test` → `blog.test`.
fn host(url: &str) -> String {
    url.split_once("://")
        .map_or(url, |(_, rest)| rest)
        .trim_end_matches('/')
        .to_owned()
}

/// Ask redd for a [`Snapshot`].
pub async fn snapshot(client: &Client) -> Option<Snapshot> {
    let status: StatusResponse = client.status().await.ok()?;
    let sites = client.get(endpoints::SITES).await.unwrap_or_default();
    let services = client
        .get::<ServicesResponse>(endpoints::SERVICES)
        .await
        .map(|r| r.services)
        .unwrap_or_default();
    Some(Snapshot {
        status,
        sites,
        services,
    })
}

fn build<R: Runtime>(app: &AppHandle<R>, entries: &[Entry]) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    for entry in entries {
        menu.append(item_for(app, entry)?.as_ref())?;
    }
    Ok(menu)
}

fn item_for<R: Runtime>(
    app: &AppHandle<R>,
    entry: &Entry,
) -> tauri::Result<Box<dyn IsMenuItem<R>>> {
    Ok(match entry {
        Entry::Label(text) => Box::new(MenuItem::new(app, text, false, None::<&str>)?),
        Entry::Item { id, text } => Box::new(MenuItem::with_id(
            app,
            id.as_str(),
            text,
            true,
            None::<&str>,
        )?),
        Entry::Separator => Box::new(PredefinedMenuItem::separator(app)?),
        Entry::Submenu { text, entries } => {
            let submenu = Submenu::new(app, text, true)?;
            for entry in entries {
                submenu.append(item_for(app, entry)?.as_ref())?;
            }
            Box::new(submenu)
        }
    })
}

/// Add the tray icon with a "not running yet" menu; [`refresh`] fills it.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = build(app, &model(None))?;
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("RED")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle(app, event.id().as_ref()));
    let builder = if cfg!(target_os = "macos") {
        builder
            .icon(tauri::image::Image::from_bytes(include_bytes!(
                "../icons/tray-template.png"
            ))?)
            .icon_as_template(true)
    } else {
        builder.icon(tauri::image::Image::from_bytes(include_bytes!(
            "../icons/32x32.png"
        ))?)
    };
    builder.build(app)?;
    Ok(())
}

/// Rebuild the menu from redd's current state.
pub async fn refresh<R: Runtime>(app: &AppHandle<R>, client: &Client) {
    let snapshot = snapshot(client).await;
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build(app, &model(snapshot.as_ref())) {
        let _ = tray.set_menu(Some(menu));
    }
    let tooltip = match &snapshot {
        Some(s) => format!("RED: {} sites", s.sites.len()),
        None => "RED is not running".to_owned(),
    };
    let _ = tray.set_tooltip(Some(tooltip));
}

/// Bring the main window back (and the Dock icon with it on macOS).
pub fn show_window<R: Runtime>(app: &AppHandle<R>) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Hide the main window; RED keeps running in the menu bar.
pub fn hide_window<R: Runtime>(window: &tauri::Window<R>) {
    let _ = window.hide();
    #[cfg(target_os = "macos")]
    let _ = window
        .app_handle()
        .set_activation_policy(tauri::ActivationPolicy::Accessory);
}

/// What a menu entry's id asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Action<'a> {
    Open,
    Navigate(&'a str),
    Quit,
    Start,
    OpenUrl(&'a str),
    Service { action: &'a str, name: &'a str },
}

pub fn parse(id: &str) -> Option<Action<'_>> {
    Some(match id {
        "open" => Action::Open,
        "quit" => Action::Quit,
        "start" => Action::Start,
        _ => {
            if let Some(url) = id.strip_prefix("url:") {
                Action::OpenUrl(url)
            } else if let Some(tab) = id.strip_prefix("nav:") {
                Action::Navigate(tab)
            } else {
                let rest = id.strip_prefix("service:")?;
                let (action, name) = rest.split_once(':')?;
                if !["start", "stop", "restart"].contains(&action) {
                    return None;
                }
                Action::Service { action, name }
            }
        }
    })
}

fn handle<R: Runtime>(app: &AppHandle<R>, id: &str) {
    use tauri::Emitter;
    use tauri_plugin_opener::OpenerExt;
    match parse(id) {
        Some(Action::Open) => show_window(app),
        Some(Action::Navigate(tab)) => {
            show_window(app);
            let _ = app.emit(crate::NAVIGATE_CHANNEL, tab);
        }
        Some(Action::Quit) => app.exit(0),
        Some(Action::Start) => {
            let _ = crate::commands::start_daemon();
        }
        Some(Action::OpenUrl(url)) => {
            let _ = app.opener().open_url(url, None::<&str>);
        }
        Some(Action::Service { action, name }) => {
            let Ok(paths) = red_platform::Paths::discover() else {
                return;
            };
            let client = Client::new(paths.socket());
            let path = endpoints::service_action(name, action);
            tauri::async_runtime::spawn(async move {
                // The daemon's ServicesChanged event refreshes the menu.
                let _ = client.request(Method::POST, &path, None).await;
            });
        }
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status() -> StatusResponse {
        serde_json::from_value(serde_json::json!({
            "version": "0.4.0", "api_version": 1, "os": "macos", "pid": 1, "uptime_secs": 1,
            "tld": "test", "http_port": 80, "https_port": 443, "dns_port": 53530,
            "helper": true, "smtp_port": 2525, "dump_port": 9912,
            "paths": {"config_dir": "/c", "data_dir": "/d", "socket": "/s", "ca_cert": "/ca"}
        }))
        .unwrap()
    }

    fn service(name: &str, status: ServiceStatus) -> ServiceInfo {
        serde_json::from_value(serde_json::json!({
            "name": name, "kind": "mysql", "flavor": null, "version": null, "binary": "/b",
            "port": 3306, "autostart": true, "status": status, "pid": null, "error": null,
            "data_dir": "/d", "log_file": "/l", "env": []
        }))
        .unwrap()
    }

    fn site(url: &str) -> SiteInfo {
        serde_json::from_value(serde_json::json!({
            "name": "blog", "path": "/p", "url": url, "kind": "linked", "driver": "laravel",
            "php": "8.4", "isolated": false, "secure": true, "cert_expires": null
        }))
        .unwrap()
    }

    #[test]
    fn offers_to_start_a_stopped_daemon() {
        let menu = model(None);
        assert_eq!(menu[0], Entry::Label("RED is not running".into()));
        assert!(menu.contains(&item("start", "Start RED")));
        assert!(menu.contains(&item("quit", "Quit RED")));
    }

    #[test]
    fn lists_sites_and_services() {
        let snapshot = Snapshot {
            status: status(),
            sites: vec![site("https://blog.test")],
            services: vec![
                service("mysql", ServiceStatus::Running),
                service("valkey", ServiceStatus::Stopped),
                service("pg", ServiceStatus::Failed),
            ],
        };
        let menu = model(Some(&snapshot));
        assert_eq!(menu[0], Entry::Label("RED 0.4.0 is running".into()));
        assert!(menu.contains(&Entry::Submenu {
            text: "Sites (1)".into(),
            entries: vec![item("url:https://blog.test", "blog.test")],
        }));
        let Some(Entry::Submenu { text, entries }) = menu
            .iter()
            .find(|e| matches!(e, Entry::Submenu { text, .. } if text.starts_with("Services")))
        else {
            panic!("no services submenu");
        };
        assert_eq!(text, "Services (1/3 running)");
        assert_eq!(
            entries[0],
            item("service:stop:mysql", "Stop mysql (port 3306)")
        );
        assert_eq!(entries[1], item("service:start:valkey", "Start valkey"));
        assert_eq!(
            entries[2],
            item("service:restart:pg", "Restart pg (failed)")
        );
    }

    #[test]
    fn says_when_there_are_no_sites() {
        let snapshot = Snapshot {
            status: status(),
            sites: vec![],
            services: vec![],
        };
        let menu = model(Some(&snapshot));
        assert!(menu.contains(&Entry::Submenu {
            text: "Sites (0)".into(),
            entries: vec![Entry::Label("No sites yet".into())],
        }));
        assert!(
            !menu
                .iter()
                .any(|e| matches!(e, Entry::Submenu { text, .. } if text.starts_with("Services")))
        );
    }

    #[test]
    fn parses_menu_ids() {
        assert_eq!(parse("open"), Some(Action::Open));
        assert_eq!(
            parse("url:https://a.test"),
            Some(Action::OpenUrl("https://a.test"))
        );
        assert_eq!(parse("nav:Mail"), Some(Action::Navigate("Mail")));
        assert_eq!(
            parse("service:stop:my:sql"),
            Some(Action::Service {
                action: "stop",
                name: "my:sql"
            })
        );
        assert_eq!(parse("service:rm:x"), None);
        assert_eq!(parse("bogus"), None);
    }
}
