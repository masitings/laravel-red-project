//! Control API routes.

use std::sync::Arc;

use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use red_api::{API_VERSION, ApiError, ErrorCode, StatusPaths, StatusResponse, endpoints};

use crate::AppState;

/// Build the Control API router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(endpoints::STATUS, get(status))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
}

async fn status(State(state): State<Arc<AppState>>) -> Json<StatusResponse> {
    let paths = &state.paths;
    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        api_version: API_VERSION,
        os: red_platform::os_name().to_owned(),
        pid: std::process::id(),
        uptime_secs: state.started.elapsed().as_secs(),
        tld: state.config.tld.clone(),
        paths: StatusPaths {
            config_dir: paths.config_dir.display().to_string(),
            data_dir: paths.data_dir.display().to_string(),
            socket: paths.socket().display().to_string(),
        },
    })
}

/// Turns an [`ApiError`] into a JSON response with the given status.
#[derive(Debug)]
pub struct ApiResponse(pub StatusCode, pub ApiError);

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

async fn not_found(method: Method, uri: Uri) -> ApiResponse {
    ApiResponse(
        StatusCode::NOT_FOUND,
        ApiError::new(ErrorCode::NotFound, format!("no route for {method} {uri}")),
    )
}

async fn method_not_allowed(method: Method, uri: Uri) -> ApiResponse {
    ApiResponse(
        StatusCode::METHOD_NOT_ALLOWED,
        ApiError::new(
            ErrorCode::BadRequest,
            format!("{method} is not allowed on {uri}"),
        ),
    )
}
