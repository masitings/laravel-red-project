//! The response body type shared by the web server, FastCGI and the proxy:
//! a boxed, streaming body so large files and long PHP output are sent as
//! they are produced instead of being buffered.

use futures_util::TryStreamExt;
use http_body_util::{BodyExt, Empty, Full, StreamBody};
use hyper::body::{Bytes, Frame};

/// Error type carried by response bodies.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// A streaming response body.
pub type Body = http_body_util::combinators::BoxBody<Bytes, BoxError>;

/// A body holding `bytes`.
pub fn full(bytes: impl Into<Bytes>) -> Body {
    Full::new(bytes.into())
        .map_err(|never| match never {})
        .boxed()
}

/// An empty body.
pub fn empty() -> Body {
    Empty::<Bytes>::new()
        .map_err(|never| match never {})
        .boxed()
}

/// A body that streams `file` in chunks.
pub fn file(file: tokio::fs::File) -> Body {
    let stream = tokio_util::io::ReaderStream::with_capacity(file, 64 * 1024)
        .map_ok(Frame::data)
        .map_err(BoxError::from);
    BodyExt::boxed(StreamBody::new(stream))
}

/// A body fed by a channel; the sender side produces frames.
pub fn channel(rx: tokio::sync::mpsc::Receiver<Result<Frame<Bytes>, BoxError>>) -> Body {
    BodyExt::boxed(StreamBody::new(
        tokio_stream::wrappers::ReceiverStream::new(rx),
    ))
}
