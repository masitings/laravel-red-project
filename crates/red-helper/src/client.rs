//! Talking to the helper from `redd` (or the CLI).

use std::io::IoSliceMut;
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::path::Path;

use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, recvmsg};
use tokio::io::{AsyncWriteExt, Interest};
use tokio::net::UnixStream;

use crate::protocol::{Request, Response};

/// Errors talking to the helper.
#[derive(Debug, thiserror::Error)]
pub enum HelperError {
    #[error("the helper is not installed or not running ({0})")]
    Unavailable(std::io::Error),
    #[error("I/O error talking to the helper: {0}")]
    Io(#[from] std::io::Error),
    #[error("unexpected answer from the helper: {0}")]
    Protocol(String),
    #[error("the helper refused: {0}")]
    Refused(String),
}

/// Listening sockets received from the helper.
#[derive(Debug, Default)]
pub struct WebListeners {
    pub http: Vec<std::net::TcpListener>,
    pub https: Vec<std::net::TcpListener>,
}

/// Send `request` and return the response plus any passed descriptors.
pub async fn call(
    socket: &Path,
    request: &Request,
) -> Result<(Response, Vec<OwnedFd>), HelperError> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(HelperError::Unavailable)?;
    let mut line = serde_json::to_vec(request).expect("requests serialise");
    line.push(b'\n');
    stream.write_all(&line).await?;

    let mut data = Vec::new();
    let mut fds = Vec::new();
    let mut space = vec![MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(4))];
    while !data.ends_with(b"\n") {
        let mut buf = [0u8; 1024];
        let received = stream
            .async_io(Interest::READABLE, || {
                let mut control = RecvAncillaryBuffer::new(&mut space);
                let result = recvmsg(
                    &stream,
                    &mut [IoSliceMut::new(&mut buf)],
                    &mut control,
                    RecvFlags::empty(),
                )
                .map_err(std::io::Error::from)?;
                for message in control.drain() {
                    if let RecvAncillaryMessage::ScmRights(received) = message {
                        fds.extend(received);
                    }
                }
                Ok(result.bytes)
            })
            .await?;
        if received == 0 {
            return Err(HelperError::Protocol("connection closed early".into()));
        }
        data.extend_from_slice(&buf[..received]);
        if data.len() > 64 * 1024 {
            return Err(HelperError::Protocol("response too large".into()));
        }
    }
    let response: Response =
        serde_json::from_slice(&data).map_err(|err| HelperError::Protocol(err.to_string()))?;
    if !response.ok {
        return Err(HelperError::Refused(
            response.error.unwrap_or_else(|| "no reason given".into()),
        ));
    }
    Ok((response, fds))
}

/// Ask the helper for listeners on the privileged web ports.
pub async fn open_web_ports(socket: &Path) -> Result<WebListeners, HelperError> {
    let (response, fds) = call(socket, &Request::OpenWebPorts).await?;
    if response.fds.len() != fds.len() {
        return Err(HelperError::Protocol(format!(
            "expected {} descriptors, got {}",
            response.fds.len(),
            fds.len()
        )));
    }
    let mut listeners = WebListeners::default();
    for (label, fd) in response.fds.iter().zip(fds) {
        let listener = std::net::TcpListener::from(fd);
        listener.set_nonblocking(true)?;
        match label.as_str() {
            "http4" | "http6" => listeners.http.push(listener),
            "https4" | "https6" => listeners.https.push(listener),
            other => return Err(HelperError::Protocol(format!("unknown descriptor {other}"))),
        }
    }
    Ok(listeners)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server;

    async fn start(allowed: Vec<u32>) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("helper.sock");
        let mut config = server::Config::new(socket.clone(), allowed);
        config.http_port = 0;
        config.https_port = 0;
        config.resolver_dir = dir.path().join("resolver");
        let listener = server::bind(&socket).unwrap();
        tokio::spawn(server::serve(listener, config));
        (dir, socket)
    }

    fn me() -> u32 {
        rustix::process::getuid().as_raw()
    }

    #[tokio::test]
    async fn passes_listening_sockets() {
        let (_dir, socket) = start(vec![me()]).await;
        let listeners = open_web_ports(&socket).await.unwrap();
        assert!(!listeners.http.is_empty());
        assert!(!listeners.https.is_empty());

        // The received descriptor is a live listener.
        let listener =
            tokio::net::TcpListener::from_std(listeners.http.into_iter().next().unwrap()).unwrap();
        let addr = listener.local_addr().unwrap();
        let connect = tokio::net::TcpStream::connect(addr);
        let (accepted, connected) = tokio::join!(listener.accept(), connect);
        accepted.unwrap();
        connected.unwrap();
    }

    #[tokio::test]
    async fn reports_status_and_writes_resolver() {
        let (dir, socket) = start(vec![me()]).await;
        let (status, _) = call(&socket, &Request::Status).await.unwrap();
        assert_eq!(status.protocol, Some(crate::protocol::PROTOCOL_VERSION));

        call(
            &socket,
            &Request::InstallResolver {
                tld: "test".into(),
                port: 53530,
            },
        )
        .await
        .unwrap();
        let files: Vec<_> = std::fs::read_dir(dir.path().join("resolver"))
            .unwrap()
            .collect();
        assert_eq!(files.len(), 1);

        let err = call(
            &socket,
            &Request::InstallResolver {
                tld: "../../etc".into(),
                port: 1,
            },
        )
        .await
        .unwrap_err();
        assert!(matches!(err, HelperError::Refused(_)));

        call(&socket, &Request::RemoveResolver { tld: "test".into() })
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn rejects_other_users() {
        let me = me();
        if me == 0 {
            return; // root is always allowed, so this cannot be tested as root
        }
        let (_dir, socket) = start(vec![me + 1]).await;
        let err = call(&socket, &Request::Status).await.unwrap_err();
        assert!(matches!(err, HelperError::Refused(_)), "{err:?}");
    }

    #[tokio::test]
    async fn missing_helper_is_unavailable() {
        let dir = tempfile::tempdir().unwrap();
        let err = call(&dir.path().join("none.sock"), &Request::Status)
            .await
            .unwrap_err();
        assert!(matches!(err, HelperError::Unavailable(_)));
    }
}
