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
    /// Where this installation keeps its files.
    pub paths: StatusPaths,
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
