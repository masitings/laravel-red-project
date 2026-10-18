//! End-to-end tests for the DNS responder and HTTPS: a real redd, real UDP
//! queries, and a rustls client that trusts only RED's local CA (so the
//! certificate chain, SANs and name constraints are all verified).

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::Request;
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use red_api::client::{Client, Method};
use red_api::{PathRequest, SiteInfo, StatusResponse, endpoints};
use red_daemon::DaemonError;
use red_platform::Paths;
use rustls_pki_types::pem::PemObject;
use rustls_pki_types::{CertificateDer, ServerName};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::oneshot;

struct Daemon {
    client: Client,
    status: StatusResponse,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), DaemonError>>,
    _home: tempfile::TempDir,
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

async fn start() -> Daemon {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::under(home.path().join("red"));
    write(
        &paths.config_file(),
        "http_port = 0\nhttps_port = 0\ndns_port = 0\n",
    );
    let sites = home.path().join("Sites");
    write(&sites.join("shop/index.html"), "secure shop");

    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(red_daemon::run(paths.clone(), async {
        let _ = stopped.await;
    }));
    let client = Client::new(paths.socket());
    let mut status = None;
    for _ in 0..200 {
        if let Ok(s) = client.status().await {
            status = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let status = status.expect("redd did not start");
    let _: Vec<String> = client
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
    Daemon {
        client,
        status,
        stop: Some(stop),
        task,
        _home: home,
    }
}

impl Daemon {
    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap().unwrap();
    }

    async fn secure(&self, site: &str, on: bool) -> SiteInfo {
        let method = if on { Method::PUT } else { Method::DELETE };
        self.client
            .call(method, &endpoints::site_secure(site), None::<&()>)
            .await
            .unwrap()
    }

    /// GET over TLS, trusting only RED's CA. Returns (status, ALPN, body).
    async fn https_get(&self, sni: &str, path: &str) -> Result<(u16, String, String), String> {
        let mut roots = rustls::RootCertStore::empty();
        let ca = CertificateDer::from_pem_file(&self.status.paths.ca_cert).unwrap();
        roots.add(ca).unwrap();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(roots)
            .with_no_client_auth();
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));

        let port = self.status.https_port.unwrap();
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let tls = connector
            .connect(ServerName::try_from(sni.to_owned()).unwrap(), tcp)
            .await
            .map_err(|e| e.to_string())?;
        let alpn = tls
            .get_ref()
            .1
            .alpn_protocol()
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .unwrap_or_default();

        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tls))
            .await
            .map_err(|e| e.to_string())?;
        tokio::spawn(conn);
        let request = Request::builder()
            .uri(path)
            .header("host", sni)
            .body(Empty::<Bytes>::new())
            .unwrap();
        let response = sender
            .send_request(request)
            .await
            .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        Ok((status, alpn, String::from_utf8_lossy(&body).into_owned()))
    }

    async fn http_get(&self, host: &str) -> (u16, Option<String>) {
        let port = self.status.http_port.unwrap();
        let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(tcp))
            .await
            .unwrap();
        tokio::spawn(conn);
        let request = Request::builder()
            .uri("/page?x=1")
            .header("host", host)
            .body(Empty::<Bytes>::new())
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        let location = response
            .headers()
            .get("location")
            .map(|v| v.to_str().unwrap().to_owned());
        (response.status().as_u16(), location)
    }
}

fn dns_query(id: u16, name: &str, qtype: u16) -> Vec<u8> {
    let mut q = Vec::new();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in name.split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.extend_from_slice(&[0, (qtype >> 8) as u8, qtype as u8, 0, 1]);
    q
}

#[tokio::test]
async fn dns_answers_the_local_tld_over_udp() {
    let daemon = start().await;
    let port = daemon.status.dns_port.expect("DNS responder running");
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    socket
        .send_to(&dns_query(7, "shop.test", 1), ("127.0.0.1", port))
        .await
        .unwrap();
    let mut buf = [0u8; 512];
    let len = tokio::time::timeout(Duration::from_secs(2), socket.recv(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let reply = &buf[..len];
    assert_eq!(&reply[..2], &[0, 7]);
    assert_eq!(reply[3] & 0x0f, 0, "NOERROR");
    assert_eq!(&reply[len - 4..], &[127, 0, 0, 1]);

    socket
        .send_to(&dns_query(8, "example.com", 1), ("127.0.0.1", port))
        .await
        .unwrap();
    let len = socket.recv(&mut buf).await.unwrap();
    assert_eq!(buf[3] & 0x0f, 5, "REFUSED");
    assert!(len > 12);

    daemon.stop().await;
}

#[tokio::test]
async fn secured_sites_are_served_over_trusted_https() {
    let daemon = start().await;
    let https_port = daemon.status.https_port.expect("HTTPS server running");

    // Not secured yet: no certificate, so the handshake fails.
    assert!(daemon.https_get("shop.test", "/").await.is_err());
    assert_eq!(daemon.http_get("shop.test").await.0, 404);

    let site = daemon.secure("shop", true).await;
    assert!(site.secure);
    assert!(site.cert_expires.is_some());
    assert_eq!(site.url, format!("https://shop.test:{https_port}"));

    let (status, alpn, body) = daemon.https_get("shop.test", "/").await.unwrap();
    assert_eq!(status, 200);
    assert_eq!(alpn, "http/1.1");
    assert_eq!(body, "secure shop");

    // Subdomains and *.localhost are covered by the same certificate.
    assert_eq!(daemon.https_get("api.shop.test", "/").await.unwrap().0, 200);
    assert_eq!(
        daemon.https_get("shop.localhost", "/").await.unwrap().0,
        200
    );

    // Plain HTTP now redirects to HTTPS, keeping path and query.
    let (status, location) = daemon.http_get("shop.test").await;
    assert_eq!(status, 301);
    assert_eq!(
        location.as_deref(),
        Some(format!("https://shop.test:{https_port}/page?x=1").as_str())
    );

    let site = daemon.secure("shop", false).await;
    assert!(!site.secure);
    assert!(daemon.https_get("shop.test", "/").await.is_err());
    // Back to plain HTTP: no redirect (the page itself does not exist).
    assert_eq!(daemon.http_get("shop.test").await, (404, None));

    daemon.stop().await;
}

#[tokio::test]
async fn ca_is_constrained_to_local_names() {
    let daemon = start().await;
    daemon.secure("shop", true).await;
    // The leaf only names shop.test / shop.localhost; asking for another
    // name must fail verification even though RED's CA is trusted.
    let err = daemon.https_get("example.com", "/").await.unwrap_err();
    assert!(!err.is_empty());
    daemon.stop().await;
}
