//! `/v1/runtimes`: PHP, MySQL and PostgreSQL builds RED downloads itself.

use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use axum::{Json, Router};
use red_api::{ApiError, ErrorCode, RuntimeJob, RuntimeRequest, RuntimesResponse, endpoints};

use super::{ApiResponse, ApiResult, App, bad_request, internal};
use crate::AppState;
use crate::runtimes::core_kind;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::RUNTIMES, get(list).post(install))
        .route("/v1/runtimes/{kind}/{line}", delete(remove))
}

async fn list(State(app): App) -> Json<RuntimesResponse> {
    let (available, catalog_errors) = app.runtimes.available().await;
    Json(RuntimesResponse {
        platform: app.runtimes.platform_name(),
        installed: app.runtimes.with_updates(app.runtimes.installed()).await,
        available,
        jobs: app.runtimes.jobs(),
        catalog_errors,
    })
}

async fn install(State(app): App, Json(body): Json<RuntimeRequest>) -> ApiResult<RuntimeJob> {
    crate::runtimes::install(app.clone(), core_kind(body.kind), body.version.trim())
        .await
        .map(Json)
        .map_err(|message| {
            ApiError::new(ErrorCode::NotFound, message)
                .with_hint("see what is available with `red runtimes`")
                .into()
        })
}

async fn remove(
    State(app): App,
    UrlPath((kind, line)): UrlPath<(String, String)>,
) -> Result<StatusCode, ApiResponse> {
    let kind = red_core::runtimes::RuntimeKind::parse(&kind)
        .ok_or_else(|| ApiResponse::from(bad_request(format!("unknown runtime {kind:?}"))))?;
    let dir = app
        .paths
        .data_dir
        .join("runtimes")
        .join(kind.as_str())
        .join(&line);
    if let Some((name, _)) = app
        .state_snapshot()
        .services
        .iter()
        .find(|(_, config)| config.binary.starts_with(&dir))
    {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!("the {name} service runs {kind} {line}"),
        )
        .with_hint(format!("remove it first with `red services:remove {name}`"))
        .into());
    }
    match app.runtimes.remove(kind, &line) {
        Ok(true) => {
            match kind {
                red_core::runtimes::RuntimeKind::Php => {
                    app.php.restart(&line).await;
                    app.php.discover().await;
                }
                _ => {
                    app.services.discover().await;
                }
            }
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(false) => Err(ApiError::new(
            ErrorCode::NotFound,
            format!("{kind} {line} was not installed by RED"),
        )
        .with_hint("list RED's runtimes with `red runtimes`")
        .into()),
        Err(err) => Err(internal(err)),
    }
}
