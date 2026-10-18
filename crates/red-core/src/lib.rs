//! Domain model for RED Project.
//!
//! This crate holds the logic that decides *what* to serve (configuration,
//! state, sites, drivers, PHP versions). It never talks to the network, which
//! keeps it easy to test in isolation.

pub mod config;
pub mod driver;
pub mod php;
pub mod sites;
pub mod state;

pub use config::{Config, ConfigError};
pub use sites::{Site, SiteKind};
pub use state::{State, StateError};

/// Version of the RED workspace this crate was built from.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
