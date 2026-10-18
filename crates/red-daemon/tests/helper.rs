//! redd takes its web listeners from red-helper when one is configured, and
//! installs the resolver file through it.

use std::time::Duration;

use red_api::client::{Client, Method};
use red_api::endpoints;
use red_helper::server;
use red_platform::Paths;
use tokio::sync::oneshot;

#[tokio::test]
async fn redd_uses_the_helper_for_ports_and_dns() {
    let home = tempfile::tempdir().unwrap();

    // A helper that hands out ephemeral ports instead of 80/443.
    let socket = home.path().join("helper.sock");
    let mut config = server::Config::new(socket.clone(), vec![rustix_uid()]);
    config.http_port = 0;
    config.https_port = 0;
    config.resolver_dir = home.path().join("resolver");
    let listener = server::bind(&socket).unwrap();
    tokio::spawn(server::serve(listener, config));

    let paths = Paths::under(home.path().join("red"));
    std::fs::create_dir_all(&paths.config_dir).unwrap();
    std::fs::write(
        paths.config_file(),
        format!(
            "dns_port = 0\nhelper_socket = {:?}\n",
            socket.display().to_string()
        ),
    )
    .unwrap();
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
    let status = status.expect("redd started");
    assert!(status.helper, "ports should come from the helper");
    let port = status.http_port.unwrap();
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
    );
    assert!(status.https_port.is_some());

    let _: serde_json::Value = client
        .call(Method::POST, endpoints::DNS, None::<&()>)
        .await
        .unwrap();
    let written: Vec<_> = std::fs::read_dir(home.path().join("resolver"))
        .unwrap()
        .collect();
    assert_eq!(written.len(), 1);
    let _: serde_json::Value = client
        .call(Method::DELETE, endpoints::DNS, None::<&()>)
        .await
        .unwrap();

    let _ = stop.send(());
    task.await.unwrap().unwrap();
}

fn rustix_uid() -> u32 {
    red_platform::user::current_uid()
}
