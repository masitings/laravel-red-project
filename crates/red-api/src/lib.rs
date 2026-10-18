//! Control API types for RED Project.
//!
//! The daemon (`redd`), the CLI (`red`) and the GUI all speak this API, so
//! every request and response type is defined exactly once, here. TypeScript
//! definitions for the GUI are generated from these types with `ts-rs`
//! (`just gen-types`, or `cargo test -p red-api`).

pub mod types;

#[cfg(feature = "client")]
pub mod client;

pub use types::*;

/// Major version of the Control API. Every path starts with `/v{API_VERSION}`.
pub const API_VERSION: u32 = 1;

/// Request paths served by the daemon.
pub mod endpoints {
    /// `GET`: daemon status, see [`crate::StatusResponse`].
    pub const STATUS: &str = "/v1/status";
}
