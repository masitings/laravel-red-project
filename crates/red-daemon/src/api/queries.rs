//! `/v1/queries`: database queries Laravel sites ran.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, put};
use axum::{Json, Router};
use red_api::{QueriesResponse, QuerySettings, endpoints};

use super::{ApiResult, App, internal};
use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::QUERIES, get(list).delete(clear))
        .route(endpoints::QUERY_SETTINGS, put(settings))
}

fn response(app: &AppState) -> QueriesResponse {
    QueriesResponse {
        enabled: app.state_snapshot().capture_queries,
        queries: app.queries.list(),
    }
}

async fn list(State(app): App) -> Json<QueriesResponse> {
    Json(response(&app))
}

async fn clear(State(app): App) -> StatusCode {
    app.queries.clear();
    app.notify(red_api::Event::QueriesChanged);
    StatusCode::NO_CONTENT
}

/// Switch capture on or off. PHP-FPM pools pick it up on their next
/// request (their settings change, so they restart).
async fn settings(State(app): App, Json(body): Json<QuerySettings>) -> ApiResult<QueriesResponse> {
    if body.enabled
        && let Some(port) = app.dump_port.get()
    {
        app.queries.prepend_file(*port).map_err(internal)?;
    }
    app.update_state(|s| s.capture_queries = body.enabled)
        .map_err(internal)?;
    app.notify(red_api::Event::QueriesChanged);
    Ok(Json(response(&app)))
}
