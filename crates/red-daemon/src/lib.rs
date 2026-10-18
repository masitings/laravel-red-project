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
pub mod fastcgi;
pub mod fpm;
pub mod proxy;
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
    /// Change notifications for `GET /v1/events` subscribers.
    pub events: tokio::sync::broadcast::Sender<red_api::Event>,
    state: RwLock<State>,
}

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

    /// PHP version for `site`: pinned, else the default, else the newest
    /// installed version.
    pub fn effective_php(&self, state: &State, site: &str) -> Option<String> {
        state
            .isolated
            .get(site)
            .cloned()
            .or_else(|| state.default_php.clone())
            .or_else(|| self.php.newest())
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
    });
    renew_certificates(&app);
    let installs = app.php.discover().await;
    tracing::info!(
        versions = ?installs.iter().map(|i| &i.full_version).collect::<Vec<_>>(),
        "PHP installations found"
    );

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
            bind_web(https_port, FALLBACK_HTTPS_PORT, "HTTPS").await,
            bind_web(http_port, FALLBACK_HTTP_PORT, "HTTP").await,
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
    if let Err(err) = std::fs::remove_file(&socket_path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%err, "could not remove socket");
    }
    tracing::info!("redd stopped");
    result
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
/// configured one needs root and no helper is available yet.
async fn bind_web(
    port: u16,
    fallback: u16,
    what: &str,
) -> Option<(Vec<tokio::net::TcpListener>, u16)> {
    match web::bind(port).await {
        Ok(bound) => Some(bound),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied && port < 1024 => {
            tracing::warn!(
                port,
                fallback,
                "cannot bind a privileged {what} port without the helper; using the fallback port"
            );
            match web::bind(fallback).await {
                Ok(bound) => Some(bound),
                Err(err) => {
                    tracing::error!(%err, port = fallback, "{what} server disabled");
                    None
                }
            }
        }
        Err(err) => {
            tracing::error!(%err, port, "{what} server disabled");
            None
        }
    }
}
