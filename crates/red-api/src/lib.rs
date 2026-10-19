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
    /// `PUT`: when sites run with Xdebug, see [`crate::XdebugRequest`].
    pub const XDEBUG: &str = "/v1/xdebug";
    /// `GET`: health checks, see [`crate::DoctorReport`].
    pub const DOCTOR: &str = "/v1/doctor";
    /// `POST`: install the resolver file through the helper. `DELETE`: remove it.
    pub const DNS: &str = "/v1/dns";
    /// `GET`: Server-Sent Events stream of [`crate::Event`].
    pub const EVENTS: &str = "/v1/events";
    /// `GET`: proxies. `POST`: add one. `DELETE /v1/proxies/{name}`: remove.
    pub const PROXIES: &str = "/v1/proxies";
    /// `GET`: services and installable servers. `POST`: add a service.
    /// `PUT`/`DELETE /v1/services/{name}`: change / remove one
    /// (`?delete_data=true` also deletes its data).
    /// `POST /v1/services/{name}/{start,stop,restart}`.
    pub const SERVICES: &str = "/v1/services";
    /// `GET`: caught mail (`?mailbox=` filters). `DELETE`: delete all.
    /// `GET`/`DELETE /v1/mail/{id}`, `GET /v1/mail/{id}/raw`,
    /// `GET /v1/mail/{id}/attachments/{index}`.
    pub const MAIL: &str = "/v1/mail";
    /// `GET`: captured dumps. `DELETE`: clear them.
    pub const DUMPS: &str = "/v1/dumps";
    /// `PUT`: switch dump capturing on or off.
    pub const DUMP_SETTINGS: &str = "/v1/dumps/settings";
    /// `GET`: installed and downloadable runtimes, and installs in
    /// progress. `POST`: install one (returns the job).
    /// `DELETE /v1/runtimes/{kind}/{line}`: remove one.
    pub const RUNTIMES: &str = "/v1/runtimes";
    /// `GET`: RED's settings. `PUT`: change them (applies after a restart).
    pub const SETTINGS: &str = "/v1/settings";
    /// `GET`: recent project creations. `POST`: create a project.
    pub const PROJECTS: &str = "/v1/projects";
    /// `GET`: known log files.
    pub const LOGS: &str = "/v1/logs";
    /// `GET ?id=<log id>&lines=<n>`: the end of one log.
    pub const LOG_READ: &str = "/v1/logs/read";

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

    /// `/v1/services/{name}`
    pub fn service(name: &str) -> String {
        format!("{SERVICES}/{name}")
    }

    /// `/v1/services/{name}/{action}` with `action` one of `start`, `stop`,
    /// `restart`.
    pub fn service_action(name: &str, action: &str) -> String {
        format!("{SERVICES}/{name}/{action}")
    }

    /// `/v1/runtimes/{kind}/{line}`
    pub fn runtime(kind: &str, line: &str) -> String {
        format!("{RUNTIMES}/{kind}/{line}")
    }

    /// `/v1/php/{version}/settings`: `PUT` replaces RED's php.ini settings.
    pub fn php_settings(version: &str) -> String {
        format!("{PHP}/{version}/settings")
    }

    /// `/v1/php/{version}/info`: extensions and effective settings.
    pub fn php_info(version: &str) -> String {
        format!("{PHP}/{version}/info")
    }

    /// `/v1/mail/{id}`
    pub fn mail(id: &str) -> String {
        format!("{MAIL}/{id}")
    }

    /// `/v1/links/{name}`
    pub fn link(name: &str) -> String {
        format!("{LINKS}/{name}")
    }
}
