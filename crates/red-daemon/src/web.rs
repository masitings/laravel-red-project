//! The web server that serves sites.
//!
//! Each request is routed host → site → driver → action. Static files are
//! read from disk; PHP goes to the site's PHP-FPM pool over FastCGI. If a
//! pool does not answer, it is restarted once and the request retried.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{self, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use red_core::driver::{self, Action, PhpScript};
use red_core::{Site, sites};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::fastcgi::{self, FcgiError};

/// Largest request body passed to PHP (uploads included).
const MAX_BODY: usize = 512 * 1024 * 1024;

type Body = Full<Bytes>;

/// Bind the web server on loopback. Binds `127.0.0.1:port` and, when
/// available, `[::1]` on the same port. Returns the listeners and the port
/// actually used (relevant when `port` is 0).
pub async fn bind(port: u16) -> std::io::Result<(Vec<TcpListener>, u16)> {
    let v4 = TcpListener::bind(("127.0.0.1", port)).await?;
    let port = v4.local_addr()?.port();
    let mut listeners = vec![v4];
    match TcpListener::bind(("::1", port)).await {
        Ok(v6) => listeners.push(v6),
        Err(err) => tracing::debug!(%err, "not listening on [::1]"),
    }
    Ok((listeners, port))
}

/// Serve requests on every listener until `cancel` fires.
pub async fn serve(listeners: Vec<TcpListener>, app: Arc<AppState>, cancel: CancellationToken) {
    let mut tasks = Vec::new();
    for listener in listeners {
        let app = app.clone();
        let cancel = cancel.clone();
        tasks.push(tokio::spawn(async move {
            loop {
                let (stream, remote) = tokio::select! {
                    () = cancel.cancelled() => return,
                    accepted = listener.accept() => match accepted {
                        Ok(pair) => pair,
                        Err(err) => {
                            tracing::warn!(%err, "accept failed");
                            continue;
                        }
                    },
                };
                let local = stream.local_addr().unwrap_or(remote);
                let app = app.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let app = app.clone();
                        async move { Ok::<_, Infallible>(handle(req, app, remote, local).await) }
                    });
                    let result = hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .with_upgrades()
                        .await;
                    if let Err(err) = result {
                        tracing::debug!(%err, "connection closed with error");
                    }
                });
            }
        }));
    }
    for task in tasks {
        let _ = task.await;
    }
}

async fn handle(
    req: Request<Incoming>,
    app: Arc<AppState>,
    remote: SocketAddr,
    local: SocketAddr,
) -> Response<Body> {
    let host = host_of(&req).unwrap_or_default();
    let state = app.state_snapshot();
    let Some(site) = sites::resolve_host(&state, &host, &app.config.tld) else {
        return page(
            StatusCode::NOT_FOUND,
            "No site here",
            &format!(
                "RED has no site for <code>{}</code>. Park a directory with <code>red park</code> \
                 or link one with <code>red link</code>.",
                escape(&host)
            ),
        );
    };

    let driver = driver::detect(&site.path);
    let action = driver.resolve(&site.path, req.uri().path());
    tracing::debug!(%host, site = %site.name, driver = driver.name, ?action, "request");

    match action {
        Action::NotFound => page(
            StatusCode::NOT_FOUND,
            "Not found",
            "Nothing matches this URL.",
        ),
        Action::Redirect(location) => {
            let location = match req.uri().query() {
                Some(query) => format!("{location}?{query}"),
                None => location,
            };
            let mut response = Response::new(Body::default());
            *response.status_mut() = StatusCode::MOVED_PERMANENTLY;
            if let Ok(value) = HeaderValue::from_str(&location) {
                response.headers_mut().insert(header::LOCATION, value);
            }
            response
        }
        Action::Static(path) => serve_file(req.method(), &path).await,
        Action::Php(script) => {
            let version = app.effective_php(&state, &site.name);
            match version {
                Some(version) => run_php(req, &app, &site, &script, &version, remote, local).await,
                None => page(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "No PHP installed",
                    "RED could not find a <code>php-fpm</code> binary. Install PHP \
                     (for example <code>brew install php</code>) and check <code>red php:list</code>.",
                ),
            }
        }
    }
}

fn host_of(req: &Request<Incoming>) -> Option<String> {
    req.headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned)
        .or_else(|| req.uri().authority().map(|a| a.to_string()))
}

async fn serve_file(method: &Method, path: &std::path::Path) -> Response<Body> {
    if method != Method::GET && method != Method::HEAD {
        return page(
            StatusCode::METHOD_NOT_ALLOWED,
            "Method not allowed",
            "Static files only accept GET and HEAD.",
        );
    }
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let content_type = if mime.type_() == mime_guess::mime::TEXT
                || mime.subtype() == mime_guess::mime::JAVASCRIPT
                || mime.subtype() == mime_guess::mime::JSON
            {
                format!("{mime}; charset=utf-8")
            } else {
                mime.to_string()
            };
            let mut response = Response::new(Body::new(Bytes::from(bytes)));
            if let Ok(value) = HeaderValue::from_str(&content_type) {
                response.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            response
        }
        Err(err) => {
            tracing::warn!(path = %path.display(), %err, "could not read file");
            page(
                StatusCode::NOT_FOUND,
                "Not found",
                "The file could not be read.",
            )
        }
    }
}

async fn run_php(
    req: Request<Incoming>,
    app: &AppState,
    site: &Site,
    script: &PhpScript,
    version: &str,
    remote: SocketAddr,
    local: SocketAddr,
) -> Response<Body> {
    let (parts, body) = req.into_parts();
    let body = match Limited::new(body, MAX_BODY).collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return page(
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request too large",
                "The request body is too large.",
            );
        }
    };
    let params = cgi_params(&parts, script, &body, remote, local);

    let mut attempt = 0;
    loop {
        attempt += 1;
        let socket = match app.php.socket(version).await {
            Ok(socket) => socket,
            Err(err) => {
                return page(
                    StatusCode::BAD_GATEWAY,
                    "PHP is not available",
                    &escape(&err.to_string()),
                );
            }
        };
        match fastcgi::request(&socket, &params, &body).await {
            Ok(response) => {
                if !response.stderr.is_empty() {
                    tracing::warn!(site = %site.name, stderr = %response.stderr.trim_end(), "PHP stderr");
                }
                return php_response(response);
            }
            Err(FcgiError::Connect { .. }) if attempt == 1 => {
                tracing::warn!(
                    version,
                    "PHP-FPM did not accept the connection; restarting it"
                );
                app.php.restart(version).await;
            }
            Err(err) => {
                return page(
                    StatusCode::BAD_GATEWAY,
                    "PHP did not answer",
                    &format!(
                        "PHP-FPM {version} failed: {}. Its log is in <code>{}</code>.",
                        escape(&err.to_string()),
                        escape(&app.paths.log_dir.display().to_string())
                    ),
                );
            }
        }
    }
}

fn cgi_params(
    parts: &hyper::http::request::Parts,
    script: &PhpScript,
    body: &Bytes,
    remote: SocketAddr,
    local: SocketAddr,
) -> Vec<(String, String)> {
    let uri = &parts.uri;
    let request_uri = uri
        .path_and_query()
        .map_or("/", |pq| pq.as_str())
        .to_owned();
    let host = parts
        .headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let server_name = host.rsplit_once(':').map_or(host, |(h, _)| h).to_owned();

    let mut params = vec![
        ("GATEWAY_INTERFACE".into(), "CGI/1.1".into()),
        (
            "SERVER_SOFTWARE".into(),
            format!("RED/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("SERVER_PROTOCOL".into(), format!("{:?}", parts.version)),
        ("SERVER_NAME".into(), server_name),
        ("SERVER_ADDR".into(), local.ip().to_string()),
        ("SERVER_PORT".into(), local.port().to_string()),
        ("REMOTE_ADDR".into(), remote.ip().to_string()),
        ("REMOTE_PORT".into(), remote.port().to_string()),
        ("REQUEST_METHOD".into(), parts.method.to_string()),
        ("REQUEST_URI".into(), request_uri),
        ("DOCUMENT_URI".into(), script.script_name.clone()),
        (
            "QUERY_STRING".into(),
            uri.query().unwrap_or_default().to_owned(),
        ),
        ("SCRIPT_NAME".into(), script.script_name.clone()),
        (
            "SCRIPT_FILENAME".into(),
            script.script_filename.display().to_string(),
        ),
        (
            "DOCUMENT_ROOT".into(),
            script.document_root.display().to_string(),
        ),
        ("CONTENT_LENGTH".into(), body.len().to_string()),
        ("REDIRECT_STATUS".into(), "200".into()),
    ];
    if let Some(info) = &script.path_info {
        params.push(("PATH_INFO".into(), info.clone()));
    }
    if let Some(ct) = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        params.push(("CONTENT_TYPE".into(), ct.to_owned()));
    }
    for (name, value) in &parts.headers {
        if name == header::CONTENT_TYPE
            || name == header::CONTENT_LENGTH
            || name.as_str() == "proxy"
        {
            continue; // httpoxy: never pass a Proxy header as HTTP_PROXY
        }
        let Ok(value) = value.to_str() else { continue };
        let key = format!(
            "HTTP_{}",
            name.as_str().to_ascii_uppercase().replace('-', "_")
        );
        match params.iter_mut().find(|(k, _)| *k == key) {
            Some((_, existing)) => {
                existing.push_str(", ");
                existing.push_str(value);
            }
            None => params.push((key, value.to_owned())),
        }
    }
    params
}

fn php_response(fcgi: fastcgi::FcgiResponse) -> Response<Body> {
    let mut response = Response::new(Body::new(fcgi.body));
    *response.status_mut() = StatusCode::from_u16(fcgi.status).unwrap_or(StatusCode::OK);
    let headers = response.headers_mut();
    for (name, value) in fcgi.headers {
        let (Ok(name), Ok(value)) = (
            header::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) else {
            continue;
        };
        headers.append(name, value);
    }
    if !headers.contains_key(header::CONTENT_TYPE) {
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
    }
    response
}

/// A small HTML error page.
fn page(status: StatusCode, title: &str, detail_html: &str) -> Response<Body> {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>{title}</title>\
         <style>body{{font:15px/1.5 system-ui,sans-serif;max-width:40rem;margin:4rem auto;padding:0 1rem;color:#222}}\
         h1{{font-size:1.4rem}}code{{background:#f2f2f2;padding:.1em .3em;border-radius:4px}}\
         small{{color:#888}}</style></head><body><h1>{title}</h1><p>{detail_html}</p>\
         <small>{code} · RED</small></body></html>",
        code = status.as_u16(),
    );
    let mut response = Response::new(Body::new(Bytes::from(html)));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
