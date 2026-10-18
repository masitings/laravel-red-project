//! The helper's listening side. Runs as root under launchd or systemd.

use std::io::IoSlice;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
use tokio::io::Interest;
use tokio::net::{UnixListener, UnixStream};

use crate::protocol::{PROTOCOL_VERSION, Request, Response, valid_tld};
use crate::resolver;

/// Longest request line accepted.
const MAX_REQUEST: usize = 4096;

/// Fixed settings, chosen by whoever installed the helper (never by clients).
#[derive(Debug, Clone)]
pub struct Config {
    pub socket: PathBuf,
    /// Users allowed to talk to the helper, in addition to root.
    pub allowed_uids: Vec<u32>,
    pub http_port: u16,
    pub https_port: u16,
    pub resolver_dir: PathBuf,
}

impl Config {
    pub fn new(socket: PathBuf, allowed_uids: Vec<u32>) -> Self {
        Self {
            socket,
            allowed_uids,
            http_port: 80,
            https_port: 443,
            resolver_dir: resolver::default_dir(),
        }
    }
}

/// Bind the helper socket. It is world-connectable on purpose: access is
/// decided per connection from the peer's uid.
pub fn bind(socket: &Path) -> std::io::Result<UnixListener> {
    if let Some(dir) = socket.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o666))?;
    Ok(listener)
}

/// Serve connections forever.
pub async fn serve(listener: UnixListener, config: Config) {
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(err) => {
                eprintln!("red-helper: accept failed: {err}");
                continue;
            }
        };
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(err) = handle(stream, &config).await {
                eprintln!("red-helper: {err}");
            }
        });
    }
}

async fn handle(stream: UnixStream, config: &Config) -> std::io::Result<()> {
    let uid = stream.peer_cred()?.uid();
    if uid != 0 && !config.allowed_uids.contains(&uid) {
        return reply(
            &stream,
            &Response::error(format!("uid {uid} is not allowed")),
            &[],
        )
        .await;
    }

    let line = read_line(&stream).await?;
    let request: Request = match serde_json::from_slice(&line) {
        Ok(request) => request,
        Err(err) => {
            return reply(
                &stream,
                &Response::error(format!("bad request: {err}")),
                &[],
            )
            .await;
        }
    };

    match request {
        Request::Status => {
            let response = Response {
                version: Some(env!("CARGO_PKG_VERSION").to_owned()),
                protocol: Some(PROTOCOL_VERSION),
                ..Response::ok()
            };
            reply(&stream, &response, &[]).await
        }
        Request::OpenWebPorts => match open_web_ports(config) {
            Ok(listeners) => {
                let response = Response {
                    fds: listeners
                        .iter()
                        .map(|(label, _)| label.to_string())
                        .collect(),
                    ..Response::ok()
                };
                let fds: Vec<BorrowedFd<'_>> = listeners.iter().map(|(_, fd)| fd.as_fd()).collect();
                reply(&stream, &response, &fds).await
            }
            Err(err) => reply(&stream, &Response::error(err.to_string()), &[]).await,
        },
        Request::InstallResolver { tld, port } => {
            let response = if !valid_tld(&tld) || port == 0 {
                Response::error("invalid tld or port")
            } else {
                match resolver::install(&config.resolver_dir, &tld, port) {
                    Ok(_) => {
                        reload_resolver();
                        Response::ok()
                    }
                    Err(err) => Response::error(err.to_string()),
                }
            };
            reply(&stream, &response, &[]).await
        }
        Request::RemoveResolver { tld } => {
            let response = if !valid_tld(&tld) {
                Response::error("invalid tld")
            } else {
                match resolver::remove(&config.resolver_dir, &tld) {
                    Ok(_) => {
                        reload_resolver();
                        Response::ok()
                    }
                    Err(err) => Response::error(err.to_string()),
                }
            };
            reply(&stream, &response, &[]).await
        }
    }
}

/// Bind the configured web ports on loopback (IPv6 when available).
fn open_web_ports(config: &Config) -> std::io::Result<Vec<(&'static str, OwnedFd)>> {
    let mut listeners = Vec::new();
    for (label4, label6, port) in [
        ("http4", "http6", config.http_port),
        ("https4", "https6", config.https_port),
    ] {
        let v4 = std::net::TcpListener::bind(("127.0.0.1", port))?;
        let actual = v4.local_addr()?.port();
        listeners.push((label4, OwnedFd::from(v4)));
        if let Ok(v6) = std::net::TcpListener::bind(("::1", actual)) {
            listeners.push((label6, OwnedFd::from(v6)));
        }
    }
    Ok(listeners)
}

/// Make the OS pick up a changed resolver file where that is needed.
fn reload_resolver() {
    if cfg!(target_os = "linux") {
        // systemd-resolved reads drop-ins only on (re)start.
        let _ = std::process::Command::new("systemctl")
            .args(["try-restart", "systemd-resolved"])
            .status();
    }
    // macOS watches /etc/resolver itself.
}

async fn reply(
    stream: &UnixStream,
    response: &Response,
    fds: &[BorrowedFd<'_>],
) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(response).expect("responses serialise");
    line.push(b'\n');
    let mut space = vec![MaybeUninit::<u8>::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut sent = 0;
    while sent < line.len() {
        let chunk = &line[sent..];
        let n = stream
            .async_io(Interest::WRITABLE, || {
                let mut control = SendAncillaryBuffer::new(&mut space);
                if sent == 0 && !fds.is_empty() {
                    control.push(SendAncillaryMessage::ScmRights(fds));
                }
                sendmsg(
                    stream,
                    &[IoSlice::new(chunk)],
                    &mut control,
                    SendFlags::empty(),
                )
                .map_err(std::io::Error::from)
            })
            .await?;
        sent += n;
    }
    Ok(())
}

/// Read one request line (without the newline), at most [`MAX_REQUEST`] bytes.
async fn read_line(stream: &UnixStream) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::new();
    let mut buf = [0u8; 512];
    loop {
        stream.readable().await?;
        match stream.try_read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                line.extend_from_slice(&buf[..n]);
                if let Some(end) = line.iter().position(|b| *b == b'\n') {
                    line.truncate(end);
                    break;
                }
                if line.len() > MAX_REQUEST {
                    return Err(std::io::Error::other("request too long"));
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => continue,
            Err(err) => return Err(err),
        }
    }
    Ok(line)
}
