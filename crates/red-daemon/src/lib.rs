//! `redd`, the RED Project daemon.
//!
//! In M0 the daemon only serves the Control API on its unix socket. The web
//! server, DNS responder and process supervisor are added in M1.

pub mod api;
pub mod socket;

use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use red_core::Config;
use red_platform::Paths;

/// Errors that stop the daemon from starting or running.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("another redd is already running on {}", .0.display())]
    AlreadyRunning(std::path::PathBuf),
    #[error(transparent)]
    Config(#[from] red_core::ConfigError),
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
}

/// Run the daemon until `shutdown` resolves.
///
/// Loads the configuration, claims the Control API socket and serves
/// requests. The socket file is removed again before this returns.
pub async fn run(paths: Paths, shutdown: impl Future<Output = ()>) -> Result<(), DaemonError> {
    let config = Config::load(&paths.config_file())?;
    let socket_path = paths.socket();
    let listener = socket::bind(&socket_path).await?;
    tracing::info!(socket = %socket_path.display(), tld = %config.tld, "redd listening");

    let state = Arc::new(AppState {
        paths,
        config,
        started: Instant::now(),
    });
    let result = socket::serve(listener, api::router(state), shutdown).await;

    if let Err(err) = std::fs::remove_file(&socket_path)
        && err.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%err, "could not remove socket");
    }
    tracing::info!("redd stopped");
    result
}
