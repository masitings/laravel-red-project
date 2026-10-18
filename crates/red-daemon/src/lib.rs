//! `redd`, the RED Project daemon.
//!
//! Serves the Control API on a unix socket, sites over HTTP and HTTPS on
//! loopback, and the local TLD over DNS; supervises one PHP-FPM pool per PHP
//! version in use. Ports 80/443 need the privileged helper (next step of
//! M1); without it RED falls back to unprivileged ports.

pub mod api;
pub mod body;
pub mod dns;
pub mod doctor;
pub mod dumps;
pub mod fastcgi;
pub mod fpm;
pub mod mail;
pub mod proxy;
pub mod services;
pub mod socket;
pub mod tls;
pub mod web;

use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use red_core::config::{FALLBACK_HTTP_PORT, FALLBACK_HTTPS_PORT};
use red_core::{Config, State};
use red_platform::Paths;
use tokio_util::sync::CancellationToken;

use crate::fpm::PhpManager;

/// Override `http_port` from `config.toml`.
pub const RED_HTTP_PORT_ENV: &str = "RED_HTTP_PORT";
/// Override `https_port` from `config.toml`.
pub const RED_HTTPS_PORT_ENV: &str = "RED_HTTPS_PORT";
/// Override `dns_port` from `config.toml`.
pub const RED_DNS_PORT_ENV: &str = "RED_DNS_PORT";

/// Errors that stop the daemon from starting or running.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("another redd is already running on {}", .0.display())]
    AlreadyRunning(std::path::PathBuf),
    #[error(transparent)]
    Config(#[from] red_core::ConfigError),
    #[error(transparent)]
    State(#[from] red_core::StateError),
    #[error(transparent)]
    Tls(#[from] tls::TlsError),
    #[error("invalid {name}: {value:?}")]
    InvalidPort { name: &'static str, value: String },
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

impl DaemonError {
    pub(crate) fn io(context: impl Into<String>) -> impl FnOnce(std::io::Error) -> Self {
        let context = context.into();
        move |source| Self::Io { context, source }
    }
}

/// State shared by every request handler.
#[derive(Debug)]
pub struct AppState {
    pub paths: Paths,
    pub config: Config,
    pub started: Instant,
    pub php: PhpManager,
    pub tls: tls::Authority,
    /// Ports actually in use, once bound.
    pub http_port: OnceLock<u16>,
    pub https_port: OnceLock<u16>,
    pub dns_port: OnceLock<u16>,
    /// `true` when the web ports came from the privileged helper.
    pub via_helper: std::sync::atomic::AtomicBool,
    /// Where the helper listens.
    pub helper_socket: std::path::PathBuf,
    /// Database and cache services.
    pub services: services::ServiceManager,
    /// Captured `dump()` output.
    pub dumps: Arc<dumps::Dumps>,
    /// Caught mail.
    pub mail: Arc<mail::MailStore>,
    /// Port of the mail catcher, once bound.
    pub smtp_port: OnceLock<u16>,
    /// Port of the dump server, once bound.
    pub dump_port: OnceLock<u16>,
    /// Why the web server is not on its configured ports, for `red doctor`.
    pub web_notes: std::sync::Mutex<Vec<String>>,
    /// Change notifications for `GET /v1/events` subscribers.
    pub events: tokio::sync::broadcast::Sender<red_api::Event>,
    state: RwLock<State>,
    /// `php` from projects' `red.toml`, keyed by file, with its mtime.
    project_php: std::sync::Mutex<ProjectPhpCache>,
}

type ProjectPhpCache =
    std::collections::HashMap<std::path::PathBuf, (Option<std::time::SystemTime>, Option<String>)>;

impl AppState {
    /// Tell event subscribers that something changed. Having no
    /// subscribers is fine.
    pub fn notify(&self, event: red_api::Event) {
        let _ = self.events.send(event);
    }

    /// A copy of the current state.
    pub fn state_snapshot(&self) -> State {
        self.state.read().expect("state lock poisoned").clone()
    }

    /// Apply `change` to the state and persist it. The state is only
    /// replaced in memory once it was written to disk.
    pub fn update_state<T>(
        &self,
        change: impl FnOnce(&mut State) -> T,
    ) -> Result<T, red_core::StateError> {
        let mut guard = self.state.write().expect("state lock poisoned");
        let mut next = guard.clone();
        let result = change(&mut next);
        if next != *guard {
            next.save(&self.paths.state_file())?;
            *guard = next;
        }
        Ok(result)
    }

    /// PHP version for `site`: pinned with `red isolate`, else `php` in
    /// the project's `red.toml`, else the default, else the newest
    /// installed version.
    pub fn effective_php(&self, state: &State, site: &red_core::Site) -> Option<String> {
        state
            .isolated
            .get(&site.name)
            .cloned()
            .or_else(|| self.project_php(&site.path))
            .or_else(|| state.default_php.clone())
            .or_else(|| self.php.newest())
    }

    /// `php` from `<dir>/red.toml`, cached until the file changes.
    fn project_php(&self, dir: &std::path::Path) -> Option<String> {
        let path = dir.join(red_core::project::FILE_NAME);
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let mut cache = self.project_php.lock().expect("project cache poisoned");
        if let Some((seen, php)) = cache.get(&path)
            && *seen == modified
        {
            return php.clone();
        }
        let php = modified.and_then(|_| match red_core::project::ProjectConfig::load(dir) {
            Ok(config) => config
                .and_then(|c| c.php)
                .and_then(|v| red_core::php::minor_version(&v)),
            Err(err) => {
                tracing::warn!(%err, "ignoring red.toml");
                None
            }
        });
        if cache.len() > 4096 {
            cache.clear();
        }
        cache.insert(path, (modified, php.clone()));
        php
    }

    /// PHP CLI binaries, newest PHP first, then `php` on `PATH`.
    pub fn php_cli_binaries(&self) -> Vec<std::path::PathBuf> {
        let mut found: Vec<std::path::PathBuf> = self
            .php
            .installs()
            .iter()
            .filter_map(|i| red_platform::binaries::php_cli_for_fpm(&i.fpm))
            .collect();
        for php in red_platform::binaries::in_path("php") {
            if !found.contains(&php) {
                found.push(php);
            }
        }
        found
    }

    /// Browser URL for a site: HTTPS when it is secured and the HTTPS
    /// server runs, HTTP otherwise.
    pub fn site_url(&self, state: &State, name: &str) -> String {
        let tld = &self.config.tld;
        if state.secured.contains_key(name)
            && let Some(port) = self.https_port.get()
        {
            return match port {
                443 => format!("https://{name}.{tld}"),
                port => format!("https://{name}.{tld}:{port}"),
            };
        }
        match self.http_port.get() {
            Some(80) | None => format!("http://{name}.{tld}"),
            Some(port) => format!("http://{name}.{tld}:{port}"),
        }
    }
}

fn port_setting(name: &'static str, configured: u16) -> Result<u16, DaemonError> {
    match std::env::var(name) {
        Ok(value) => value
            .parse()
            .map_err(|_| DaemonError::InvalidPort { name, value }),
        Err(_) => Ok(configured),
    }
}

/// Run the daemon until `shutdown` resolves.
///
/// Loads configuration and state, claims the Control API socket, starts the
/// DNS responder and the HTTP/HTTPS servers, and serves until shutdown.
/// PHP-FPM pools and the socket file are cleaned up before this returns.
pub async fn run(paths: Paths, shutdown: impl Future<Output = ()>) -> Result<(), DaemonError> {
    let config = Config::load(&paths.config_file())?;
    let state = State::load(&paths.state_file())?;
    let http_port = port_setting(RED_HTTP_PORT_ENV, config.http_port)?;
    let https_port = port_setting(RED_HTTPS_PORT_ENV, config.https_port)?;
    let dns_port = port_setting(RED_DNS_PORT_ENV, config.dns_port)?;

    let helper_socket = std::env::var_os(red_helper::RED_HELPER_SOCKET_ENV)
        .map(std::path::PathBuf::from)
        .or_else(|| config.helper_socket.clone());

    let socket_path = paths.socket();
    let listener = socket::bind(&socket_path).await?;
    let authority = tls::Authority::load_or_create(&paths.data_dir.join("certs"), &config.tld)?;

    let (events, _) = tokio::sync::broadcast::channel(64);
    let app = Arc::new(AppState {
        php: PhpManager::new(paths.clone(), events.clone()),
        services: services::ServiceManager::new(paths.clone(), events.clone()),
        smtp_port: OnceLock::new(),
        dump_port: OnceLock::new(),
        mail: Arc::new(mail::MailStore::open(paths.mail_dir())),
        web_notes: std::sync::Mutex::default(),
        dumps: Arc::new(dumps::Dumps::new(&paths.data_dir, state.capture_dumps)),
        events,
        tls: authority,
        paths,
        config,
        started: Instant::now(),
        http_port: OnceLock::new(),
        https_port: OnceLock::new(),
        dns_port: OnceLock::new(),
        via_helper: std::sync::atomic::AtomicBool::new(false),
        helper_socket: helper_socket
            .clone()
            .unwrap_or_else(red_helper::default_socket),
        state: RwLock::new(state),
        project_php: std::sync::Mutex::default(),
    });
    renew_certificates(&app);
    let installs = app.php.discover().await;
    tracing::info!(
        versions = ?installs.iter().map(|i| &i.full_version).collect::<Vec<_>>(),
        "PHP installations found"
    );

    start_services(&app).await;

    let cancel = CancellationToken::new();
    let mut tasks = Vec::new();

    match dns::bind(dns_port).await {
        Ok((socket, port)) => {
            let _ = app.dns_port.set(port);
            tracing::info!(port, tld = %app.config.tld, "DNS responder listening");
            tasks.push(tokio::spawn(dns::serve(
                socket,
                app.config.tld.clone(),
                cancel.clone(),
            )));
        }
        Err(err) => tracing::error!(%err, port = dns_port, "DNS responder disabled"),
    }

    match mail::bind(app.config.smtp_port).await {
        Ok((listener, port)) => {
            let _ = app.smtp_port.set(port);
            tracing::info!(port, "mail catcher listening");
            tasks.push(tokio::spawn(mail::serve(
                listener,
                app.mail.clone(),
                app.events.clone(),
                cancel.clone(),
            )));
        }
        Err(err) => tracing::error!(%err, port = app.config.smtp_port, "mail catcher disabled"),
    }

    match dumps::bind(app.config.dump_port).await {
        Ok((listener, port)) => {
            let _ = app.dump_port.set(port);
            tracing::info!(port, "dump server listening");
            let renderer_app = app.clone();
            tasks.push(tokio::spawn(dumps::serve(
                listener,
                app.dumps.clone(),
                move || dumps::Renderer {
                    php: renderer_app.php_cli_binaries(),
                    projects: red_core::sites::list(&renderer_app.state_snapshot())
                        .into_iter()
                        .map(|s| s.path)
                        .collect(),
                },
                app.events.clone(),
                cancel.clone(),
            )));
        }
        Err(err) => tracing::error!(%err, port = app.config.dump_port, "dump server disabled"),
    }

    // Privileged ports come from the helper when we are not root, or
    // whenever a helper socket is configured explicitly.
    let mut from_helper = None;
    if helper_socket.is_some()
        || (!red_platform::user::is_root() && (http_port < 1024 || https_port < 1024))
    {
        match red_helper::client::open_web_ports(&app.helper_socket).await {
            Ok(listeners) => {
                tracing::info!("received web ports from red-helper");
                app.via_helper
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                from_helper = Some(listeners);
            }
            Err(err) => tracing::info!(%err, "red-helper not available; using fallback ports"),
        }
    }
    let (https_listeners, http_listeners) = match from_helper {
        Some(listeners) => (adopt(listeners.https), adopt(listeners.http)),
        None => (
            bind_web(&app, https_port, FALLBACK_HTTPS_PORT, "HTTPS").await,
            bind_web(&app, http_port, FALLBACK_HTTP_PORT, "HTTP").await,
        ),
    };

    if let Some((listeners, port)) = https_listeners {
        let config = tls::server_config(app.clone())?;
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let _ = app.https_port.set(port);
        tracing::info!(port, "serving sites over HTTPS");
        tasks.push(tokio::spawn(web::serve_tls(
            listeners,
            app.clone(),
            acceptor,
            cancel.clone(),
        )));
    }

    if let Some((listeners, port)) = http_listeners {
        let _ = app.http_port.set(port);
        tracing::info!(port, "serving sites over HTTP");
        tasks.push(tokio::spawn(web::serve(
            listeners,
            app.clone(),
            cancel.clone(),
        )));
    }
    tracing::info!(socket = %socket_path.display(), "Control API listening");

    let result = socket::serve(listener, api::router(app.clone()), shutdown).await;

    cancel.cancel();
    for task in tasks {
        let _ = task.await;
    }
    app.php.shutdown().await;
    app.services.shutdown().await;
    if let Err(err) = std::fs::remove_file(&socket_path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%err, "could not remove socket");
    }
    tracing::info!("redd stopped");
    result
}

/// Find server binaries and start services marked for autostart.
async fn start_services(app: &AppState) {
    let installs = app.services.discover().await;
    tracing::info!(
        servers = ?installs
            .iter()
            .map(|i| format!("{} {}", i.server.flavor, i.server.full_version))
            .collect::<Vec<_>>(),
        "service servers found"
    );
    for (name, config) in app.state_snapshot().services {
        if config.autostart {
            app.services.start(&name, &config).await;
        }
    }
}

/// Turn listeners received from the helper into tokio listeners.
fn adopt(listeners: Vec<std::net::TcpListener>) -> Option<(Vec<tokio::net::TcpListener>, u16)> {
    let mut adopted = Vec::new();
    let mut port = None;
    for listener in listeners {
        match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => {
                if port.is_none() {
                    port = listener.local_addr().ok().map(|a| a.port());
                }
                adopted.push(listener);
            }
            Err(err) => tracing::warn!(%err, "unusable listener from red-helper"),
        }
    }
    Some((adopted, port?))
}

/// Reissue certificates of secured sites that expire soon.
fn renew_certificates(app: &AppState) {
    let due: Vec<String> = app
        .state_snapshot()
        .secured
        .iter()
        .filter(|(_, expires)| tls::Authority::needs_renewal(**expires))
        .map(|(site, _)| site.clone())
        .collect();
    for site in due {
        match app.tls.issue(&site) {
            Ok(expires) => {
                if let Err(err) = app.update_state(|s| s.secured.insert(site.clone(), expires)) {
                    tracing::warn!(%site, %err, "could not record renewed certificate");
                } else {
                    tracing::info!(%site, "renewed certificate");
                }
            }
            Err(err) => tracing::warn!(%site, %err, "could not renew certificate"),
        }
    }
}

/// Bind a web listener, falling back to an unprivileged port when the
/// configured one needs root (and no helper is installed) or is already
/// used by another program.
async fn bind_web(
    app: &AppState,
    port: u16,
    fallback: u16,
    what: &str,
) -> Option<(Vec<tokio::net::TcpListener>, u16)> {
    let reason = match web::bind(port).await {
        Ok(bound) => return Some(bound),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied && port < 1024 => {
            format!("port {port} needs the privileged helper")
        }
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse && port != fallback => {
            let note = format!(
                "port {port} is used by another program (another web server such as Herd, Valet, nginx or Apache?)"
            );
            app.web_notes
                .lock()
                .expect("web notes poisoned")
                .push(note.clone());
            note
        }
        Err(err) => {
            tracing::error!(%err, port, "{what} server disabled");
            app.web_notes
                .lock()
                .expect("web notes poisoned")
                .push(format!("{what} on port {port}: {err}"));
            return None;
        }
    };
    tracing::warn!(
        port,
        fallback,
        "{reason}; using the fallback port for {what}"
    );
    match web::bind(fallback).await {
        Ok(bound) => Some(bound),
        Err(err) => {
            tracing::error!(%err, port = fallback, "{what} server disabled");
            app.web_notes
                .lock()
                .expect("web notes poisoned")
                .push(format!("{what} fallback port {fallback}: {err}"));
            None
        }
    }
}
