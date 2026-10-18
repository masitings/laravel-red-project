//! The dump server (with a stand-in for symfony/var-dumper) and the log
//! viewer.

mod common;

use base64::Engine;
use red_api::client::Method;
use red_api::{DumpSettings, DumpsResponse, LogFile, LogKind, LogResponse, PathRequest, endpoints};
use tokio::io::AsyncWriteExt;

/// Just enough of VarDumper's API for the renderer script.
fn fake_var_dumper(project: &std::path::Path) {
    let dir = project.join("vendor/symfony/var-dumper");
    std::fs::create_dir_all(dir.join("Cloner")).unwrap();
    std::fs::create_dir_all(dir.join("Dumper")).unwrap();
    std::fs::write(
        dir.join("Cloner/Data.php"),
        "<?php namespace Symfony\\Component\\VarDumper\\Cloner; class Data { public $v; }",
    )
    .unwrap();
    std::fs::write(
        dir.join("Dumper/HtmlDumper.php"),
        "<?php namespace Symfony\\Component\\VarDumper\\Dumper; class HtmlDumper { \
         public function dump($data, $output = null) { return '<pre>'.htmlspecialchars($data->v).'</pre>'; } }",
    )
    .unwrap();
}

fn php_available() -> bool {
    !red_platform::binaries::in_path("php").is_empty()
}

#[tokio::test]
async fn renders_dumps_sent_by_var_dumper() {
    if !php_available() {
        eprintln!("php not installed; skipping");
        return;
    }
    let daemon = common::start().await;
    let project = daemon.home.path().join("app");
    fake_var_dumper(&project);
    let file = project.join("public/index.php").display().to_string();

    let dumps: DumpsResponse = daemon
        .client
        .call(
            Method::PUT,
            endpoints::DUMP_SETTINGS,
            Some(&DumpSettings { enabled: true }),
        )
        .await
        .unwrap();
    assert!(dumps.enabled);

    let class = "Symfony\\Component\\VarDumper\\Cloner\\Data";
    let payload = format!(
        "a:2:{{i:0;O:{}:\"{class}\":1:{{s:1:\"v\";s:5:\"hello\";}}i:1;a:2:{{s:9:\"timestamp\";d:1760000000.25;\
         s:6:\"source\";a:3:{{s:4:\"name\";s:9:\"index.php\";s:4:\"file\";s:{}:\"{file}\";s:4:\"line\";i:3;}}}}}}",
        class.len(),
        file.len()
    );
    let line = base64::engine::general_purpose::STANDARD.encode(payload) + "\n";
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", dumps.port.unwrap()))
        .await
        .unwrap();
    stream.write_all(line.as_bytes()).await.unwrap();
    stream.write_all(b"bm90IGEgZHVtcA==\n").await.unwrap(); // ignored
    drop(stream);

    let dumps = common::eventually(15, || async {
        let d: DumpsResponse = daemon.client.get(endpoints::DUMPS).await.ok()?;
        (!d.dumps.is_empty()).then_some(d)
    })
    .await;
    let dump = &dumps.dumps[0];
    assert_eq!(dump.html, "<pre>hello</pre>");
    assert_eq!(dump.received, 1_760_000_000_250);
    let source = dump.source.as_ref().unwrap();
    assert_eq!(source.file, file);
    assert_eq!(source.line, Some(3));

    let _: serde_json::Value = daemon
        .client
        .call(Method::DELETE, endpoints::DUMPS, None::<&()>)
        .await
        .unwrap();
    let dumps: DumpsResponse = daemon.client.get(endpoints::DUMPS).await.unwrap();
    assert!(dumps.dumps.is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn lists_and_reads_site_logs() {
    let daemon = common::start().await;
    let project = daemon.home.path().join("blog");
    std::fs::create_dir_all(project.join("storage/logs")).unwrap();
    std::fs::write(
        project.join("storage/logs/laravel.log"),
        "[2026-10-18 10:00:00] local.ERROR: Boom\n#0 trace\n[2026-10-18 10:00:01] local.INFO: ok\n",
    )
    .unwrap();
    let _: serde_json::Value = daemon
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

    let logs: Vec<LogFile> = daemon.client.get(endpoints::LOGS).await.unwrap();
    let laravel = logs
        .iter()
        .find(|l| l.kind == LogKind::Site)
        .expect("site log listed");
    assert_eq!(laravel.id, "site:blog:storage/logs/laravel.log");

    let read: LogResponse = daemon
        .client
        .get(&format!(
            "{}?id={}&lines=1",
            endpoints::LOG_READ,
            laravel.id
        ))
        .await
        .unwrap();
    assert_eq!(read.entries.len(), 1);
    assert!(read.truncated);
    assert_eq!(read.entries[0].level.as_deref(), Some("info"));

    let missing = daemon
        .client
        .get::<LogResponse>(&format!("{}?id=../../etc/passwd", endpoints::LOG_READ))
        .await;
    assert!(missing.is_err());
    daemon.stop().await;
}
