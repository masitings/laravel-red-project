//! `/v1/dumps`: captured `dump()` output.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use red_api::{DumpSettings, DumpsResponse, endpoints};

use super::{ApiResult, App, internal};
use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::DUMPS, get(list).delete(clear))
        .route(endpoints::DUMP_SETTINGS, put(settings))
}

fn response(app: &AppState) -> DumpsResponse {
    DumpsResponse {
        enabled: app.dumps.enabled(),
        port: app.dump_port.get().copied(),
        dumps: app.dumps.list(),
    }
}

async fn list(State(app): App) -> Json<DumpsResponse> {
    Json(response(&app))
}

async fn clear(State(app): App) -> StatusCode {
    app.dumps.clear();
    app.notify(red_api::Event::DumpsChanged);
    StatusCode::NO_CONTENT
}

async fn settings(State(app): App, Json(body): Json<DumpSettings>) -> ApiResult<DumpsResponse> {
    app.update_state(|s| s.capture_dumps = body.enabled)
        .map_err(internal)?;
    app.dumps.set_enabled(body.enabled);
    app.notify(red_api::Event::DumpsChanged);
    Ok(Json(response(&app)))
}
