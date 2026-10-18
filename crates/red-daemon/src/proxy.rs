//! Reverse proxy from `<name>.<tld>` to a local dev server, including
//! WebSocket upgrades (Vite/webpack HMR, Laravel Reverb, Next.js).
//!
//! The upstream sees its own address in `Host` (dev servers such as Vite
//! reject unknown hosts) and the original host in `X-Forwarded-Host`.

use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::header::{self, HeaderName, HeaderValue};
use hyper::{Request, Response, StatusCode, Uri};
use hyper_util::rt::TokioIo;
use tokio::net::TcpStream;

use crate::body::{self, Body, BoxError};
use crate::web::Conn;

/// Headers that describe a single hop and must not be forwarded.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// Errors before a response could be produced.
#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("invalid proxy target {0}")]
    Target(String),
    #[error("nothing is listening on {target}: {source}")]
    Connect {
        target: String,
        #[source]
        source: std::io::Error,
    },
    #[error("upstream error: {0}")]
    Http(#[from] hyper::Error),
    #[error("invalid request: {0}")]
    Request(#[from] hyper::http::Error),
}

/// Forward `req` to `target` (an `http://host:port[/base]` URL).
pub async fn forward(
    mut req: Request<Incoming>,
    target: &str,
    conn: Conn,
) -> Result<Response<Body>, ProxyError> {
    let upstream: Uri = target
        .parse()
        .map_err(|_| ProxyError::Target(target.to_owned()))?;
    let authority = upstream
        .authority()
        .ok_or_else(|| ProxyError::Target(target.to_owned()))?
        .clone();
    let base = upstream.path().trim_end_matches('/');

    let wants_upgrade = is_upgrade(req.headers());
    let client_upgrade = wants_upgrade.then(|| hyper::upgrade::on(&mut req));

    let stream = TcpStream::connect(authority.as_str())
        .await
        .map_err(|source| ProxyError::Connect {
            target: target.to_owned(),
            source,
        })?;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        if let Err(err) = connection.with_upgrades().await {
            tracing::debug!(%err, "upstream connection closed");
        }
    });

    let (parts, incoming) = req.into_parts();
    let path_and_query = parts.uri.path_and_query().map_or("/", |pq| pq.as_str());
    let mut outgoing = Request::builder()
        .method(parts.method.clone())
        .uri(format!("{base}{path_and_query}"));
    let original_host = parts.headers.get(header::HOST).cloned().or_else(|| {
        parts
            .uri
            .authority()
            .and_then(|a| HeaderValue::from_str(a.as_str()).ok())
    });
    let headers = outgoing.headers_mut().expect("builder has no error yet");
    for (name, value) in &parts.headers {
        if name == header::HOST || HOP_BY_HOP.contains(&name.as_str()) {
            continue;
        }
        headers.append(name.clone(), value.clone());
    }
    headers.insert(
        header::HOST,
        HeaderValue::from_str(authority.as_str())
            .map_err(|_| ProxyError::Target(target.to_owned()))?,
    );
    if let Some(host) = original_host {
        headers.insert(HeaderName::from_static("x-forwarded-host"), host);
    }
    headers.insert(
        HeaderName::from_static("x-forwarded-proto"),
        HeaderValue::from_static(if conn.secure { "https" } else { "http" }),
    );
    if let Ok(ip) = HeaderValue::from_str(&conn.remote.ip().to_string()) {
        headers.insert(HeaderName::from_static("x-forwarded-for"), ip);
    }
    if wants_upgrade {
        // Re-add the two hop-by-hop headers an upgrade needs.
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
        if let Some(protocol) = parts.headers.get(header::UPGRADE) {
            headers.insert(header::UPGRADE, protocol.clone());
        }
    }

    let mut response = sender.send_request(outgoing.body(incoming)?).await?;

    if response.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client_upgrade) = client_upgrade
    {
        let upstream_upgrade = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            match tokio::try_join!(client_upgrade, upstream_upgrade) {
                Ok((client, upstream)) => {
                    let mut client = TokioIo::new(client);
                    let mut upstream = TokioIo::new(upstream);
                    if let Err(err) =
                        tokio::io::copy_bidirectional(&mut client, &mut upstream).await
                    {
                        tracing::debug!(%err, "upgraded connection closed");
                    }
                }
                Err(err) => tracing::debug!(%err, "upgrade failed"),
            }
        });
        let (parts, _) = response.into_parts();
        return Ok(Response::from_parts(parts, body::empty()));
    }

    let (mut parts, incoming) = response.into_parts();
    for name in HOP_BY_HOP {
        parts.headers.remove(*name);
    }
    Ok(Response::from_parts(
        parts,
        incoming.map_err(BoxError::from).boxed(),
    ))
}

fn is_upgrade(headers: &hyper::HeaderMap) -> bool {
    headers.contains_key(header::UPGRADE)
        && headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_upgrade_requests() {
        let mut headers = hyper::HeaderMap::new();
        headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
        headers.insert(
            header::CONNECTION,
            HeaderValue::from_static("keep-alive, Upgrade"),
        );
        assert!(is_upgrade(&headers));
        headers.insert(header::CONNECTION, HeaderValue::from_static("keep-alive"));
        assert!(!is_upgrade(&headers));
    }
}
