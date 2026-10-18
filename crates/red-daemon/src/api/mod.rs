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
    PhpVersionRequest, ProxyInfo, ProxyRequest, SiteInfo, StatusPaths, StatusResponse, endpoints,
};
use red_core::{Site, SiteKind, sites};

use crate::AppState;

mod dumps;
mod logs;
mod mail;
mod runtimes;
mod services;

pub(crate) type App = State<Arc<AppState>>;
pub(crate) type ApiResult<T> = Result<Json<T>, ApiResponse>;

/// Build the Control API router.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(endpoints::STATUS, get(status))
        .route(endpoints::EVENTS, get(events))
        .route(endpoints::DOCTOR, get(doctor))
        .route(
            endpoints::DNS,
            axum::routing::post(install_dns).delete(remove_dns),
        )
        .route(endpoints::SITES, get(list_sites))
        .route("/v1/sites/{name}", get(get_site))
        .route("/v1/sites/{name}/php", put(isolate).delete(unisolate))
        .route("/v1/sites/{name}/secure", put(secure).delete(unsecure))
        .route(endpoints::PATHS, get(list_paths).post(park).delete(forget))
        .route(endpoints::LINKS, axum::routing::post(link))
        .route("/v1/links/{name}", delete(unlink))
        .route(endpoints::PROXIES, get(list_proxies).post(add_proxy))
        .route("/v1/proxies/{name}", delete(remove_proxy))
        .route(
            "/v1/proxies/{name}/secure",
            put(secure_proxy).delete(unsecure_proxy),
        )
        .route(endpoints::PHP, get(list_php))
        .route(endpoints::PHP_DEFAULT, put(set_default_php))
        .merge(services::routes())
        .merge(mail::routes())
        .merge(dumps::routes())
        .merge(logs::routes())
        .merge(runtimes::routes())
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
        helper: app.via_helper.load(std::sync::atomic::Ordering::Relaxed),
        smtp_port: app.smtp_port.get().copied(),
        dump_port: app.dump_port.get().copied(),
        paths: StatusPaths {
            config_dir: paths.config_dir.display().to_string(),
            data_dir: paths.data_dir.display().to_string(),
            socket: paths.socket().display().to_string(),
            ca_cert: app.tls.ca_cert_path().display().to_string(),
        },
    })
}

async fn doctor(State(app): App) -> Json<red_api::DoctorReport> {
    Json(crate::doctor::run(&app).await)
}

// ---- DNS (through the helper) ---------------------------------------------------

async fn install_dns(State(app): App) -> Result<StatusCode, ApiResponse> {
    let port = app.dns_port.get().copied().ok_or_else(|| {
        ApiResponse::from(ApiError::new(
            ErrorCode::Unavailable,
            "the DNS responder is not running",
        ))
    })?;
    helper_call(
        &app,
        red_helper::protocol::Request::InstallResolver {
            tld: app.config.tld.clone(),
            port,
        },
    )
    .await
}

async fn remove_dns(State(app): App) -> Result<StatusCode, ApiResponse> {
    helper_call(
        &app,
        red_helper::protocol::Request::RemoveResolver {
            tld: app.config.tld.clone(),
        },
    )
    .await
}

async fn helper_call(
    app: &AppState,
    request: red_helper::protocol::Request,
) -> Result<StatusCode, ApiResponse> {
    use red_helper::client::HelperError;
    match red_helper::client::call(&app.helper_socket, &request).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(HelperError::Unavailable(err)) => Err(ApiError::new(
            ErrorCode::Unavailable,
            format!("red-helper is not available: {err}"),
        )
        .with_hint("install it with `red helper:install`")
        .into()),
        Err(err) => Err(internal(err)),
    }
}

// ---- events ------------------------------------------------------------------

async fn events(
    State(app): App,
) -> axum::response::Sse<
    impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    use futures_util::StreamExt;
    use tokio_stream::wrappers::BroadcastStream;

    let to_sse = |event: red_api::Event| {
        let data = serde_json::to_string(&event).expect("events serialise");
        Ok(axum::response::sse::Event::default().data(data))
    };
    let first = futures_util::stream::once(async move { to_sse(red_api::Event::Connected) });
    // A lagging subscriber skips missed events; the next one makes it refetch.
    let rest = BroadcastStream::new(app.events.subscribe())
        .filter_map(move |event| async move { event.ok().map(to_sse) });
    axum::response::Sse::new(first.chain(rest))
        .keep_alive(axum::response::sse::KeepAlive::default())
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
    app.notify(red_api::Event::SitesChanged);
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn unisolate(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    app.update_state(|s| s.isolated.remove(&site.name))
        .map_err(internal)?;
    app.notify(red_api::Event::SitesChanged);
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn secure(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    let expires = app.tls.issue(&site.name).map_err(internal)?;
    app.update_state(|s| s.secured.insert(site.name.clone(), expires))
        .map_err(internal)?;
    app.notify(red_api::Event::SitesChanged);
    Ok(Json(site_info(&app, &app.state_snapshot(), &site)))
}

async fn unsecure(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<SiteInfo> {
    let state = app.state_snapshot();
    let site = sites::find(&state, &name).ok_or_else(|| unknown_site(&name))?;
    app.update_state(|s| s.secured.remove(&site.name))
        .map_err(internal)?;
    app.tls.remove(&site.name);
    app.notify(red_api::Event::SitesChanged);
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
        php: app.effective_php(state, site),
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
    app.notify(red_api::Event::SitesChanged);
    Ok(Json(display_paths(&app.state_snapshot().parked)))
}

async fn forget(State(app): App, Json(body): Json<PathRequest>) -> ApiResult<Vec<String>> {
    let dir = PathBuf::from(&body.path);
    let removed = app.update_state(|s| s.forget(&dir)).map_err(internal)?;
    app.notify(red_api::Event::SitesChanged);
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
    app.notify(red_api::Event::SitesChanged);
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
    app.notify(red_api::Event::SitesChanged);
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

// ---- proxies -----------------------------------------------------------------

fn proxy_info(app: &AppState, state: &red_core::State, name: &str, target: &str) -> ProxyInfo {
    ProxyInfo {
        name: name.to_owned(),
        target: target.to_owned(),
        url: app.site_url(state, name),
        secure: state.secured.contains_key(name),
        cert_expires: state.secured.get(name).copied(),
    }
}

fn proxy_list(app: &AppState) -> Vec<ProxyInfo> {
    let state = app.state_snapshot();
    state
        .proxies
        .iter()
        .map(|(name, target)| proxy_info(app, &state, name, target))
        .collect()
}

async fn list_proxies(State(app): App) -> Json<Vec<ProxyInfo>> {
    Json(proxy_list(&app))
}

async fn add_proxy(State(app): App, Json(body): Json<ProxyRequest>) -> ApiResult<ProxyInfo> {
    let name = sites::normalize_name(&body.name).ok_or_else(|| {
        bad_request(format!("{:?} is not a valid name", body.name))
            .with_hint("use letters, digits, '.' and '-'")
    })?;
    let target = sites::normalize_proxy_target(&body.target).ok_or_else(|| {
        bad_request(format!("{:?} is not a valid proxy target", body.target))
            .with_hint("use a port (5173), host:port, or an http:// URL")
    })?;
    app.update_state(|s| s.proxies.insert(name.clone(), target.clone()))
        .map_err(internal)?;
    app.notify(red_api::Event::ProxiesChanged);
    Ok(Json(proxy_info(
        &app,
        &app.state_snapshot(),
        &name,
        &target,
    )))
}

async fn remove_proxy(
    State(app): App,
    UrlPath(name): UrlPath<String>,
) -> Result<StatusCode, ApiResponse> {
    let removed = app
        .update_state(|s| {
            s.secured.remove(&name);
            s.proxies.remove(&name).is_some()
        })
        .map_err(internal)?;
    if !removed {
        return Err(unknown_proxy(&name));
    }
    app.tls.remove(&name);
    app.notify(red_api::Event::ProxiesChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn secure_proxy(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<ProxyInfo> {
    let target = app
        .state_snapshot()
        .proxies
        .get(&name)
        .cloned()
        .ok_or_else(|| unknown_proxy(&name))?;
    let expires = app.tls.issue(&name).map_err(internal)?;
    app.update_state(|s| s.secured.insert(name.clone(), expires))
        .map_err(internal)?;
    app.notify(red_api::Event::ProxiesChanged);
    Ok(Json(proxy_info(
        &app,
        &app.state_snapshot(),
        &name,
        &target,
    )))
}

async fn unsecure_proxy(State(app): App, UrlPath(name): UrlPath<String>) -> ApiResult<ProxyInfo> {
    let target = app
        .state_snapshot()
        .proxies
        .get(&name)
        .cloned()
        .ok_or_else(|| unknown_proxy(&name))?;
    app.update_state(|s| s.secured.remove(&name))
        .map_err(internal)?;
    app.tls.remove(&name);
    app.notify(red_api::Event::ProxiesChanged);
    Ok(Json(proxy_info(
        &app,
        &app.state_snapshot(),
        &name,
        &target,
    )))
}

fn unknown_proxy(name: &str) -> ApiResponse {
    ApiResponse(
        StatusCode::NOT_FOUND,
        ApiError::new(ErrorCode::NotFound, format!("no proxy named {name:?}"))
            .with_hint("list proxies with `red proxies`"),
    )
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
    app.notify(red_api::Event::PhpChanged);
    app.notify(red_api::Event::SitesChanged);
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
            ErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        Self(status, error)
    }
}

pub(crate) fn bad_request(message: String) -> ApiError {
    ApiError::new(ErrorCode::BadRequest, message)
}

fn unknown_site(name: &str) -> ApiResponse {
    ApiResponse(
        StatusCode::NOT_FOUND,
        ApiError::new(ErrorCode::NotFound, format!("no site named {name:?}"))
            .with_hint("list sites with `red sites`"),
    )
}

pub(crate) fn internal(err: impl std::fmt::Display) -> ApiResponse {
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
