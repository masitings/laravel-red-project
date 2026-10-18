//! Control API routes.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, put};
use axum::{Json, Router};
use red_api::{
    API_VERSION, ApiError, ErrorCode, PathRequest, PhpInstallInfo, PhpListResponse,
    PhpVersionRequest, SiteInfo, StatusPaths, StatusResponse, endpoints,
};
use red_core::{Site, SiteKind, sites};

use crate::AppState;

type App = State<Arc<AppState>>;
type ApiResult<T> = Result<Json<T>, ApiResponse>;

/// Build the Control API router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(endpoints::STATUS, get(status))
        .route(endpoints::SITES, get(list_sites))
        .route("/v1/sites/{name}", get(get_site))
        .route("/v1/sites/{name}/php", put(isolate).delete(unisolate))
        .route("/v1/sites/{name}/secure", put(secure).delete(unsecure))
        .route(endpoints::PATHS, get(list_paths).post(park).delete(forget))
        .route(endpoints::LINKS, axum::routing::post(link))
        .route("/v1/links/{name}", delete(unlink))
        .route(endpoints::PHP, get(list_php))
        .route(endpoints::PHP_DEFAULT, put(set_default_php))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .with_state(state)
}

async fn status(State(app): App) -> Json<StatusResponse> {
    let paths = &app.paths;
    Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        api_version: API_VERSION,
        os: red_platform::os_name().to_owned(),
        pid: std::process::id(),
        uptime_secs: app.started.elapsed().as_secs(),
        tld: app.config.tld.clone(),
        http_port: app.http_port.get().copied(),
        https_port: app.https_port.get().copied(),
        dns_port: app.dns_port.get().copied(),
        paths: StatusPaths {
            config_dir: paths.config_dir.display().to_string(),
            data_dir: paths.data_dir.display().to_string(),
            socket: paths.socket().display().to_string(),
            ca_cert: app.tls.ca_cert_path().display().to_string(),
        },
    })
}

// ---- sites -----------------------------------------------------------------

async fn list_sites(State(app): App) -> Json<Vec<SiteInfo>> {
    let state = app.state_snapshot();
    Json(
        sites::list(&state)
            .iter()
            .map(|site| site_info(&app, &state, site))
            .collect(),
    )
}

async fn get_site(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    Ok(Json(site_info(&app, &state, &site)))
}

async fn isolate(
    State(app): App,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<PhpVersionRequest>,
) -> ApiResult<SiteInfo> {
    let version = installed_version(&app, &body.version).await?;
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    app.update_state(|s| s.isolated.insert(site.name.clone(), version))
        .map_err(internal)?;
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn unisolate(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    app.update_state(|s| s.isolated.remove(&site.name))
        .map_err(internal)?;
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn secure(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    let expires = app.tls.issue(&site.name).map_err(internal)?;
    app.update_state(|s| s.secured.insert(site.name.clone(), expires))
        .map_err(internal)?;
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn unsecure(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    app.update_state(|s| s.secured.remove(&site.name))
        .map_err(internal)?;
    app.tls.remove(&site.name);
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

fn site_info(app: &AppState, state: &red_core::State, site: &Site) -> SiteInfo {
    SiteInfo {
        name: site.name.clone(),
        path: site.path.display().to_string(),
        url: app.site_url(state, &site.name),
        kind: match site.kind {
            SiteKind::Parked => red_api::SiteKind::Parked,
            SiteKind::Linked => red_api::SiteKind::Linked,
        },
        driver: red_core::driver::detect(&site.path).name.to_owned(),
        php: app.effective_php(state, &site.name),
        isolated: state.isolated.contains_key(&site.name),
        secure: state.secured.contains_key(&site.name),
        cert_expires: state.secured.get(&site.name).copied(),
    }
}

// ---- parked paths and links -------------------------------------------------

async fn list_paths(State(app): App) -> Json<Vec<String>> {
    Json(display_paths(&app.state_snapshot().parked))
}

async fn park(State(app): App, Json(body): Json<PathRequest>) -> ApiResult<Vec<String>> {
    let dir = existing_dir(&body.path)?;
    app.update_state(|s| s.park(dir)).map_err(internal)?;
    Ok(Json(display_paths(&app.state_snapshot().parked)))
}

async fn forget(State(app): App, Json(body): Json<PathRequest>) -> ApiResult<Vec<String>> {
    let dir = PathBuf::from(&body.path);
    let removed = app.update_state(|s| s.forget(&dir)).map_err(internal)?;
    if !removed {
        return Err(ApiResponse(
            StatusCode::NOT_FOUND,
            ApiError::new(ErrorCode::NotFound, format!("{} is not parked", body.path))
                .with_hint("list parked paths with `red paths`"),
        ));
    }
    Ok(Json(display_paths(&app.state_snapshot().parked)))
}

async fn link(State(app): App, Json(body): Json<PathRequest>) -> ApiResult<SiteInfo> {
    let dir = existing_dir(&body.path)?;
    let requested = body.name.clone().unwrap_or_else(|| {
        dir.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let name = sites::normalize_name(&requested).ok_or_else(|| {
        bad_request(format!("{requested:?} is not a valid site name"))
            .with_hint("use letters, digits, '.' and '-'")
    })?;
    app.update_state(|s| s.link(name.clone(), dir.clone()))
        .map_err(internal)?;
    let site = Site {
        name,
        path: dir,
        kind: SiteKind::Linked,
    };
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn unlink(
    State(app): App,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiResponse> {
    let removed = app.update_state(|s| s.unlink(&name)).map_err(internal)?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiResponse(
            StatusCode::NOT_FOUND,
            ApiError::new(ErrorCode::NotFound, format!("no link named {name:?}"))
                .with_hint("list sites with `red sites`"),
        ))
    }
}

fn existing_dir(path: &str) -> Result<PathBuf, ApiResponse> {
    let dir = PathBuf::from(path);
    if !dir.is_absolute() {
        return Err(bad_request(format!("{path} is not an absolute path")).into());
    }
    if !dir.is_dir() {
        return Err(bad_request(format!("{path} is not a directory")).into());
    }
    Ok(dir)
}

fn display_paths(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|p| p.display().to_string()).collect()
}

// ---- PHP ---------------------------------------------------------------------

async fn list_php(State(app): App) -> Json<PhpListResponse> {
    let installs = app.php.discover().await;
    let running = app.php.running().await;
    let state = app.state_snapshot();
    Json(PhpListResponse {
        default: state.default_php.clone().or_else(|| app.php.newest()),
        installs: installs
            .into_iter()
            .map(|i| PhpInstallInfo {
                running: running.contains(&i.version),
                version: i.version,
                full_version: i.full_version,
                fpm_path: i.fpm.display().to_string(),
            })
            .collect(),
    })
}

async fn set_default_php(
    State(app): App,
    Json(body): Json<PhpVersionRequest>,
) -> ApiResult<PhpListResponse> {
    let version = installed_version(&app, &body.version).await?;
    app.update_state(|s| s.default_php = Some(version))
        .map_err(internal)?;
    Ok(list_php(State(app)).await)
}

/// Normalise `requested` to a minor version and check it is installed.
async fn installed_version(app: &AppState, requested: &str) -> Result<String, ApiResponse> {
    let version = red_core::php::minor_version(requested)
        .ok_or_else(|| bad_request(format!("{requested:?} is not a PHP version like 8.3")))?;
    if app.php.find(&version).is_none() {
        // The list may be stale: PHP could have been installed after redd started.
        app.php.discover().await;
    }
    if app.php.find(&version).is_some() {
        return Ok(version);
    }
    let installed: Vec<String> = app.php.installs().into_iter().map(|i| i.version).collect();
    let hint = if installed.is_empty() {
        "no PHP installation found; install one, e.g. `brew install php`".to_owned()
    } else {
        format!("installed: {}", installed.join(", "))
    };
    Err(bad_request(format!("PHP {version} is not installed"))
        .with_hint(hint)
        .into())
}

// ---- errors ------------------------------------------------------------------

/// Turns an [`ApiError`] into a JSON response with the given status.
#[derive(Debug)]
pub struct ApiResponse(pub StatusCode, pub ApiError);

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        (self.0, Json(self.1)).into_response()
    }
}

impl From<ApiError> for ApiResponse {
    fn from(error: ApiError) -> Self {
        let status = match error.code {
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self(status, error)
    }
}

fn bad_request(message: String) -> ApiError {
    ApiError::new(ErrorCode::BadRequest, message)
}

fn unknown_site(name: &str) -> ApiResponse {
    ApiResponse(
        StatusCode::NOT_FOUND,
        ApiError::new(ErrorCode::NotFound, format!("no site named {name:?}"))
            .with_hint("list sites with `red sites`"),
    )
}

fn internal(err: impl std::fmt::Display) -> ApiResponse {
    tracing::error!(%err, "request failed");
    ApiResponse(
        StatusCode::INTERNAL_SERVER_ERROR,
        ApiError::new(ErrorCode::Internal, err.to_string()),
    )
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
