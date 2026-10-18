//! red-helper binary. Installed by `red helper:install` as a launchd daemon
//! (macOS) or systemd service (Linux), running as root.

use std::path::PathBuf;

use clap::Parser;
use red_helper::server;

/// Privileged helper for RED Project. Opens loopback ports 80/443 for redd
/// and manages the resolver file. Not meant to be run by hand.
#[derive(Debug, Parser)]
#[command(version)]
struct Args {
    /// Socket to listen on.
    #[arg(long, default_value_os_t = red_helper::default_socket())]
    socket: PathBuf,
    /// User allowed to connect (repeatable). Root is always allowed.
    #[arg(long = "allow-uid", required = true)]
    allow_uid: Vec<u32>,
    #[arg(long, default_value_t = 80)]
    http_port: u16,
    #[arg(long, default_value_t = 443)]
    https_port: u16,
    /// Directory for resolver files.
    #[arg(long, default_value_os_t = red_helper::resolver::default_dir())]
    resolver_dir: PathBuf,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> std::process::ExitCode {
    let args = Args::parse();
    let config = server::Config {
        socket: args.socket.clone(),
        allowed_uids: args.allow_uid,
        http_port: args.http_port,
        https_port: args.https_port,
        resolver_dir: args.resolver_dir,
    };
    let listener = match server::bind(&args.socket) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!(
                "red-helper: cannot listen on {}: {err}",
                args.socket.display()
            );
            return std::process::ExitCode::FAILURE;
        }
    };
    eprintln!(
        "red-helper {} listening on {} (allowed uids: {:?})",
        env!("CARGO_PKG_VERSION"),
        args.socket.display(),
        config.allowed_uids
    );
    server::serve(listener, config).await;
    std::process::ExitCode::SUCCESS
}
