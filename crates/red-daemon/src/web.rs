//! The web server that serves sites.
//!
//! Each request is routed host → site → driver → action. Static files are
//! read from disk; PHP goes to the site's PHP-FPM pool over FastCGI. If a
//! pool does not answer, it is restarted once and the request retried.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use http_body_util::{BodyExt, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{self, HeaderValue};
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use red_core::driver::{self, Action, PhpScript};
use red_core::sites::Target;
use red_core::{Site, sites};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::AppState;
use crate::body::{self, Body};
use crate::fastcgi::{self, FcgiError};
use crate::proxy;

/// How a request reached us.
#[derive(Debug, Clone, Copy)]
pub struct Conn {
    pub remote: SocketAddr,
    pub local: SocketAddr,
    /// `true` for requests that arrived over TLS.
    pub secure: bool,
}

/// How long a client may take to finish the TLS handshake.
const TLS_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Largest request body passed to PHP (uploads included).
const MAX_BODY: usize = 512 * 1024 * 1024;

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

/// Serve plain HTTP on every listener until `cancel` fires.
pub async fn serve(listeners: Vec<TcpListener>, app: Arc<AppState>, cancel: CancellationToken) {
    accept_loop(listeners, app, cancel, None).await;
}

/// Serve HTTPS (HTTP/1.1 and HTTP/2) on every listener until `cancel` fires.
pub async fn serve_tls(
    listeners: Vec<TcpListener>,
    app: Arc<AppState>,
    acceptor: tokio_rustls::TlsAcceptor,
    cancel: CancellationToken,
) {
    accept_loop(listeners, app, cancel, Some(acceptor)).await;
}

async fn accept_loop(
    listeners: Vec<TcpListener>,
    app: Arc<AppState>,
    cancel: CancellationToken,
    tls: Option<tokio_rustls::TlsAcceptor>,
) {
    let mut tasks = Vec::new();
    for listener in listeners {
        let app = app.clone();
        let cancel = cancel.clone();
        let tls = tls.clone();
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
                let conn = Conn {
                    remote,
                    local: stream.local_addr().unwrap_or(remote),
                    secure: tls.is_some(),
                };
                let app = app.clone();
                let tls = tls.clone();
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| {
                        let app = app.clone();
                        async move { Ok::<_, Infallible>(handle(req, app, conn).await) }
                    });
                    let result = match tls {
                        None => hyper::server::conn::http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .with_upgrades()
                            .await
                            .map_err(|err| err.to_string()),
                        Some(acceptor) => {
                            let tls_stream = match tokio::time::timeout(
                                TLS_HANDSHAKE_TIMEOUT,
                                acceptor.accept(stream),
                            )
                            .await
                            {
                                Ok(Ok(tls_stream)) => tls_stream,
                                Ok(Err(err)) => {
                                    tracing::debug!(%err, "TLS handshake failed");
                                    return;
                                }
                                Err(_) => return,
                            };
                            hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            )
                            .serve_connection_with_upgrades(TokioIo::new(tls_stream), service)
                            .await
                            .map_err(|err| err.to_string())
                        }
                    };
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

async fn handle(req: Request<Incoming>, app: Arc<AppState>, conn: Conn) -> Response<Body> {
    let host = host_of(&req).unwrap_or_default();
    let state = app.state_snapshot();
    let Some(target) = sites::resolve_target(&state, &host, &app.config.tld) else {
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

    if !conn.secure
        && state.secured.contains_key(target.name())
        && let Some(https_port) = app.https_port.get()
    {
        let authority = host.rsplit_once(':').map_or(host.as_str(), |(h, _)| h);
        let port = if *https_port == 443 {
            String::new()
        } else {
            format!(":{https_port}")
        };
        let path = req.uri().path_and_query().map_or("/", |pq| pq.as_str());
        return redirect(&format!("https://{authority}{port}{path}"));
    }

    let site = match target {
        Target::Site(site) => site,
        Target::Proxy { name, url } => {
            return match proxy::forward(req, &url, conn).await {
                Ok(response) => response,
                Err(err) => {
                    tracing::debug!(proxy = %name, %err, "proxy failed");
                    page(
                        StatusCode::BAD_GATEWAY,
                        "Proxy target not reachable",
                        &format!(
                            "<code>{}</code> forwards to <code>{}</code>, but: {}. \
                             Is your dev server running?",
                            escape(&name),
                            escape(&url),
                            escape(&err.to_string())
                        ),
                    )
                }
            };
        }
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
            redirect(&location)
        }
        Action::Static(path) => serve_file(req.method(), &path).await,
        Action::Php(script) => {
            let version = app.effective_php(&state, &site.name);
            match version {
                Some(version) => run_php(req, &app, &site, &script, &version, conn).await,
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
    match tokio::fs::File::open(path).await {
        Ok(file) => {
            let len = file.metadata().await.map(|m| m.len()).ok();
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            let content_type = if mime.type_() == mime_guess::mime::TEXT
                || mime.subtype() == mime_guess::mime::JAVASCRIPT
                || mime.subtype() == mime_guess::mime::JSON
            {
                format!("{mime}; charset=utf-8")
            } else {
                mime.to_string()
            };
            let mut response = Response::new(body::file(file));
            if let Ok(value) = HeaderValue::from_str(&content_type) {
                response.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            if let Some(len) = len {
                response
                    .headers_mut()
                    .insert(header::CONTENT_LENGTH, HeaderValue::from(len));
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
    conn: Conn,
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
    let params = cgi_params(&parts, script, &body, conn);

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
        let label = format!("{} {}", site.name, parts.uri.path());
        match fastcgi::request(&socket, &params, &body, label).await {
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
    conn: Conn,
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
        ("SERVER_ADDR".into(), conn.local.ip().to_string()),
        ("SERVER_PORT".into(), conn.local.port().to_string()),
        ("REMOTE_ADDR".into(), conn.remote.ip().to_string()),
        ("REMOTE_PORT".into(), conn.remote.port().to_string()),
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
    if conn.secure {
        params.push(("HTTPS".into(), "on".into()));
        params.push(("REQUEST_SCHEME".into(), "https".into()));
    } else {
        params.push(("REQUEST_SCHEME".into(), "http".into()));
    }
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
    let mut response = Response::new(fcgi.body);
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

/// A permanent redirect to `location`.
fn redirect(location: &str) -> Response<Body> {
    let mut response = Response::new(body::empty());
    *response.status_mut() = StatusCode::MOVED_PERMANENTLY;
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(header::LOCATION, value);
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
    let mut response = Response::new(body::full(html));
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
