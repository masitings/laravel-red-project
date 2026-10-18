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
    /// `GET`: all sites. `GET /v1/sites/{name}`: one site.
    /// `PUT`/`DELETE /v1/sites/{name}/php`: isolate / unisolate.
    pub const SITES: &str = "/v1/sites";
    /// `GET`: parked paths. `POST`: park. `DELETE` (with a JSON body): forget.
    pub const PATHS: &str = "/v1/paths";
    /// `POST`: link a directory. `DELETE /v1/links/{name}`: unlink.
    pub const LINKS: &str = "/v1/links";
    /// `GET`: installed PHP versions.
    pub const PHP: &str = "/v1/php";
    /// `PUT`: change the default PHP version.
    pub const PHP_DEFAULT: &str = "/v1/php/default";
    /// `GET`: health checks, see [`crate::DoctorReport`].
    pub const DOCTOR: &str = "/v1/doctor";
    /// `POST`: install the resolver file through the helper. `DELETE`: remove it.
    pub const DNS: &str = "/v1/dns";
    /// `GET`: Server-Sent Events stream of [`crate::Event`].
    pub const EVENTS: &str = "/v1/events";
    /// `GET`: proxies. `POST`: add one. `DELETE /v1/proxies/{name}`: remove.
    pub const PROXIES: &str = "/v1/proxies";

    /// `/v1/sites/{name}`
    pub fn site(name: &str) -> String {
        format!("{SITES}/{name}")
    }

    /// `/v1/sites/{name}/php`
    pub fn site_php(name: &str) -> String {
        format!("{SITES}/{name}/php")
    }

    /// `/v1/sites/{name}/secure`: `PUT` secures, `DELETE` unsecures.
    pub fn site_secure(name: &str) -> String {
        format!("{SITES}/{name}/secure")
    }

    /// `/v1/proxies/{name}`
    pub fn proxy(name: &str) -> String {
        format!("{PROXIES}/{name}")
    }

    /// `/v1/proxies/{name}/secure`: `PUT` secures, `DELETE` unsecures.
    pub fn proxy_secure(name: &str) -> String {
        format!("{PROXIES}/{name}/secure")
    }

    /// `/v1/links/{name}`
    pub fn link(name: &str) -> String {
        format!("{LINKS}/{name}")
    }
}
