//! Services: adding, starting, stopping and autostarting a real Redis.
//! Skipped when `redis-server` is not installed.

mod common;

use red_api::client::Method;
use red_api::{
    ServiceInfo, ServiceKind, ServiceRequest, ServiceStatus, ServicesResponse, endpoints,
};

fn redis_server() -> Option<String> {
    red_platform::binaries::server_candidates(
        red_platform::binaries::Server::Redis,
        std::path::Path::new("/nonexistent"),
    )
    .into_iter()
    .next()
    .map(|p| p.display().to_string())
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

async fn status(daemon: &common::Daemon, name: &str, want: ServiceStatus) -> ServiceInfo {
    common::eventually(60, || async {
        let s: ServiceInfo = daemon.client.get(&endpoints::service(name)).await.ok()?;
        assert!(
            want == ServiceStatus::Failed || s.status != ServiceStatus::Failed,
            "{name} failed: {:?}",
            s.error
        );
        (s.status == want).then_some(s)
    })
    .await
}

#[tokio::test]
async fn runs_redis_as_a_service() {
    let Some(binary) = redis_server() else {
        eprintln!("redis-server not installed; skipping");
        return;
    };
    let daemon = common::start().await;
    let port = free_port();
    let added: ServiceInfo = daemon
        .client
        .call(
            Method::POST,
            endpoints::SERVICES,
            Some(&ServiceRequest {
                kind: ServiceKind::Redis,
                name: Some("cache".into()),
                port: Some(port),
                version: None,
                binary: Some(binary),
                autostart: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(added.name, "cache");
    assert!(
        added
            .env
            .iter()
            .any(|v| v.name == "REDIS_PORT" && v.value == port.to_string())
    );

    let running = status(&daemon, "cache", ServiceStatus::Running).await;
    assert!(running.pid.is_some());
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());

    // A second service on the same port is refused.
    let err = daemon
        .client
        .call::<_, ServiceInfo>(
            Method::POST,
            endpoints::SERVICES,
            Some(&ServiceRequest {
                kind: ServiceKind::Redis,
                name: Some("other".into()),
                port: Some(port),
                version: None,
                binary: None,
                autostart: None,
            }),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("already uses port"), "{err}");

    let _: ServiceInfo = daemon
        .client
        .call(
            Method::POST,
            &endpoints::service_action("cache", "stop"),
            None::<&()>,
        )
        .await
        .unwrap();
    status(&daemon, "cache", ServiceStatus::Stopped).await;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());

    // Autostart brings it back after a daemon restart.
    let (home, paths) = daemon.stop().await;
    let daemon = common::start_in(home, paths).await;
    status(&daemon, "cache", ServiceStatus::Running).await;
    let list: ServicesResponse = daemon.client.get(endpoints::SERVICES).await.unwrap();
    assert!(list.available.iter().any(|i| i.kind == ServiceKind::Redis));

    let _: serde_json::Value = daemon
        .client
        .call(
            Method::DELETE,
            &format!("{}?delete_data=true", endpoints::service("cache")),
            None::<&()>,
        )
        .await
        .unwrap();
    assert!(!daemon.paths.service_dir("cache").exists());
    daemon.stop().await;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[tokio::test]
async fn unknown_binary_is_rejected() {
    let daemon = common::start().await;
    let err = daemon
        .client
        .call::<_, ServiceInfo>(
            Method::POST,
            endpoints::SERVICES,
            Some(&ServiceRequest {
                kind: ServiceKind::Postgres,
                name: None,
                port: Some(free_port()),
                version: None,
                binary: Some("/bin/true".into()),
                autostart: None,
            }),
        )
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("is not a postgres server"),
        "{err}"
    );
    daemon.stop().await;
}
