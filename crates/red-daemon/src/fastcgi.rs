//! Minimal FastCGI client (responder role), enough to talk to PHP-FPM.
//!
//! One request per connection (`FCGI_KEEP_CONN` unset), with the request
//! body sent up front and the whole response read before returning.
//! Specification: <https://fastcgi-archives.github.io/FastCGI_Specification.html>.

use std::path::Path;

use hyper::body::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const VERSION: u8 = 1;
const BEGIN_REQUEST: u8 = 1;
const END_REQUEST: u8 = 3;
const PARAMS: u8 = 4;
const STDIN: u8 = 5;
const STDOUT: u8 = 6;
const STDERR: u8 = 7;
const RESPONDER: u16 = 1;
const REQUEST_ID: u16 = 1;
const MAX_CONTENT: usize = 0xffff;

/// Errors talking to a FastCGI server.
#[derive(Debug, thiserror::Error)]
pub enum FcgiError {
    /// The socket could not be reached (server down or restarting).
    #[error("could not connect to {path}: {source}")]
    Connect {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("I/O error during FastCGI request: {0}")]
    Io(#[from] std::io::Error),
    #[error("malformed FastCGI response: {0}")]
    Protocol(&'static str),
}

/// Response headers in the order the script sent them.
pub type Headers = Vec<(String, String)>;

/// A parsed CGI response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FcgiResponse {
    pub status: u16,
    pub headers: Headers,
    pub body: Bytes,
    /// Anything the script wrote to stderr (PHP warnings, notices).
    pub stderr: String,
}

/// Send one request and wait for the complete response.
pub async fn request(
    socket: &Path,
    params: &[(String, String)],
    body: &[u8],
) -> Result<FcgiResponse, FcgiError> {
    let mut stream = UnixStream::connect(socket)
        .await
        .map_err(|source| FcgiError::Connect {
            path: socket.display().to_string(),
            source,
        })?;
    stream.write_all(&encode_request(params, body)).await?;
    stream.flush().await?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    loop {
        let mut header = [0u8; 8];
        match stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(FcgiError::Protocol("connection closed before END_REQUEST"));
            }
            Err(err) => return Err(err.into()),
        }
        let kind = header[1];
        let length = u16::from_be_bytes([header[4], header[5]]) as usize;
        let padding = header[6] as usize;
        let mut content = vec![0u8; length + padding];
        stream.read_exact(&mut content).await?;
        content.truncate(length);
        match kind {
            STDOUT => stdout.extend_from_slice(&content),
            STDERR => stderr.extend_from_slice(&content),
            END_REQUEST => break,
            _ => {} // ignore unknown record types
        }
    }

    let (status, headers, body) = parse_cgi_output(&stdout)?;
    Ok(FcgiResponse {
        status,
        headers,
        body: Bytes::copy_from_slice(body),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// Encode BEGIN_REQUEST, PARAMS and STDIN records for one request.
pub fn encode_request(params: &[(String, String)], body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len() + 1024);

    let mut begin = [0u8; 8];
    begin[..2].copy_from_slice(&RESPONDER.to_be_bytes());
    record(&mut out, BEGIN_REQUEST, &begin);

    let mut encoded = Vec::new();
    for (name, value) in params {
        encode_length(&mut encoded, name.len());
        encode_length(&mut encoded, value.len());
        encoded.extend_from_slice(name.as_bytes());
        encoded.extend_from_slice(value.as_bytes());
    }
    stream_records(&mut out, PARAMS, &encoded);
    stream_records(&mut out, STDIN, body);
    out
}

/// Records of `kind` carrying `data`, followed by the empty record that ends
/// the stream.
fn stream_records(out: &mut Vec<u8>, kind: u8, data: &[u8]) {
    for chunk in data.chunks(MAX_CONTENT) {
        record(out, kind, chunk);
    }
    record(out, kind, &[]);
}

fn record(out: &mut Vec<u8>, kind: u8, content: &[u8]) {
    debug_assert!(content.len() <= MAX_CONTENT);
    let padding = (8 - content.len() % 8) % 8;
    out.push(VERSION);
    out.push(kind);
    out.extend_from_slice(&REQUEST_ID.to_be_bytes());
    out.extend_from_slice(&(content.len() as u16).to_be_bytes());
    out.push(padding as u8);
    out.push(0);
    out.extend_from_slice(content);
    out.extend(std::iter::repeat_n(0, padding));
}

fn encode_length(out: &mut Vec<u8>, len: usize) {
    if len < 128 {
        out.push(len as u8);
    } else {
        out.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes());
    }
}

/// Split CGI output into status, headers and body. `Status:` sets the status;
/// a `Location:` header without one means 302, as CGI specifies.
pub fn parse_cgi_output(output: &[u8]) -> Result<(u16, Headers, &[u8]), FcgiError> {
    let (head, body) = split_head(output).ok_or(FcgiError::Protocol("no header terminator"))?;
    let head = std::str::from_utf8(head).map_err(|_| FcgiError::Protocol("non-UTF-8 headers"))?;

    let mut status = None;
    let mut headers = Vec::new();
    for line in head.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(FcgiError::Protocol("header line without ':'"))?;
        let (name, value) = (name.trim(), value.trim());
        if name.eq_ignore_ascii_case("status") {
            let code = value.split_whitespace().next().unwrap_or_default();
            status = Some(
                code.parse()
                    .map_err(|_| FcgiError::Protocol("bad Status header"))?,
            );
        } else {
            headers.push((name.to_owned(), value.to_owned()));
        }
    }
    let status = status.unwrap_or_else(|| {
        if headers
            .iter()
            .any(|(n, _)| n.eq_ignore_ascii_case("location"))
        {
            302
        } else {
            200
        }
    });
    Ok((status, headers, body))
}

fn split_head(output: &[u8]) -> Option<(&[u8], &[u8])> {
    let crlf = output
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4));
    let lf = output.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let (at, len) = match (crlf, lf) {
        (Some(a), Some(b)) => {
            if a.0 <= b.0 {
                a
            } else {
                b
            }
        }
        (a, b) => a.or(b)?,
    };
    Some((&output[..at], &output[at + len..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::UnixListener;

    #[test]
    fn records_are_padded_to_eight_bytes() {
        let bytes = encode_request(&[("A".into(), "b".into())], b"hello");
        assert_eq!(bytes.len() % 8, 0);
        // BEGIN_REQUEST header + 8-byte body.
        assert_eq!(&bytes[..4], &[VERSION, BEGIN_REQUEST, 0, 1]);
    }

    #[test]
    fn long_names_use_four_byte_lengths() {
        let mut out = Vec::new();
        encode_length(&mut out, 127);
        encode_length(&mut out, 300);
        assert_eq!(out, [127, 0x80, 0, 1, 44]);
    }

    #[test]
    fn parses_status_headers_and_body() {
        let (status, headers, body) = parse_cgi_output(
            b"Status: 404 Not Found\r\nContent-Type: text/html\r\n\r\n<h1>no</h1>",
        )
        .unwrap();
        assert_eq!(status, 404);
        assert_eq!(headers, [("Content-Type".into(), "text/html".into())]);
        assert_eq!(body, b"<h1>no</h1>");

        let (status, _, _) = parse_cgi_output(b"Location: /login\n\n").unwrap();
        assert_eq!(status, 302);
        let (status, _, body) = parse_cgi_output(b"X-A: 1\n\nbody\r\n\r\nmore").unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, b"body\r\n\r\nmore");
        assert!(parse_cgi_output(b"no terminator").is_err());
    }

    /// A fake FastCGI server that checks the request and answers like PHP.
    #[tokio::test]
    async fn round_trip_against_fake_server() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("fpm.sock");
        let listener = UnixListener::bind(&socket).unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut params = Vec::new();
            let mut stdin = Vec::new();
            loop {
                let mut header = [0u8; 8];
                stream.read_exact(&mut header).await.unwrap();
                let len = u16::from_be_bytes([header[4], header[5]]) as usize;
                let mut content = vec![0u8; len + header[6] as usize];
                stream.read_exact(&mut content).await.unwrap();
                content.truncate(len);
                match header[1] {
                    PARAMS => params.extend(content),
                    STDIN if len == 0 => break,
                    STDIN => stdin.extend(content),
                    _ => {}
                }
            }
            assert!(params.windows(14).any(|w| w == b"REQUEST_METHOD"));
            assert_eq!(stdin.len(), 70_000); // spans two STDIN records

            let mut reply = Vec::new();
            record(&mut reply, STDERR, b"PHP Notice: hi");
            record(
                &mut reply,
                STDOUT,
                b"Status: 201 Created\r\nX-Powered-By: test\r\n\r\n",
            );
            record(&mut reply, STDOUT, b"created");
            record(&mut reply, END_REQUEST, &[0; 8]);
            stream.write_all(&reply).await.unwrap();
        });

        let params = vec![("REQUEST_METHOD".to_owned(), "POST".to_owned())];
        let response = request(&socket, &params, &vec![b'x'; 70_000])
            .await
            .unwrap();
        server.await.unwrap();

        assert_eq!(response.status, 201);
        assert_eq!(response.headers, [("X-Powered-By".into(), "test".into())]);
        assert_eq!(&response.body[..], b"created");
        assert_eq!(response.stderr, "PHP Notice: hi");
    }

    #[tokio::test]
    async fn missing_socket_is_a_connect_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = request(&dir.path().join("none.sock"), &[], b"")
            .await
            .unwrap_err();
        assert!(matches!(err, FcgiError::Connect { .. }));
    }
}
