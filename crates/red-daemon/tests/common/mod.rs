//! A daemon on free ports in a temporary RED_HOME, for integration tests.

#![allow(dead_code)]

use std::time::Duration;

use red_api::StatusResponse;
use red_api::client::Client;
use red_daemon::DaemonError;
use red_platform::Paths;
use tokio::sync::oneshot;

pub struct Daemon {
    pub client: Client,
    pub status: StatusResponse,
    pub paths: Paths,
    stop: Option<oneshot::Sender<()>>,
    task: tokio::task::JoinHandle<Result<(), DaemonError>>,
    pub home: tempfile::TempDir,
}

/// Start a daemon with every port picked by the OS.
pub async fn start() -> Daemon {
    start_with("").await
}

/// Start a daemon with extra `config.toml` lines.
pub async fn start_with(extra_config: &str) -> Daemon {
    let home = tempfile::tempdir().unwrap();
    let paths = Paths::under(home.path().join("red"));
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.config_file(),
        format!(
            "http_port = 0\nhttps_port = 0\ndns_port = 0\nsmtp_port = 0\ndump_port = 0\n{extra_config}"
        ),
    )
    .unwrap();
    start_in(home, paths).await
}

pub async fn start_in(home: tempfile::TempDir, paths: Paths) -> Daemon {
    let (stop, stopped) = oneshot::channel::<()>();
    let task = tokio::spawn(red_daemon::run(paths.clone(), async {
        let _ = stopped.await;
    }));
    let client = Client::new(paths.socket());
    for _ in 0..300 {
        if let Ok(status) = client.status().await {
            return Daemon {
                client,
                status,
                paths,
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
    /// Stop the daemon and wait for it to exit.
    pub async fn stop(mut self) -> (tempfile::TempDir, Paths) {
        let _ = self.stop.take().unwrap().send(());
        self.task.await.unwrap().unwrap();
        (self.home, self.paths)
    }
}

/// Poll `check` until it returns `Some`, for up to `secs` seconds.
pub async fn eventually<T, F, Fut>(secs: u64, mut check: F) -> T
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Option<T>>,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(secs);
    loop {
        if let Some(value) = check().await {
            return value;
        }
        assert!(tokio::time::Instant::now() < deadline, "timed out");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
