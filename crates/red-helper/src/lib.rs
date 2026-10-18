//! red-helper: the only part of RED that runs as root.
//!
//! Its job is deliberately tiny and fixed:
//!
//! 1. open loopback listeners on the web ports (80 and 443 by default) and
//!    hand their file descriptors to `redd`;
//! 2. write or remove the resolver file that points the local TLD at redd's
//!    DNS responder;
//! 3. report its version.
//!
//! Requests are a closed enum ([`protocol::Request`]); paths, ports and
//! file contents are never taken from the client. Only the users named at
//! install time may connect (checked with the socket's peer credentials).

pub mod client;
pub mod protocol;
pub mod resolver;
pub mod server;

/// Where the helper listens unless told otherwise.
pub fn default_socket() -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        "/var/run/red-helper.sock".into()
    } else {
        "/run/red-helper.sock".into()
    }
}

/// Environment variable that points `redd` at a different helper socket.
pub const RED_HELPER_SOCKET_ENV: &str = "RED_HELPER_SOCKET";
