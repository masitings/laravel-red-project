//! The Control API unix socket: claiming it safely and serving connections.
//!
//! Only processes running as the same user as the daemon may talk to it. The
//! socket file is created with mode `0600` inside a `0700` directory, and the
//! peer's uid is checked on every connection as a second line of defence.

use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use axum::Router;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::net::{UnixListener, UnixStream};

use crate::DaemonError;

/// Create the run directory and bind the socket, replacing a stale socket
/// file left behind by a crashed daemon.
pub async fn bind(socket: &Path) -> Result<UnixListener, DaemonError> {
    let dir = socket.parent().ok_or_else(|| DaemonError::Io {
        context: format!("socket path {} has no parent", socket.display()),
        source: std::io::ErrorKind::InvalidInput.into(),
    })?;
    std::fs::create_dir_all(dir).map_err(DaemonError::io(format!("create {}", dir.display())))?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
        .map_err(DaemonError::io(format!("chmod {}", dir.display())))?;

    if socket.exists() {
        if UnixStream::connect(socket).await.is_ok() {
            return Err(DaemonError::AlreadyRunning(socket.to_owned()));
        }
        tracing::warn!(socket = %socket.display(), "removing stale socket");
        std::fs::remove_file(socket)
            .map_err(DaemonError::io(format!("remove {}", socket.display())))?;
    }

    let listener = UnixListener::bind(socket)
        .map_err(DaemonError::io(format!("bind {}", socket.display())))?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .map_err(DaemonError::io(format!("chmod {}", socket.display())))?;
    Ok(listener)
}

/// Accept connections until `shutdown` resolves.
pub async fn serve(
    listener: UnixListener,
    router: Router,
    shutdown: impl Future<Output = ()>,
) -> Result<(), DaemonError> {
    let own_uid = red_platform::user::current_uid();
    tokio::pin!(shutdown);

    loop {
        let stream = tokio::select! {
            () = &mut shutdown => return Ok(()),
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(err) => {
                    tracing::warn!(%err, "accept failed");
                    continue;
                }
            },
        };

        match stream.peer_cred() {
            Ok(cred) if cred.uid() == own_uid => {}
            Ok(cred) => {
                tracing::warn!(
                    peer_uid = cred.uid(),
                    "rejected connection from another user"
                );
                continue;
            }
            Err(err) => {
                tracing::warn!(%err, "could not read peer credentials");
                continue;
            }
        }

        let service = TowerToHyperService::new(router.clone());
        tokio::spawn(async move {
            let result = hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await;
            if let Err(err) = result {
                tracing::debug!(%err, "connection closed with error");
            }
        });
    }
}
