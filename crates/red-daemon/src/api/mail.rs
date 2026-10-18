//! `/v1/mail`: the mail catcher.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use red_api::{ApiError, ErrorCode, MailMessage, MailSummary, endpoints};
use serde::Deserialize;

use super::{ApiResponse, ApiResult, App};
use crate::AppState;

pub(super) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(endpoints::MAIL, get(list).delete(clear))
        .route("/v1/mail/{id}", get(show).delete(remove))
        .route("/v1/mail/{id}/raw", get(raw))
        .route("/v1/mail/{id}/attachments/{index}", get(attachment))
}

#[derive(Debug, Deserialize)]
struct MailboxQuery {
    mailbox: Option<String>,
}

async fn list(State(app): App, Query(query): Query<MailboxQuery>) -> Json<Vec<MailSummary>> {
    Json(app.mail.list(query.mailbox.as_deref()))
}

async fn clear(State(app): App, Query(query): Query<MailboxQuery>) -> StatusCode {
    if app.mail.clear(query.mailbox.as_deref()) > 0 {
        app.notify(red_api::Event::MailChanged);
    }
    StatusCode::NO_CONTENT
}

async fn show(State(app): App, UrlPath(id): UrlPath<String>) -> ApiResult<MailMessage> {
    let (message, changed) = app.mail.message(&id).ok_or_else(|| unknown(&id))?;
    if changed {
        app.notify(red_api::Event::MailChanged);
    }
    Ok(Json(message))
}

async fn remove(State(app): App, UrlPath(id): UrlPath<String>) -> Result<StatusCode, ApiResponse> {
    if !app.mail.delete(&id) {
        return Err(unknown(&id));
    }
    app.notify(red_api::Event::MailChanged);
    Ok(StatusCode::NO_CONTENT)
}

async fn raw(State(app): App, UrlPath(id): UrlPath<String>) -> Result<Response, ApiResponse> {
    let bytes = app.mail.raw(&id).ok_or_else(|| unknown(&id))?;
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "message/rfc822")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{id}.eml\""),
        )
        .body(Body::from(bytes))
        .expect("valid response"))
}

async fn attachment(
    State(app): App,
    UrlPath((id, index)): UrlPath<(String, u32)>,
) -> Result<Response, ApiResponse> {
    let (name, content_type, bytes) = app.mail.attachment(&id, index).ok_or_else(|| {
        ApiResponse::from(ApiError::new(
            ErrorCode::NotFound,
            format!("message {id:?} has no attachment {index}"),
        ))
    })?;
    let name = name.replace('"', "'");
    Ok(Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{name}\""),
        )
        .body(Body::from(bytes))
        .expect("valid response"))
}

fn unknown(id: &str) -> ApiResponse {
    ApiError::new(ErrorCode::NotFound, format!("no message {id:?}"))
        .with_hint("list mail with `red mail`")
        .into()
}
