//! End-to-end tests for the web server: a real redd in a temporary RED_HOME,
//! real HTTP over TCP, and a real PHP-FPM when one is installed (PHP tests
//! are skipped otherwise).

use std::path::{Path, PathBuf};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::{Method, Request};
use hyper_util::rt::TokioIo;
use red_api::client::{Client, ClientError};
use red_api::{PathRequest, PhpListResponse, PhpVersionRequest, SiteInfo, endpoints};
use red_daemon::DaemonError;
use red_platform::Paths;
use tokio::net::TcpStream;
use tokio::sync::oneshot;

struct Daemon {
    paths: Paths,
    client: Client,
    port: u16,
    sites: PathBuf,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), DaemonError>>,
    _home: tempfile::TempDir,
}

struct Reply {
    status: u16,
    headers: hyper::HeaderMap,
    body: String,
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

const ECHO_PHP: &str = r#"<?php
header('X-Test: yes');
header('Content-Type: application/json');
echo json_encode([
    'uri' => $_SERVER['REQUEST_URI'],
    'script' => $_SERVER['SCRIPT_NAME'],
    'method' => $_SERVER['REQUEST_METHOD'],
    'host' => $_SERVER['HTTP_HOST'],
    'query' => $_GET,
    'body' => file_get_contents('php://input'),
    'php' => PHP_MAJOR_VERSION . '.' . PHP_MINOR_VERSION,
    'path_info' => $_SERVER['PATH_INFO'] ?? null,
]);
"#;

async fn start() -> Daemon {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::under(home.path().join("red"));
    write(
        &paths.config_file(),
        "http_port = 0\nhttps_port = 0\ndns_port = 0\n",
    );

    let sites = home.path().join("Sites");
    write(
        &sites.join("static-site/index.html"),
        "<h1>hello static</h1>",
    );
    write(&sites.join("static-site/.env"), "SECRET=1");
    write(&sites.join("laravel-app/artisan"), "");
    write(&sites.join("laravel-app/public/index.php"), ECHO_PHP);
    write(&sites.join("laravel-app/public/app.css"), "body{}");
    write(&sites.join("wordpress/wp-config.php"), "<?php");
    write(&sites.join("wordpress/index.php"), ECHO_PHP);
    write(&sites.join("wordpress/wp-admin/index.php"), ECHO_PHP);
    write(
        &sites.join("streamer/index.php"),
        "<?php while (ob_get_level()) { ob_end_flush(); }\n\
         echo str_repeat('a', 8192); flush(); usleep(700000); echo 'done';",
    );

    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(red_daemon::run(paths.clone(), async {
        let _ = stopped.await;
    }));
    let client = Client::new(paths.socket());
    let mut port = None;
    for _ in 0..200 {
        if let Ok(status) = client.status().await {
            port = status.http_port;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let port = port.expect("redd did not start a web server");

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
        paths,
        client,
        port,
        sites,
        stop: Some(stop),
        task,
        _home: home,
    }
}

impl Daemon {
    async fn send(&self, method: Method, host: &str, path: &str, body: &str) -> Reply {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).await.unwrap();
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(conn);
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("host", host)
            .header("content-type", "text/plain")
            .body(Full::new(Bytes::from(body.to_owned())))
            .unwrap();
        let response = sender.send_request(request).await.unwrap();
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        Reply {
            status,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    async fn get(&self, host: &str, path: &str) -> Reply {
        self.send(Method::GET, host, path, "").await
    }

    /// The newest installed PHP version, or `None` when the machine has no
    /// php-fpm (PHP tests are skipped then).
    async fn php(&self) -> Option<String> {
        let list: PhpListResponse = self.client.get(endpoints::PHP).await.unwrap();
        if list.installs.is_empty() {
            eprintln!("skipping PHP assertions: no php-fpm installed");
        }
        list.default
    }

    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap().unwrap();
    }
}

fn json(reply: &Reply) -> serde_json::Value {
    serde_json::from_str(&reply.body).unwrap_or_else(|_| panic!("not JSON: {}", reply.body))
}

#[tokio::test]
async fn serves_static_files_and_hides_dotfiles() {
    let daemon = start().await;

    let reply = daemon.get("static-site.test", "/").await;
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, "<h1>hello static</h1>");
    assert_eq!(reply.headers["content-type"], "text/html; charset=utf-8");

    // .localhost works too, which lets browsers reach sites before DNS is set up.
    let reply = daemon
        .get(&format!("static-site.localhost:{}", daemon.port), "/")
        .await;
    assert_eq!(reply.status, 200);

    assert_eq!(daemon.get("static-site.test", "/.env").await.status, 404);
    assert_eq!(daemon.get("static-site.test", "/nope").await.status, 404);

    let reply = daemon.get("unknown.test", "/").await;
    assert_eq!(reply.status, 404);
    assert!(reply.body.contains("No site here"));

    daemon.stop().await;
}

#[tokio::test]
async fn lists_sites_with_drivers_and_urls() {
    let daemon = start().await;
    let sites: Vec<SiteInfo> = daemon.client.get(endpoints::SITES).await.unwrap();
    let summary: Vec<(&str, &str)> = sites
        .iter()
        .map(|s| (s.name.as_str(), s.driver.as_str()))
        .collect();
    assert_eq!(
        summary,
        [
            ("laravel-app", "laravel"),
            ("static-site", "static"),
            ("streamer", "php"),
            ("wordpress", "wordpress")
        ]
    );
    assert_eq!(
        sites[0].url,
        format!("http://laravel-app.test:{}", daemon.port)
    );

    let state = std::fs::read_to_string(daemon.paths.state_file()).unwrap();
    assert!(state.contains(&daemon.sites.display().to_string()));
    daemon.stop().await;
}

#[tokio::test]
async fn runs_php_through_the_front_controller() {
    let daemon = start().await;
    let Some(version) = daemon.php().await else {
        return daemon.stop().await;
    };

    let reply = daemon.get("laravel-app.test", "/users/42?tab=posts").await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(reply.headers["x-test"], "yes");
    let body = json(&reply);
    assert_eq!(body["uri"], "/users/42?tab=posts");
    assert_eq!(body["script"], "/index.php");
    assert_eq!(body["host"], "laravel-app.test");
    assert_eq!(body["query"]["tab"], "posts");
    assert_eq!(body["php"], version.as_str());

    let reply = daemon
        .send(Method::POST, "laravel-app.test", "/api/echo", "payload=1")
        .await;
    assert_eq!(json(&reply)["body"], "payload=1");
    assert_eq!(json(&reply)["method"], "POST");

    // Static files next to the front controller are served directly.
    let reply = daemon.get("laravel-app.test", "/app.css").await;
    assert_eq!(reply.body, "body{}");
    assert!(
        reply.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/css")
    );

    daemon.stop().await;
}

#[tokio::test]
async fn wordpress_layout_and_path_info() {
    let daemon = start().await;
    if daemon.php().await.is_none() {
        return daemon.stop().await;
    }

    let reply = daemon.get("wordpress.test", "/wp-admin?x=1").await;
    assert_eq!(reply.status, 301);
    assert_eq!(reply.headers["location"], "/wp-admin/?x=1");

    let reply = daemon.get("wordpress.test", "/wp-admin/").await;
    assert_eq!(json(&reply)["script"], "/wp-admin/index.php");

    let reply = daemon.get("wordpress.test", "/index.php/feed/rss").await;
    assert_eq!(json(&reply)["path_info"], "/feed/rss");

    daemon.stop().await;
}

#[tokio::test]
async fn restarts_php_fpm_after_a_crash() {
    let daemon = start().await;
    let Some(version) = daemon.php().await else {
        return daemon.stop().await;
    };
    assert_eq!(daemon.get("laravel-app.test", "/").await.status, 200);

    let pid_file = daemon.paths.php_dir(&version).join("php-fpm.pid");
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    std::process::Command::new("kill")
        .arg("-9")
        .arg(pid.to_string())
        .status()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    let reply = daemon.get("laravel-app.test", "/again").await;
    assert_eq!(reply.status, 200, "{}", reply.body);
    assert_eq!(json(&reply)["uri"], "/again");
    daemon.stop().await;
}

#[tokio::test]
async fn links_and_isolation() {
    let daemon = start().await;

    let project = daemon.sites.join("../elsewhere/My Project");
    write(&project.join("index.html"), "linked!");
    let project = project.canonicalize().unwrap();
    let site: SiteInfo = daemon
        .client
        .call(
            Method::POST,
            endpoints::LINKS,
            Some(&PathRequest {
                path: project.display().to_string(),
                name: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(site.name, "my-project");
    assert_eq!(daemon.get("my-project.test", "/").await.body, "linked!");

    let err = daemon
        .client
        .call::<_, SiteInfo>(
            Method::PUT,
            &endpoints::site_php("my-project"),
            Some(&PhpVersionRequest {
                version: "5.6".into(),
            }),
        )
        .await
        .unwrap_err();
    let ClientError::Api { status, error } = err else {
        panic!("unexpected {err:?}");
    };
    assert_eq!(status, 400);
    assert!(error.hint.is_some());

    if let Some(version) = daemon.php().await {
        let site: SiteInfo = daemon
            .client
            .call(
                Method::PUT,
                &endpoints::site_php("my-project"),
                Some(&PhpVersionRequest {
                    version: version.clone(),
                }),
            )
            .await
            .unwrap();
        assert!(site.isolated);
        assert_eq!(site.php.as_deref(), Some(version.as_str()));
    }

    let _: serde_json::Value = daemon
        .client
        .call(Method::DELETE, &endpoints::link("my-project"), None::<&()>)
        .await
        .unwrap();
    assert_eq!(daemon.get("my-project.test", "/").await.status, 404);
    daemon.stop().await;
}

#[tokio::test]
async fn php_output_is_streamed_before_the_script_finishes() {
    let daemon = start().await;
    if daemon.php().await.is_none() {
        return daemon.stop().await;
    }
    let stream = TcpStream::connect(("127.0.0.1", daemon.port))
        .await
        .unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let started = std::time::Instant::now();
    let request = Request::builder()
        .uri("/")
        .header("host", "streamer.test")
        .body(Full::new(Bytes::new()))
        .unwrap();
    let mut body = sender.send_request(request).await.unwrap().into_body();

    let first = BodyExt::frame(&mut body)
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    let first_at = started.elapsed();
    assert!(first.starts_with(b"a"));
    assert!(
        first_at < Duration::from_millis(600),
        "first bytes took {first_at:?}"
    );
    let mut rest = first.to_vec();
    while let Some(frame) = BodyExt::frame(&mut body).await {
        if let Ok(data) = frame.unwrap().into_data() {
            rest.extend_from_slice(&data);
        }
    }
    assert!(started.elapsed() >= Duration::from_millis(650));
    assert!(rest.ends_with(b"done"));
    daemon.stop().await;
}

#[tokio::test]
async fn static_files_report_their_length() {
    let daemon = start().await;
    let reply = daemon.get("static-site.test", "/").await;
    assert_eq!(reply.headers["content-length"], "21");
    daemon.stop().await;
}
