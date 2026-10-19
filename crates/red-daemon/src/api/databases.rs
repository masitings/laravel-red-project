//! `/v1/services/{name}/databases`: databases in MySQL, MariaDB and
//! PostgreSQL services.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use red_api::{BackupInfo, DatabaseRequest, DatabasesResponse, RestoreRequest};

use super::{ApiResponse, ApiResult, App, bad_request, internal};
use crate::AppState;
use crate::databases::{self, Tools};

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/services/{name}/databases", get(list).post(create))
        .route("/v1/services/{name}/databases/{db}", delete(drop_db))
        .route("/v1/services/{name}/databases/{db}/backup", post(backup))
        .route("/v1/services/{name}/databases/{db}/restore", post(restore))
}

fn backup_dir(app: &AppState, service: &str) -> PathBuf {
    app.paths.data_dir.join("backups").join(service)
}

/// The service's tools, with the service running.
async fn tools(app: &AppState, service: &str) -> Result<Tools, ApiResponse> {
    let config = app
        .state_snapshot()
        .services
        .get(service)
        .cloned()
        .ok_or_else(|| {
            ApiResponse::from(
                red_api::ApiError::new(
                    red_api::ErrorCode::NotFound,
                    format!("no service named {service:?}"),
                )
                .with_hint("list services with `red services`"),
            )
        })?;
    let tools = Tools::for_service(&config).map_err(bad_request)?;
    crate::projects::ensure_running(app, service)
        .await
        .map_err(|e| ApiResponse::from(bad_request(e)))?;
    Ok(tools)
}

/// Run blocking client work off the async threads.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, ApiResponse> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(internal)?
        .map_err(|e| ApiResponse::from(bad_request(e)))
}

async fn response(app: &AppState, service: &str, tools: Tools) -> ApiResult<DatabasesResponse> {
    let databases = blocking(move || tools.list()).await?;
    Ok(Json(DatabasesResponse {
        service: service.to_owned(),
        databases,
        backups: databases::backups(&backup_dir(app, service)),
    }))
}

async fn list(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<DatabasesResponse> {
    let tools = tools(&app, &name).await?;
    response(&app, &name, tools).await
}

async fn create(
    State(app): App,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<DatabaseRequest>,
) -> ApiResult<DatabasesResponse> {
    let tools = tools(&app, &name).await?;
    let db = body.name.trim().to_owned();
    let worker = tools.clone();
    blocking(move || worker.create(&db)).await?;
    response(&app, &name, tools).await
}

async fn drop_db(
    State(app): App,
    UrlPath((name, db)): UrlPath<(String, String)>,
) -> ApiResult<DatabasesResponse> {
    let tools = tools(&app, &name).await?;
    let worker = tools.clone();
    blocking(move || worker.drop(&db)).await?;
    response(&app, &name, tools).await
}

async fn backup(
    State(app): App,
    UrlPath((name, db)): UrlPath<(String, String)>,
) -> Result<(StatusCode, Json<BackupInfo>), ApiResponse> {
    let tools = tools(&app, &name).await?;
    let dir = backup_dir(&app, &name);
    let info = blocking(move || tools.backup(&db, &dir)).await?;
    tracing::info!(file = %info.file, "database backed up");
    Ok((StatusCode::CREATED, Json(info)))
}

async fn restore(
    State(app): App,
    UrlPath((name, db)): UrlPath<(String, String)>,
    Json(body): Json<RestoreRequest>,
) -> ApiResult<DatabasesResponse> {
    let file = PathBuf::from(&body.file);
    if !file.is_absolute() || !file.is_file() {
        return Err(bad_request(format!("{} is not a file", body.file)).into());
    }
    let tools = tools(&app, &name).await?;
    let worker = tools.clone();
    let create = body.create;
    blocking(move || worker.restore(&db, &file, create)).await?;
    response(&app, &name, tools).await
}
