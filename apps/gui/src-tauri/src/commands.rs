//! Commands the webview may invoke.

use red_api::client::{Client, ClientError, Method};
use red_platform::Paths;
use serde::Serialize;

/// Client for the local redd, or the reason there is none.
#[derive(Debug)]
pub struct ApiState(Result<Client, String>);

impl ApiState {
    pub fn discover() -> Self {
        Self(
            Paths::discover()
                .map(|paths| Client::new(paths.socket()))
                .map_err(|err| err.to_string()),
        )
    }

    #[cfg(test)]
    pub fn with_client(client: Client) -> Self {
        Self(Ok(client))
    }
}

/// Error shape sent to the webview. Matches `RedApiError` in
/// `src/api/client.ts`.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl CommandError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_owned(),
            message: message.into(),
            hint: None,
            status: None,
        }
    }
}

impl From<ClientError> for CommandError {
    fn from(err: ClientError) -> Self {
        match err {
            ClientError::DaemonNotRunning { .. } => Self {
                hint: Some("Start the daemon with `redd`.".to_owned()),
                ..Self::new("daemon_not_running", err.to_string())
            },
            ClientError::Api { status, error } => Self {
                code: serde_json::to_value(error.code)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "internal".to_owned()),
                message: error.message,
                hint: error.hint,
                status: Some(status),
            },
            other => Self::new("internal", other.to_string()),
        }
    }
}

/// Forward one Control API request to redd and return its JSON body.
#[tauri::command]
pub async fn api(
    state: tauri::State<'_, ApiState>,
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, CommandError> {
    forward(&state, &method, &path, body).await
}

async fn forward(
    state: &ApiState,
    method: &str,
    path: &str,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, CommandError> {
    let client = state
        .0
        .as_ref()
        .map_err(|err| CommandError::new("internal", err.clone()))?;
    if !path.starts_with("/v") {
        return Err(CommandError::new(
            "bad_request",
            format!("not a Control API path: {path}"),
        ));
    }
    let method = Method::from_bytes(method.as_bytes())
        .map_err(|_| CommandError::new("bad_request", format!("invalid method {method}")))?;
    Ok(client.request(method, path, body).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_daemon_becomes_daemon_not_running() {
        let home = tempfile::tempdir().unwrap();
        let state = ApiState::with_client(Client::new(home.path().join("redd.sock")));
        let err = forward(&state, "GET", "/v1/status", None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "daemon_not_running");
        assert!(err.hint.is_some());
    }

    #[tokio::test]
    async fn rejects_non_api_paths_and_bad_methods() {
        let home = tempfile::tempdir().unwrap();
        let state = ApiState::with_client(Client::new(home.path().join("redd.sock")));
        let err = forward(&state, "GET", "http://evil", None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "bad_request");
        let err = forward(&state, "G ET", "/v1/status", None)
            .await
            .unwrap_err();
        assert_eq!(err.code, "bad_request");
    }
}
