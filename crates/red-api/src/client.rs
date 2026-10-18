//! Minimal async client for the Control API over a unix socket.

use std::path::{Path, PathBuf};

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::net::UnixStream;

use crate::{ApiError, StatusResponse, endpoints};

pub use hyper::Method;

/// Errors returned by [`Client`].
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Nothing is listening on the socket.
    #[error("redd is not running (no socket at {})", socket.display())]
    DaemonNotRunning { socket: PathBuf },
    #[error("could not connect to {}: {source}", socket.display())]
    Connect {
        socket: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("HTTP error talking to redd: {0}")]
    Http(#[from] hyper::Error),
    #[error("invalid request: {0}")]
    Request(#[from] hyper::http::Error),
    /// The daemon answered with an error body.
    #[error("{}", error.message)]
    Api { status: u16, error: ApiError },
    #[error("unexpected response from redd: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Talks to one `redd` instance through its socket.
#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
}

impl Client {
    pub fn new(socket: impl Into<PathBuf>) -> Self {
        Self {
            socket: socket.into(),
        }
    }

    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// `GET /v1/status`.
    pub async fn status(&self) -> Result<StatusResponse, ClientError> {
        self.get(endpoints::STATUS).await
    }

    /// `GET` a path and decode the JSON response.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        self.call(Method::GET, path, None::<&()>).await
    }

    /// Send `body` as JSON with `method` and decode the JSON response.
    pub async fn call<B: Serialize, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, ClientError> {
        let body = body.map(serde_json::to_value).transpose()?;
        let value = self.request(method, path, body).await?;
        Ok(serde_json::from_value(value)?)
    }

    /// Send any request and return the JSON body. Non-2xx responses become
    /// [`ClientError::Api`].
    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ClientError> {
        let stream =
            UnixStream::connect(&self.socket)
                .await
                .map_err(|source| match source.kind() {
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                        ClientError::DaemonNotRunning {
                            socket: self.socket.clone(),
                        }
                    }
                    _ => ClientError::Connect {
                        socket: self.socket.clone(),
                        source,
                    },
                })?;

        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
        tokio::spawn(async move {
            // Errors surface through `send_request`; nothing to do here.
            let _ = connection.await;
        });

        let body = match body {
            Some(value) => Full::new(Bytes::from(serde_json::to_vec(&value)?)),
            None => Full::new(Bytes::new()),
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header(hyper::header::HOST, "redd")
            .header(hyper::header::ACCEPT, "application/json")
            .header(hyper::header::CONTENT_TYPE, "application/json")
            .body(body)?;

        let response = sender.send_request(request).await?;
        let status = response.status();
        let bytes = response.into_body().collect().await?.to_bytes();

        if status.is_success() {
            if bytes.is_empty() || status == StatusCode::NO_CONTENT {
                return Ok(serde_json::Value::Null);
            }
            return Ok(serde_json::from_slice(&bytes)?);
        }
        let error = serde_json::from_slice::<ApiError>(&bytes).unwrap_or_else(|_| {
            ApiError::new(
                crate::ErrorCode::Internal,
                format!(
                    "redd answered {status}: {}",
                    String::from_utf8_lossy(&bytes).trim()
                ),
            )
        });
        Err(ClientError::Api {
            status: status.as_u16(),
            error,
        })
    }
}
