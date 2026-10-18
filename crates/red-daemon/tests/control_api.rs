//! End-to-end test: start `redd` in a temporary RED_HOME and talk to it
//! through the real unix socket with the `red-api` client.

use std::time::Duration;

use red_api::client::{Client, ClientError};
use red_api::{API_VERSION, ErrorCode};
use red_daemon::DaemonError;
use red_platform::Paths;
use tokio::sync::oneshot;

struct Running {
    paths: Paths,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), DaemonError>>,
    _home: tempfile::TempDir,
}

/// Paths under `home`, with a config that lets the OS pick the web port so
/// tests can run in parallel (and as root) without fighting over port 80.
fn test_paths(home: &std::path::Path) -> Paths {
    let paths = Paths::under(home);
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.config_file(),
        "http_port = 0\nhttps_port = 0\ndns_port = 0\n",
    )
    .unwrap();
    paths
}

async fn start() -> Running {
    let home = tempfile::tempdir().unwrap();
    let paths = test_paths(home.path());
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(red_daemon::run(paths.clone(), async {
        let _ = stopped.await;
    }));

    let client = Client::new(paths.socket());
    for _ in 0..100 {
        if client.status().await.is_ok() {
            return Running {
                paths,
                stop: Some(stop),
                task,
                _home: home,
            };
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("redd did not come up");
}

impl Running {
    async fn stop(mut self) {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap().unwrap();
    }
}

#[tokio::test]
async fn status_reports_daemon_details() {
    let daemon = start().await;
    let status = Client::new(daemon.paths.socket()).status().await.unwrap();

    assert_eq!(status.api_version, API_VERSION);
    assert_eq!(status.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(status.tld, "test");
    assert_eq!(status.pid, std::process::id());
    assert_eq!(
        status.paths.socket,
        daemon.paths.socket().display().to_string()
    );

    daemon.stop().await;
}

#[tokio::test]
async fn unknown_route_returns_json_error() {
    let daemon = start().await;
    let err = Client::new(daemon.paths.socket())
        .get::<serde_json::Value>("/v1/nope")
        .await
        .unwrap_err();

    match err {
        ClientError::Api { status, error } => {
            assert_eq!(status, 404);
            assert_eq!(error.code, ErrorCode::NotFound);
        }
        other => panic!("unexpected error: {other:?}"),
    }
    daemon.stop().await;
}

#[tokio::test]
async fn second_daemon_refuses_to_start() {
    let daemon = start().await;
    let err = red_daemon::run(daemon.paths.clone(), std::future::pending())
        .await
        .unwrap_err();
    assert!(matches!(err, DaemonError::AlreadyRunning(_)), "{err:?}");
    daemon.stop().await;
}

#[tokio::test]
async fn socket_is_private_and_removed_on_shutdown() {
    use std::os::unix::fs::PermissionsExt;

    let daemon = start().await;
    let socket = daemon.paths.socket();
    let mode = std::fs::metadata(&socket).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let dir_mode = std::fs::metadata(&daemon.paths.run_dir)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(dir_mode & 0o777, 0o700);

    daemon.stop().await;
    assert!(!socket.exists());
}

#[tokio::test]
async fn stale_socket_is_replaced() {
    let home = tempfile::tempdir().unwrap();
    let paths = test_paths(home.path());
    std::fs::create_dir_all(&paths.run_dir).unwrap();
    // A socket file nobody listens on, as left behind by a crash.
    drop(std::os::unix::net::UnixListener::bind(paths.socket()).unwrap());
    assert!(paths.socket().exists());

    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(red_daemon::run(paths.clone(), async {
        let _ = stopped.await;
    }));
    let client = Client::new(paths.socket());
    let mut up = false;
    for _ in 0..100 {
        if client.status().await.is_ok() {
            up = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(up, "redd did not replace the stale socket");
    let _ = stop.send(());
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn client_reports_daemon_not_running() {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::under(home.path());
    let err = Client::new(paths.socket()).status().await.unwrap_err();
    assert!(
        matches!(err, ClientError::DaemonNotRunning { .. }),
        "{err:?}"
    );
}
