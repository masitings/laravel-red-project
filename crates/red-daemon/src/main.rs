use clap::Parser;
use red_platform::Paths;
use tracing_subscriber::EnvFilter;

/// redd: the RED Project daemon.
///
/// Serves the Control API on a unix socket. Normally started by launchd
/// (macOS) or systemd (Linux); run it directly while developing.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _args = Args::parse();
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let paths = Paths::discover()?;
    red_daemon::run(paths, shutdown_signal()).await?;
    Ok(())
}

/// Resolves on Ctrl-C or SIGTERM.
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut term = match signal(SignalKind::terminate()) {
        Ok(term) => term,
        Err(err) => {
            tracing::warn!(%err, "cannot listen for SIGTERM; only Ctrl-C will stop redd");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    tracing::info!("shutting down");
}
