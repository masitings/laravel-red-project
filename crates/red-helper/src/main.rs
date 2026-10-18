//! red-helper: the only part of RED that runs as root.
//!
//! Its job is deliberately tiny and fixed (see the design doc):
//!
//! 1. open the TCP sockets for ports 80 and 443 on loopback and hand their
//!    file descriptors to `redd`;
//! 2. write or remove the resolver file for the configured TLD;
//! 3. report its own status and version.
//!
//! It never accepts paths or commands from clients. The IPC itself lands in
//! M1; in M0 this crate only pins down the request vocabulary.

mod protocol;

fn main() {
    let version = env!("CARGO_PKG_VERSION");
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("red-helper {version}");
        return;
    }
    eprintln!(
        "red-helper {version} (protocol v{}): not functional yet, arrives in M1",
        protocol::PROTOCOL_VERSION
    );
    std::process::exit(2);
}
