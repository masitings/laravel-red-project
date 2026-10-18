//! End-to-end tests for proxies (HTTP and WebSocket upgrades) and the
//! `/v1/events` stream.

use std::convert::Infallible;
use std::time::Duration;

use http_body_util::{BodyExt, Empty, Full};
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use red_api::client::{Client, ClientError, Method};
use red_api::{Event, PathRequest, ProxyInfo, ProxyRequest, StatusResponse, endpoints};
use red_daemon::DaemonError;
use red_platform::Paths;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

struct Daemon {
    client: Client,
    status: StatusResponse,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), DaemonError>>,
    home: tempfile::TempDir,
}

async fn start() -> Daemon {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::under(home.path().join("red"));
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.config_file(),
        "http_port = 0\nhttps_port = 0\ndns_port = 0\n",
    )
    .unwrap();
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(red_daemon::run(paths.clone(), async {
        let _ = stopped.await;
    }));
    let client = Client::new(paths.socket());
    for _ in 0..200 {
        if let Ok(status) = client.status().await {
            return Daemon {
                client,
                status,
                stop: Some(stop),
                task,
                home,
            };
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("redd did not start");
}

impl Daemon {
    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap().unwrap();
    }

    async fn proxy(&self, name: &str, target: &str) -> Result<ProxyInfo, ClientError> {
        self.client
            .call(
                Method::POST,
                endpoints::PROXIES,
                Some(&ProxyRequest {
                    name: name.into(),
                    target: target.into(),
                }),
            )
            .await
    }

    async fn get(&self, host: &str, path: &str) -> (u16, String) {
        let tcp = TcpStream::connect(("127.0.0.1", self.status.http_port.unwrap()))
            .await
            .unwrap();
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tcp))
            .await
            .unwrap();
        tokio::spawn(conn);
        let request = Request::builder()
            .uri(path)
            .header("host", host)
            .body(Empty::<Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }
}

/// An upstream HTTP server that describes the request it received.
async fn echo_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let service =
                    hyper::service::service_fn(|req: Request<hyper::body::Incoming>| async move {
                        let header = |name: &str| {
                            req.headers()
                                .get(name)
                                .map(|v| v.to_str().unwrap().to_owned())
                                .unwrap_or_default()
                        };
                        let text = format!(
                            "host={} fwd-host={} fwd-proto={} path={}",
                            header("host"),
                            header("x-forwarded-host"),
                            header("x-forwarded-proto"),
                            req.uri()
                        );
                        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(text))))
                    });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    port
}

/// An upstream that accepts a WebSocket-style upgrade and then echoes bytes
/// back in upper case.
async fn upgrade_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let head = read_head(&mut stream).await;
        assert!(
            head.to_ascii_lowercase().contains("upgrade: websocket"),
            "{head}"
        );
        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
            )
            .await
            .unwrap();
        let mut buf = [0u8; 64];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                return;
            }
            stream
                .write_all(&buf[..n].to_ascii_uppercase())
                .await
                .unwrap();
        }
    });
    port
}

async fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    String::from_utf8(head).unwrap()
}

#[tokio::test]
async fn proxies_http_requests_to_a_local_port() {
    let daemon = start().await;
    let upstream = echo_upstream().await;
    let proxy = daemon.proxy("vite", &upstream.to_string()).await.unwrap();
    assert_eq!(proxy.target, format!("http://127.0.0.1:{upstream}"));

    let (status, body) = daemon.get("vite.test", "/src/main.ts?t=1").await;
    assert_eq!(status, 200);
    assert_eq!(
        body,
        format!(
            "host=127.0.0.1:{upstream} fwd-host=vite.test fwd-proto=http path=/src/main.ts?t=1"
        )
    );
    // Subdomains go to the same proxy.
    assert_eq!(daemon.get("hmr.vite.test", "/").await.0, 200);

    let proxies: Vec<ProxyInfo> = daemon.client.get(endpoints::PROXIES).await.unwrap();
    assert_eq!(proxies.len(), 1);

    let _: serde_json::Value = daemon
        .client
        .call(Method::DELETE, &endpoints::proxy("vite"), None::<&()>)
        .await
        .unwrap();
    assert_eq!(daemon.get("vite.test", "/").await.0, 404);
    daemon.stop().await;
}

#[tokio::test]
async fn proxies_websocket_upgrades() {
    let daemon = start().await;
    let upstream = upgrade_upstream().await;
    daemon
        .proxy("hmr", &format!("localhost:{upstream}"))
        .await
        .unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", daemon.status.http_port.unwrap()))
        .await
        .unwrap();
    stream
        .write_all(
            b"GET /socket HTTP/1.1\r\nHost: hmr.test\r\nUpgrade: websocket\r\n\
              Connection: Upgrade\r\nSec-WebSocket-Version: 13\r\n\
              Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
        )
        .await
        .unwrap();
    let head = read_head(&mut stream).await;
    assert!(head.starts_with("HTTP/1.1 101"), "{head}");

    stream.write_all(b"ping").await.unwrap();
    let mut buf = [0u8; 4];
    tokio::time::timeout(Duration::from_secs(2), stream.read_exact(&mut buf))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&buf, b"PING");
    daemon.stop().await;
}

#[tokio::test]
async fn unreachable_proxy_targets_get_a_helpful_502() {
    let daemon = start().await;
    let free = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    daemon.proxy("down", &port.to_string()).await.unwrap();

    let (status, body) = daemon.get("down.test", "/").await;
    assert_eq!(status, 502);
    assert!(body.contains("Is your dev server running?"));

    let err = daemon.proxy("bad", "https://x:1").await.unwrap_err();
    assert!(matches!(err, ClientError::Api { status: 400, .. }));
    daemon.stop().await;
}

#[tokio::test]
async fn events_stream_reports_changes() {
    let daemon = start().await;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let client = daemon.client.clone();
    let listener = tokio::spawn(async move {
        client
            .events(|event| {
                let _ = tx.send(event);
                true
            })
            .await
    });

    assert_eq!(next(&mut rx).await, Event::Connected);

    let sites = daemon.home.path().join("Sites");
    std::fs::create_dir_all(&sites).unwrap();
    let _: Vec<String> = daemon
        .client
        .call(
            Method::POST,
            endpoints::PATHS,
            Some(&PathRequest {
                path: sites.display().to_string(),
                name: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(next(&mut rx).await, Event::SitesChanged);

    daemon.proxy("app", "3000").await.unwrap();
    assert_eq!(next(&mut rx).await, Event::ProxiesChanged);

    listener.abort();
    daemon.stop().await;
}

async fn next(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("event in time")
        .expect("stream open")
}
