//! `/v1/services`: database and cache services.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use red_api::{
    ApiError, EnvVar, ErrorCode, ServerInstall, ServiceInfo, ServiceRequest, ServiceUpdate,
    ServicesResponse, endpoints,
};
use red_core::{ServiceConfig, ServiceKind};
use serde::Deserialize;

use super::{ApiResponse, ApiResult, App, bad_request, internal};
use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::SERVICES, get(list).post(add))
        .route("/v1/services/{name}", get(show).put(update).delete(remove))
        .route("/v1/services/{name}/{action}", post(action))
}

pub(crate) fn api_kind(kind: ServiceKind) -> red_api::ServiceKind {
    match kind {
        ServiceKind::Mysql => red_api::ServiceKind::Mysql,
        ServiceKind::Postgres => red_api::ServiceKind::Postgres,
        ServiceKind::Redis => red_api::ServiceKind::Redis,
    }
}

fn core_kind(kind: red_api::ServiceKind) -> ServiceKind {
    match kind {
        red_api::ServiceKind::Mysql => ServiceKind::Mysql,
        red_api::ServiceKind::Postgres => ServiceKind::Postgres,
        red_api::ServiceKind::Redis => ServiceKind::Redis,
    }
}

pub(crate) fn service_info(app: &AppState, name: &str, config: &ServiceConfig) -> ServiceInfo {
    let runtime = app.services.runtime(name);
    ServiceInfo {
        name: name.to_owned(),
        kind: api_kind(config.kind),
        flavor: runtime.flavor.clone(),
        version: config.version.clone(),
        binary: config.binary.display().to_string(),
        port: config.port,
        autostart: config.autostart,
        status: runtime.status,
        pid: runtime.pid,
        error: runtime.error,
        data_dir: app
            .paths
            .service_dir(name)
            .join("data")
            .display()
            .to_string(),
        log_file: app.paths.service_log(name).display().to_string(),
        env: config
            .kind
            .laravel_env(config.port)
            .into_iter()
            .map(|(name, value)| EnvVar {
                name: name.to_owned(),
                value,
            })
            .collect(),
    }
}

fn install_info(install: &crate::services::ServerInstall) -> ServerInstall {
    ServerInstall {
        kind: api_kind(install.server.kind),
        flavor: install.server.flavor.clone(),
        version: install.server.version.clone(),
        full_version: install.server.full_version.clone(),
        binary: install.binary.display().to_string(),
    }
}

async fn list(State(app): App) -> Json<ServicesResponse> {
    let installs = app.services.discover().await;
    Json(response(&app, &installs))
}

fn response(app: &AppState, installs: &[crate::services::ServerInstall]) -> ServicesResponse {
    let state = app.state_snapshot();
    ServicesResponse {
        services: state
            .services
            .iter()
            .map(|(name, config)| service_info(app, name, config))
            .collect(),
        available: installs.iter().map(install_info).collect(),
    }
}

async fn show(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<ServiceInfo> {
    let config = config_of(&app, &name)?;
    Ok(Json(service_info(&app, &name, &config)))
}

async fn add(State(app): App, Json(body): Json<ServiceRequest>) -> ApiResult<ServiceInfo> {
    let kind = core_kind(body.kind);
    let requested = body
        .name
        .clone()
        .unwrap_or_else(|| kind.as_str().to_owned());
    let name = red_core::sites::normalize_name(&requested)
        .filter(|n| !n.contains('.'))
        .ok_or_else(|| {
            bad_request(format!("{requested:?} is not a valid service name"))
                .with_hint("use letters, digits and '-'")
        })?;
    let state = app.state_snapshot();
    if state.services.contains_key(&name) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!("a service named {name:?} already exists"),
        )
        .with_hint("pass another name, e.g. `--name postgres-15`")
        .into());
    }
    let port = body.port.unwrap_or_else(|| kind.default_port());
    if port == 0 {
        return Err(bad_request("port 0 is not allowed".into()).into());
    }
    if let Some((other, _)) = state.services.iter().find(|(_, c)| c.port == port) {
        return Err(ApiError::new(
            ErrorCode::Conflict,
            format!("service {other:?} already uses port {port}"),
        )
        .with_hint("pass another port with `--port`")
        .into());
    }

    let (binary, version) = match &body.binary {
        Some(binary) => {
            let binary = PathBuf::from(binary);
            let found = crate::services::probe(kind, &binary).await.ok_or_else(|| {
                bad_request(format!("{} is not a {kind} server", binary.display()))
            })?;
            (binary, found.version)
        }
        None => {
            let installs = app.services.discover().await;
            let wanted = body.version.as_deref().map(str::trim);
            let install = installs
                .iter()
                .filter(|i| i.server.kind == kind)
                .find(|i| {
                    wanted.is_none_or(|w| {
                        i.server.full_version == w
                            || i.server.full_version.starts_with(&format!("{w}."))
                            || i.server.flavor == w
                    })
                })
                .ok_or_else(|| missing_server(kind, wanted, &installs))?;
            (install.binary.clone(), install.server.version.clone())
        }
    };

    let config = ServiceConfig {
        kind,
        binary,
        version: Some(version),
        port,
        autostart: body.autostart.unwrap_or(true),
    };
    app.update_state(|s| s.services.insert(name.clone(), config.clone()))
        .map_err(internal)?;
    app.services.start(&name, &config).await;
    app.notify(red_api::Event::ServicesChanged);
    Ok(Json(service_info(&app, &name, &config)))
}

fn missing_server(
    kind: ServiceKind,
    wanted: Option<&str>,
    installs: &[crate::services::ServerInstall],
) -> ApiResponse {
    let available: Vec<String> = installs
        .iter()
        .filter(|i| i.server.kind == kind)
        .map(|i| format!("{} {}", i.server.flavor, i.server.full_version))
        .collect();
    let message = match wanted {
        Some(w) => format!("no {kind} server matching {w:?} was found"),
        None => format!("no {kind} server was found"),
    };
    let hint = if available.is_empty() {
        match kind {
            ServiceKind::Mysql => {
                "install one, e.g. `brew install mysql` or `brew install mariadb`"
            }
            ServiceKind::Postgres => "install one, e.g. `brew install postgresql@17`",
            ServiceKind::Redis => "install one, e.g. `brew install valkey` or `brew install redis`",
        }
        .to_owned()
    } else {
        format!("installed: {}", available.join(", "))
    };
    ApiError::new(ErrorCode::NotFound, message)
        .with_hint(hint)
        .into()
}

async fn update(
    State(app): App,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<ServiceUpdate>,
) -> ApiResult<ServiceInfo> {
    let mut config = config_of(&app, &name)?;
    let mut restart = false;
    if let Some(port) = body.port
        && port != config.port
    {
        if port == 0 {
            return Err(bad_request("port 0 is not allowed".into()).into());
        }
        if let Some((other, _)) = app
            .state_snapshot()
            .services
            .iter()
            .find(|(n, c)| c.port == port && **n != name)
        {
            return Err(ApiError::new(
                ErrorCode::Conflict,
                format!("service {other:?} already uses port {port}"),
            )
            .into());
        }
        config.port = port;
        restart = true;
    }
    if let Some(autostart) = body.autostart {
        config.autostart = autostart;
    }
    app.update_state(|s| s.services.insert(name.clone(), config.clone()))
        .map_err(internal)?;
    let status = app.services.runtime(&name).status;
    if restart && status != red_api::ServiceStatus::Stopped {
        app.services.stop(&name).await;
        app.services.start(&name, &config).await;
    }
    app.notify(red_api::Event::ServicesChanged);
    Ok(Json(service_info(&app, &name, &config)))
}

#[derive(Debug, Deserialize)]
struct RemoveQuery {
    #[serde(default)]
    delete_data: bool,
}

async fn remove(
    State(app): App,
    UrlPath(name): UrlPath<String>,
    Query(query): Query<RemoveQuery>,
) -> Result<StatusCode, ApiResponse> {
    config_of(&app, &name)?;
    app.services.stop(&name).await;
    app.update_state(|s| s.services.remove(&name))
        .map_err(internal)?;
    app.services.forget(&name);
    if query.delete_data {
        let dir = app.paths.service_dir(&name);
        if let Err(err) = std::fs::remove_dir_all(&dir)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            return Err(internal(format!("cannot delete {}: {err}", dir.display())));
        }
    }
    app.notify(red_api::Event::ServicesChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn action(
    State(app): App,
    UrlPath((name, action)): UrlPath<(String, String)>,
) -> ApiResult<ServiceInfo> {
    let config = config_of(&app, &name)?;
    match action.as_str() {
        "start" => app.services.start(&name, &config).await,
        "stop" => app.services.stop(&name).await,
        "restart" => {
            app.services.stop(&name).await;
            app.services.start(&name, &config).await;
        }
        other => {
            return Err(bad_request(format!("unknown action {other:?}"))
                .with_hint("use start, stop or restart")
                .into());
        }
    }
    Ok(Json(service_info(&app, &name, &config)))
}

fn config_of(app: &AppState, name: &str) -> Result<ServiceConfig, ApiResponse> {
    app.state_snapshot()
        .services
        .get(name)
        .cloned()
        .ok_or_else(|| {
            ApiError::new(ErrorCode::NotFound, format!("no service named {name:?}"))
                .with_hint("list services with `red services`")
                .into()
        })
}
