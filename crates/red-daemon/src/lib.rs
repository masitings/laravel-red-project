//! `redd`, the RED Project daemon.
//!
//! Serves the Control API on a unix socket and sites on a loopback HTTP
//! port, and supervises one PHP-FPM pool per PHP version in use. DNS, TLS
//! and the privileged helper follow later in M1.

pub mod api;
pub mod fastcgi;
pub mod fpm;
pub mod socket;
pub mod web;

use std::future::Future;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use red_core::config::FALLBACK_HTTP_PORT;
use red_core::{Config, State};
use red_platform::Paths;
use tokio_util::sync::CancellationToken;

use crate::fpm::PhpManager;

/// Overrides `http_port` from `config.toml`.
pub const RED_HTTP_PORT_ENV: &str = "RED_HTTP_PORT";

/// Errors that stop the daemon from starting or running.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("another redd is already running on {}", .0.display())]
    AlreadyRunning(std::path::PathBuf),
    #[error(transparent)]
    Config(#[from] red_core::ConfigError),
    #[error(transparent)]
    State(#[from] red_core::StateError),
    #[error("invalid {RED_HTTP_PORT_ENV}: {0:?}")]
    InvalidPort(String),
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
    /// Port the web server listens on, once it is bound.
    pub http_port: OnceLock<u16>,
    state: RwLock<State>,
}

impl AppState {
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

    /// Browser URL for a site.
    pub fn site_url(&self, name: &str) -> String {
        match self.http_port.get() {
            Some(80) | None => format!("http://{name}.{}", self.config.tld),
            Some(port) => format!("http://{name}.{}:{port}", self.config.tld),
        }
    }
}

/// Run the daemon until `shutdown` resolves.
///
/// Loads configuration and state, claims the Control API socket, binds the
/// web server and serves both. PHP-FPM pools and the socket file are cleaned
/// up before this returns.
pub async fn run(paths: Paths, shutdown: impl Future<Output = ()>) -> Result<(), DaemonError> {
    let config = Config::load(&paths.config_file())?;
    let state = State::load(&paths.state_file())?;
    let http_port = match std::env::var(RED_HTTP_PORT_ENV) {
        Ok(value) => value.parse().map_err(|_| DaemonError::InvalidPort(value))?,
        Err(_) => config.http_port,
    };

    let socket_path = paths.socket();
    let listener = socket::bind(&socket_path).await?;

    let app = Arc::new(AppState {
        php: PhpManager::new(paths.clone()),
        paths,
        config,
        started: Instant::now(),
        http_port: OnceLock::new(),
        state: RwLock::new(state),
    });
    let installs = app.php.discover().await;
    tracing::info!(
        versions = ?installs.iter().map(|i| &i.full_version).collect::<Vec<_>>(),
        "PHP installations found"
    );

    let cancel = CancellationToken::new();
    let web_task = match bind_web(http_port).await {
        Some((listeners, port)) => {
            let _ = app.http_port.set(port);
            tracing::info!(port, tld = %app.config.tld, "serving sites");
            Some(tokio::spawn(web::serve(
                listeners,
                app.clone(),
                cancel.clone(),
            )))
        }
        None => None,
    };
    tracing::info!(socket = %socket_path.display(), "Control API listening");

    let result = socket::serve(listener, api::router(app.clone()), shutdown).await;

    cancel.cancel();
    if let Some(task) = web_task {
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

/// Bind the web server, falling back to an unprivileged port when the
/// configured one needs root and no helper is available yet.
async fn bind_web(port: u16) -> Option<(Vec<tokio::net::TcpListener>, u16)> {
    match web::bind(port).await {
        Ok(bound) => Some(bound),
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied && port < 1024 => {
            tracing::warn!(
                port,
                fallback = FALLBACK_HTTP_PORT,
                "cannot bind a privileged port without the helper; using the fallback port"
            );
            match web::bind(FALLBACK_HTTP_PORT).await {
                Ok(bound) => Some(bound),
                Err(err) => {
                    tracing::error!(%err, port = FALLBACK_HTTP_PORT, "web server disabled");
                    None
                }
            }
        }
        Err(err) => {
            tracing::error!(%err, port, "web server disabled");
            None
        }
    }
}
