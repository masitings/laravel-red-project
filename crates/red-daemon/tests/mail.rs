//! The mail catcher: SMTP in, Control API out.

mod common;

use red_api::client::Method;
use red_api::{MailMessage, MailSummary, endpoints};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

async fn expect(reader: &mut BufReader<tokio::net::tcp::OwnedReadHalf>, code: &str) -> String {
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert!(line.starts_with(code), "expected {code}, got {line:?}");
        // Multi-line replies use `250-`; the last line uses `250 `.
        if line.as_bytes().get(3) != Some(&b'-') {
            return line;
        }
    }
}

#[tokio::test]
async fn catches_mail_over_smtp() {
    let daemon = common::start().await;
    let port = daemon.status.smtp_port.expect("mail catcher runs");

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    expect(&mut reader, "220").await;
    write.write_all(b"EHLO laravel\r\n").await.unwrap();
    expect(&mut reader, "250").await;
    // AUTH PLAIN with user "blog", password "secret".
    write
        .write_all(b"AUTH PLAIN AGJsb2cAc2VjcmV0\r\n")
        .await
        .unwrap();
    expect(&mut reader, "235").await;
    write
        .write_all(b"MAIL FROM:<app@blog.test>\r\n")
        .await
        .unwrap();
    expect(&mut reader, "250").await;
    write
        .write_all(b"RCPT TO:<user@example.test>\r\n")
        .await
        .unwrap();
    expect(&mut reader, "250").await;
    write
        .write_all(b"RCPT TO:<hidden@example.test>\r\n")
        .await
        .unwrap();
    expect(&mut reader, "250").await;
    write.write_all(b"DATA\r\n").await.unwrap();
    expect(&mut reader, "354").await;
    write
        .write_all(
            b"From: Blog <app@blog.test>\r\nTo: user@example.test\r\nSubject: Welcome\r\n\
              Content-Type: text/plain\r\n\r\nHi there\r\n..leading dot\r\n.\r\n",
        )
        .await
        .unwrap();
    let queued = expect(&mut reader, "250").await;
    write.write_all(b"QUIT\r\n").await.unwrap();
    expect(&mut reader, "221").await;

    let list: Vec<MailSummary> = daemon.client.get(endpoints::MAIL).await.unwrap();
    assert_eq!(list.len(), 1);
    let summary = &list[0];
    assert!(queued.contains(&summary.id));
    assert_eq!(summary.mailbox.as_deref(), Some("blog"));
    assert_eq!(summary.subject, "Welcome");
    assert!(!summary.read);

    let filtered: Vec<MailSummary> = daemon
        .client
        .get(&format!("{}?mailbox=other", endpoints::MAIL))
        .await
        .unwrap();
    assert!(filtered.is_empty());

    let message: MailMessage = daemon
        .client
        .get(&endpoints::mail(&summary.id))
        .await
        .unwrap();
    let text = message.text.unwrap();
    assert!(text.contains("Hi there"));
    assert!(
        text.contains("\n.leading dot"),
        "dot stuffing undone: {text:?}"
    );
    assert_eq!(
        message.envelope_to,
        ["user@example.test", "hidden@example.test"]
    );
    assert!(message.summary.read);

    let _: serde_json::Value = daemon
        .client
        .call(Method::DELETE, endpoints::MAIL, None::<&()>)
        .await
        .unwrap();
    let list: Vec<MailSummary> = daemon.client.get(endpoints::MAIL).await.unwrap();
    assert!(list.is_empty());
    daemon.stop().await;
}

#[tokio::test]
async fn rejects_commands_out_of_order() {
    let daemon = common::start().await;
    let port = daemon.status.smtp_port.unwrap();
    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    expect(&mut reader, "220").await;
    write.write_all(b"RCPT TO:<a@b>\r\n").await.unwrap();
    expect(&mut reader, "503").await;
    write.write_all(b"DATA\r\n").await.unwrap();
    expect(&mut reader, "503").await;
    write.write_all(b"STARTTLS\r\n").await.unwrap();
    expect(&mut reader, "502").await;
    daemon.stop().await;
}
