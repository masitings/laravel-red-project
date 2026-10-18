//! Request and response bodies of the Control API.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Response of `GET /v1/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusResponse {
    /// Version of the running `redd` binary.
    pub version: String,
    /// Control API major version the daemon speaks.
    pub api_version: u32,
    /// Operating system the daemon runs on (`macos` or `linux`).
    pub os: String,
    /// Process id of the daemon.
    pub pid: u32,
    /// Seconds since the daemon started.
    #[ts(type = "number")]
    pub uptime_secs: u64,
    /// Top-level domain sites are served under.
    pub tld: String,
    /// Port the web server listens on, or `null` if it could not start.
    pub http_port: Option<u16>,
    /// Where this installation keeps its files.
    pub paths: StatusPaths,
}

/// How a site came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SiteKind {
    /// A sub-directory of a parked path.
    Parked,
    /// Linked explicitly by name.
    Linked,
}

/// One served site, as returned by `GET /v1/sites`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SiteInfo {
    /// Site name; the site answers on `<name>.<tld>`.
    pub name: String,
    /// Project directory.
    pub path: String,
    /// URL to open the site in a browser.
    pub url: String,
    pub kind: SiteKind,
    /// Detected framework driver, e.g. `laravel`.
    pub driver: String,
    /// PHP version (`"8.3"`) the site runs on, or `null` if none is installed.
    pub php: Option<String>,
    /// `true` when the PHP version is pinned for this site.
    pub isolated: bool,
}

/// Body of `POST /v1/paths` and `POST /v1/links`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PathRequest {
    /// Absolute directory path.
    pub path: String,
    /// Site name for links. Defaults to the directory name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
}

/// Body of `PUT /v1/php/default` and `PUT /v1/sites/{name}/php`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpVersionRequest {
    /// Minor version, e.g. `"8.3"`.
    pub version: String,
}

/// One PHP installation RED found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpInstallInfo {
    /// Minor version, e.g. `"8.3"`.
    pub version: String,
    /// Full version, e.g. `"8.3.6"`.
    pub full_version: String,
    /// Path of the `php-fpm` binary.
    pub fpm_path: String,
    /// `true` while RED runs a PHP-FPM pool for this version.
    pub running: bool,
}

/// Response of `GET /v1/php`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpListResponse {
    /// Version used by sites that are not isolated.
    pub default: Option<String>,
    /// Installed versions, newest first.
    pub installs: Vec<PhpInstallInfo>,
}

/// Directories reported by [`StatusResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusPaths {
    pub config_dir: String,
    pub data_dir: String,
    pub socket: String,
}

/// Machine-readable error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    /// The path or resource does not exist.
    NotFound,
    /// The request was malformed or failed validation.
    BadRequest,
    /// The request conflicts with the current state.
    Conflict,
    /// Something went wrong inside the daemon.
    Internal,
}

/// Body of every non-2xx response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApiError {
    pub code: ErrorCode,
    /// Human-readable description of what went wrong.
    pub message: String,
    /// Suggested next step, shown by the CLI and GUI when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hint: Option<String>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " (hint: {hint})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_snake_case() {
        let json = serde_json::to_string(&ErrorCode::NotFound).unwrap();
        assert_eq!(json, "\"not_found\"");
    }

    #[test]
    fn hint_is_omitted_when_absent() {
        let json = serde_json::to_value(ApiError::new(ErrorCode::Internal, "boom")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"code": "internal", "message": "boom"})
        );
    }
}
