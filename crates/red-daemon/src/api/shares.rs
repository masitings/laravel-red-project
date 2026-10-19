//! `/v1/shares`: share sites on the internet through Cloudflare Tunnel.

use std::sync::Arc;

use axum::extract::{Path as UrlPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use red_api::{
    ApiError, ErrorCode, ShareInfo, ShareLoginResponse, ShareRequest, SharesResponse, endpoints,
};

use super::{ApiResponse, ApiResult, App, bad_request};
use crate::AppState;
use crate::shares;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::SHARES, get(list))
        .route(endpoints::SHARE_LOGIN, post(login))
        .route("/v1/shares/{site}", post(start).delete(stop))
}

async fn list(State(app): App) -> Json<SharesResponse> {
    Json(SharesResponse {
        shares: app.shares.list(),
        logged_in: shares::logged_in(),
        cloudflared: shares::cloudflared(&app).map(|p| p.display().to_string()),
    })
}

async fn start(
    State(app): App,
    UrlPath(site): UrlPath<String>,
    body: Option<Json<ShareRequest>>,
) -> Result<(StatusCode, Json<ShareInfo>), ApiResponse> {
    let request = body.map(|Json(b)| b).unwrap_or_default();
    let state = app.state_snapshot();
    if red_core::sites::find(&state, &site).is_none() {
        return Err(
            ApiError::new(ErrorCode::NotFound, format!("no site named {site:?}"))
                .with_hint("list sites with `red sites`")
                .into(),
        );
    }
    let hostname = match request.hostname.map(|h| h.trim().to_ascii_lowercase()) {
        Some(h) if h.is_empty() => None,
        Some(h) if !shares::valid_hostname(&h) => {
            return Err(bad_request(format!("{h:?} is not a hostname"))
                .with_hint(
                    "use a name on a domain in your Cloudflare account, e.g. app.example.com",
                )
                .into());
        }
        Some(h) => {
            if !shares::logged_in() {
                return Err(ApiError::new(
                    ErrorCode::Conflict,
                    "cloudflared is not logged in to Cloudflare".to_owned(),
                )
                .with_hint(
                    "log in once with `red share:login` (or Share → Use my domain in the app)",
                )
                .into());
            }
            Some(h)
        }
        None => None,
    };
    let info = shares::start(app.clone(), site, hostname);
    Ok((StatusCode::ACCEPTED, Json(info)))
}

async fn stop(State(app): App, UrlPath(site): UrlPath<String>) -> Result<StatusCode, ApiResponse> {
    if app.shares.stop(&site) {
        app.notify(red_api::Event::SharesChanged);
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::new(ErrorCode::NotFound, format!("{site} is not shared")).into())
    }
}

async fn login(State(app): App) -> ApiResult<ShareLoginResponse> {
    let url = shares::login(&app).await.map_err(bad_request)?;
    Ok(Json(ShareLoginResponse {
        logged_in: shares::logged_in(),
        url,
    }))
}
