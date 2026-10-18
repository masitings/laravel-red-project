//! Domain model for RED Project.
//!
//! This crate holds pure logic (configuration, and later sites and drivers)
//! and performs no network IO, so it is easy to test in isolation.

pub mod config;

pub use config::{Config, ConfigError};

/// Version of the RED workspace this crate was built from.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
