//! `/v1/projects`: create new Laravel, WordPress or empty projects.

use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use red_api::{ProjectJob, ProjectRequest, endpoints};

use super::{ApiResponse, App, bad_request};
use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new().route(endpoints::PROJECTS, get(list).post(create))
}

async fn list(State(app): App) -> Json<Vec<ProjectJob>> {
    Json(app.projects.list())
}

async fn create(
    State(app): App,
    Json(body): Json<ProjectRequest>,
) -> Result<(StatusCode, Json<ProjectJob>), ApiResponse> {
    crate::projects::start(app.clone(), body)
        .map(|job| (StatusCode::ACCEPTED, Json(job)))
        .map_err(|message| bad_request(message).into())
}
