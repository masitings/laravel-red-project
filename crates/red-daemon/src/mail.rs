//! The mail catcher: a small SMTP server on 127.0.0.1 that accepts every
//! message and keeps it for inspection instead of delivering it.
//!
//! Messages are stored as `<data>/mail/<id>.eml` with a JSON sidecar
//! (`<id>.json`) holding the summary, so listing does not re-parse mail.
//! The SMTP username an app logs in with becomes the message's mailbox,
//! which tells apps apart (Laravel sends `MAIL_USERNAME`).

use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use mail_parser::{MessageParser, MimeHeaders, PartType};
use red_api::{Event, MailAttachment, MailHeader, MailMessage, MailSummary};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt as _, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

/// Largest message accepted (advertised with `SIZE`).
pub const MAX_MESSAGE: usize = 25 * 1024 * 1024;
/// Messages kept; older ones are deleted when new mail arrives.
pub const KEEP_MESSAGES: usize = 1000;
/// Longest command line accepted.
const MAX_LINE: usize = 4096;
/// A client idle for this long is disconnected.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
/// Inline images up to this size are embedded into the HTML preview.
const MAX_INLINE_IMAGE: usize = 5 * 1024 * 1024;

/// What the sidecar file holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Stored {
    summary: MailSummary,
    envelope_from: String,
    envelope_to: Vec<String>,
}

/// All caught mail, newest first in memory.
#[derive(Debug)]
pub struct MailStore {
    dir: PathBuf,
    index: RwLock<Vec<Stored>>,
    counter: AtomicU32,
}

impl MailStore {
    /// Load the index from `dir` (created on first message).
    pub fn open(dir: PathBuf) -> Self {
        let mut index = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "json")
                    && let Ok(bytes) = std::fs::read(&path)
                    && let Ok(stored) = serde_json::from_slice::<Stored>(&bytes)
                    && dir.join(format!("{}.eml", stored.summary.id)).exists()
                {
                    index.push(stored);
                }
            }
        }
        index.sort_by(|a, b| b.summary.id.cmp(&a.summary.id));
        Self {
            dir,
            index: RwLock::new(index),
            counter: AtomicU32::new(0),
        }
    }

    fn new_id(&self) -> String {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let n = self.counter.fetch_add(1, Ordering::Relaxed) % 0x10000;
        format!("{millis:013}-{n:04x}")
    }

    fn path(&self, id: &str, ext: &str) -> Option<PathBuf> {
        valid_id(id).then(|| self.dir.join(format!("{id}.{ext}")))
    }

    /// Store a received message and return its summary.
    pub fn save(
        &self,
        raw: &[u8],
        mailbox: Option<String>,
        envelope_from: String,
        envelope_to: Vec<String>,
    ) -> std::io::Result<MailSummary> {
        std::fs::create_dir_all(&self.dir)?;
        let id = self.new_id();
        let summary = summarise(&id, raw, mailbox, &envelope_from, &envelope_to);
        let stored = Stored {
            summary: summary.clone(),
            envelope_from,
            envelope_to,
        };
        std::fs::write(self.dir.join(format!("{id}.eml")), raw)?;
        write_json(&self.dir.join(format!("{id}.json")), &stored)?;
        let removed: Vec<Stored> = {
            let mut index = self.index.write().expect("mail index poisoned");
            index.insert(0, stored);
            if index.len() > KEEP_MESSAGES {
                index.split_off(KEEP_MESSAGES)
            } else {
                Vec::new()
            }
        };
        for old in removed {
            self.remove_files(&old.summary.id);
        }
        Ok(summary)
    }

    /// Summaries, newest first, optionally only one mailbox.
    pub fn list(&self, mailbox: Option<&str>) -> Vec<MailSummary> {
        self.index
            .read()
            .expect("mail index poisoned")
            .iter()
            .filter(|s| mailbox.is_none_or(|m| s.summary.mailbox.as_deref() == Some(m)))
            .map(|s| s.summary.clone())
            .collect()
    }

    /// The raw message.
    pub fn raw(&self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.path(id, "eml")?).ok()
    }

    /// The parsed message. Marks it as read; returns whether that changed.
    pub fn message(&self, id: &str) -> Option<(MailMessage, bool)> {
        let raw = self.raw(id)?;
        let mut was_unread = false;
        let stored = {
            let mut index = self.index.write().expect("mail index poisoned");
            let stored = index.iter_mut().find(|s| s.summary.id == id)?;
            if !stored.summary.read {
                stored.summary.read = true;
                was_unread = true;
                if let Some(path) = self.path(id, "json") {
                    let _ = write_json(&path, stored);
                }
            }
            stored.clone()
        };
        Some((details(&raw, stored), was_unread))
    }

    /// Attachment `index` of message `id`: file name, content type, bytes.
    pub fn attachment(&self, id: &str, index: u32) -> Option<(String, String, Vec<u8>)> {
        let raw = self.raw(id)?;
        let message = MessageParser::default().parse(&raw)?;
        let part = message.attachment(index)?;
        Some((
            attachment_name(part, index),
            content_type(part),
            part.contents().to_vec(),
        ))
    }

    /// Delete one message. Returns `false` when it did not exist.
    pub fn delete(&self, id: &str) -> bool {
        let removed = {
            let mut index = self.index.write().expect("mail index poisoned");
            let before = index.len();
            index.retain(|s| s.summary.id != id);
            index.len() != before
        };
        if removed {
            self.remove_files(id);
        }
        removed
    }

    /// Delete every message (of one mailbox). Returns how many were deleted.
    pub fn clear(&self, mailbox: Option<&str>) -> usize {
        let removed: Vec<String> = {
            let mut index = self.index.write().expect("mail index poisoned");
            let (gone, kept): (Vec<Stored>, Vec<Stored>) = index
                .drain(..)
                .partition(|s| mailbox.is_none_or(|m| s.summary.mailbox.as_deref() == Some(m)));
            *index = kept;
            gone.into_iter().map(|s| s.summary.id).collect()
        };
        for id in &removed {
            self.remove_files(id);
        }
        removed.len()
    }

    fn remove_files(&self, id: &str) {
        for ext in ["eml", "json"] {
            if let Some(path) = self.path(id, ext) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Ids are generated by [`MailStore::new_id`]; anything else is refused so
/// an id can never name a file outside the mail directory.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn write_json(path: &Path, value: &impl Serialize) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(value).expect("serialisable"))?;
    std::fs::rename(tmp, path)
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn format_addresses(address: Option<&mail_parser::Address<'_>>) -> Vec<String> {
    let Some(address) = address else {
        return Vec::new();
    };
    address
        .iter()
        .filter_map(|a| match (a.name(), a.address()) {
            (Some(name), Some(addr)) if !name.is_empty() => Some(format!("{name} <{addr}>")),
            (_, Some(addr)) => Some(addr.to_owned()),
            (Some(name), None) => Some(name.to_owned()),
            (None, None) => None,
        })
        .collect()
}

fn summarise(
    id: &str,
    raw: &[u8],
    mailbox: Option<String>,
    envelope_from: &str,
    envelope_to: &[String],
) -> MailSummary {
    let parsed = MessageParser::default().parse(raw);
    let (from, to, subject, has_html, attachments) = match &parsed {
        Some(m) => (
            format_addresses(m.from())
                .into_iter()
                .next()
                .unwrap_or_else(|| envelope_from.to_owned()),
            {
                let to = format_addresses(m.to());
                if to.is_empty() {
                    envelope_to.to_vec()
                } else {
                    to
                }
            },
            m.subject().unwrap_or_default().to_owned(),
            m.html_bodies().any(|p| matches!(p.body, PartType::Html(_))),
            u32::try_from(m.attachment_count()).unwrap_or(u32::MAX),
        ),
        None => (
            envelope_from.to_owned(),
            envelope_to.to_vec(),
            String::new(),
            false,
            0,
        ),
    };
    MailSummary {
        id: id.to_owned(),
        mailbox,
        from,
        to,
        subject,
        received: now(),
        size: raw.len() as u64,
        has_html,
        attachments,
        read: false,
    }
}

fn content_type(part: &mail_parser::MessagePart<'_>) -> String {
    part.content_type()
        .map(|ct| match ct.subtype() {
            Some(sub) => format!("{}/{}", ct.ctype(), sub),
            None => ct.ctype().to_owned(),
        })
        .unwrap_or_else(|| "application/octet-stream".into())
}

fn attachment_name(part: &mail_parser::MessagePart<'_>, index: u32) -> String {
    part.attachment_name()
        .map(|n| n.replace(['/', '\\'], "_"))
        .unwrap_or_else(|| format!("attachment-{}", index + 1))
}

fn details(raw: &[u8], stored: Stored) -> MailMessage {
    let Some(message) = MessageParser::default().parse(raw) else {
        return MailMessage {
            summary: stored.summary,
            cc: Vec::new(),
            envelope_to: stored.envelope_to,
            text: Some(String::from_utf8_lossy(raw).into_owned()),
            html: None,
            headers: Vec::new(),
            attachments: Vec::new(),
        };
    };
    let headers = message
        .headers()
        .iter()
        .map(|h| {
            let start = h.offset_start() as usize;
            let end = (h.offset_end() as usize).min(raw.len());
            let value = raw.get(start..end).unwrap_or_default();
            MailHeader {
                name: h.name().to_owned(),
                value: unfold(&String::from_utf8_lossy(value)),
            }
        })
        .collect();
    let text = message.text_bodies().find_map(|p| match &p.body {
        PartType::Text(text) => Some(text.to_string()),
        _ => None,
    });
    let mut html = message.html_bodies().find_map(|p| match &p.body {
        PartType::Html(html) => Some(html.to_string()),
        _ => None,
    });
    let mut attachments = Vec::new();
    for (index, part) in message.attachments().enumerate() {
        let index = u32::try_from(index).unwrap_or(u32::MAX);
        let content_type = content_type(part);
        // Show inline images (`cid:`) in the HTML preview.
        if let (Some(body), Some(cid)) = (html.as_mut(), part.content_id()) {
            let cid = cid.trim_matches(|c| c == '<' || c == '>');
            let reference = format!("cid:{cid}");
            if body.contains(&reference)
                && content_type.starts_with("image/")
                && part.contents().len() <= MAX_INLINE_IMAGE
            {
                let data = format!(
                    "data:{content_type};base64,{}",
                    BASE64.encode(part.contents())
                );
                *body = body.replace(&reference, &data);
            }
        }
        attachments.push(MailAttachment {
            index,
            filename: attachment_name(part, index),
            content_type,
            size: part.contents().len() as u64,
        });
    }
    MailMessage {
        cc: format_addresses(message.cc()),
        summary: stored.summary,
        envelope_to: stored.envelope_to,
        text,
        html,
        headers,
        attachments,
    }
}

/// Undo header folding (`\r\n ` continuation lines).
fn unfold(value: &str) -> String {
    value
        .split(['\r', '\n'])
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- SMTP ------------------------------------------------------------------------

/// Bind the SMTP listener on 127.0.0.1.
pub async fn bind(port: u16) -> std::io::Result<(TcpListener, u16)> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let port = listener.local_addr()?.port();
    Ok((listener, port))
}

/// Accept SMTP connections until cancelled.
pub async fn serve(
    listener: TcpListener,
    store: std::sync::Arc<MailStore>,
    events: tokio::sync::broadcast::Sender<Event>,
    cancel: CancellationToken,
) {
    loop {
        let (stream, _) = tokio::select! {
            () = cancel.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(err) => {
                    tracing::warn!(%err, "SMTP accept failed");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        let store = store.clone();
        let events = events.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move {
            tokio::select! {
                () = cancel.cancelled() => {}
                result = session(stream, &store, &events) => {
                    if let Err(err) = result {
                        tracing::debug!(%err, "SMTP session ended");
                    }
                }
            }
        });
    }
}

struct Session {
    mailbox: Option<String>,
    from: Option<String>,
    to: Vec<String>,
}

async fn session(
    stream: TcpStream,
    store: &MailStore,
    events: &tokio::sync::broadcast::Sender<Event>,
) -> std::io::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut state = Session {
        mailbox: None,
        from: None,
        to: Vec::new(),
    };
    write
        .write_all(b"220 red.local ESMTP RED Project mail catcher\r\n")
        .await?;
    loop {
        let Some(line) = read_line(&mut reader).await? else {
            return Ok(());
        };
        let (verb, rest) = match line.split_once(' ') {
            Some((verb, rest)) => (verb.to_ascii_uppercase(), rest.trim()),
            None => (line.to_ascii_uppercase(), ""),
        };
        let reply: String = match verb.as_str() {
            "EHLO" => format!(
                "250-red.local\r\n250-SIZE {MAX_MESSAGE}\r\n250-8BITMIME\r\n250-SMTPUTF8\r\n\
                 250-PIPELINING\r\n250-AUTH PLAIN LOGIN\r\n250 HELP\r\n"
            ),
            "HELO" => "250 red.local\r\n".into(),
            "AUTH" => {
                state.mailbox = authenticate(rest, &mut reader, &mut write).await?;
                "235 2.7.0 Authentication successful\r\n".into()
            }
            "MAIL" => match address_arg(rest, "FROM:") {
                Some(from) => {
                    state.from = Some(from);
                    state.to.clear();
                    "250 2.1.0 Ok\r\n".into()
                }
                None => "501 5.5.4 Syntax: MAIL FROM:<address>\r\n".into(),
            },
            "RCPT" => match (&state.from, address_arg(rest, "TO:")) {
                (None, _) => "503 5.5.1 Send MAIL first\r\n".into(),
                (_, None) => "501 5.5.4 Syntax: RCPT TO:<address>\r\n".into(),
                (Some(_), Some(to)) => {
                    state.to.push(to);
                    "250 2.1.5 Ok\r\n".into()
                }
            },
            "DATA" => {
                if state.to.is_empty() {
                    "503 5.5.1 Send RCPT first\r\n".into()
                } else {
                    write
                        .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                        .await?;
                    match read_data(&mut reader).await? {
                        Some(raw) => {
                            let from = state.from.take().unwrap_or_default();
                            let to = std::mem::take(&mut state.to);
                            match store.save(&raw, state.mailbox.clone(), from, to) {
                                Ok(summary) => {
                                    tracing::info!(id = %summary.id, subject = %summary.subject, "mail caught");
                                    let _ = events.send(Event::MailReceived {
                                        id: summary.id.clone(),
                                    });
                                    format!("250 2.0.0 Ok: queued as {}\r\n", summary.id)
                                }
                                Err(err) => {
                                    tracing::error!(%err, "could not store mail");
                                    "451 4.3.0 Could not store the message\r\n".into()
                                }
                            }
                        }
                        None => {
                            state.from = None;
                            state.to.clear();
                            "552 5.3.4 Message too big\r\n".into()
                        }
                    }
                }
            }
            "RSET" => {
                state.from = None;
                state.to.clear();
                "250 2.0.0 Ok\r\n".into()
            }
            "NOOP" => "250 2.0.0 Ok\r\n".into(),
            "VRFY" => "252 2.5.2 Cannot verify, but will accept\r\n".into(),
            "QUIT" => {
                write.write_all(b"221 2.0.0 Bye\r\n").await?;
                return Ok(());
            }
            "" => continue,
            _ => "502 5.5.2 Command not implemented\r\n".into(),
        };
        write.write_all(reply.as_bytes()).await?;
    }
}

/// Read one command line without its line ending. `None` at end of input.
async fn read_line<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> std::io::Result<Option<String>> {
    let mut buf = Vec::new();
    let read = tokio::time::timeout(IDLE_TIMEOUT, async {
        let mut limited = (&mut *reader).take(MAX_LINE as u64);
        limited.read_until(b'\n', &mut buf).await
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"))??;
    if read == 0 {
        return Ok(None);
    }
    if !buf.ends_with(b"\n") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "line too long",
        ));
    }
    Ok(Some(
        String::from_utf8_lossy(&buf)
            .trim_end_matches(['\r', '\n'])
            .to_owned(),
    ))
}

/// Read the message after `DATA` up to the lone `.` line, undoing dot
/// stuffing. `None` when it is larger than [`MAX_MESSAGE`] (the rest is
/// still consumed so the session can continue).
async fn read_data<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> std::io::Result<Option<Vec<u8>>> {
    let mut message = Vec::new();
    let mut too_big = false;
    loop {
        let mut line = Vec::new();
        let read = tokio::time::timeout(IDLE_TIMEOUT, reader.read_until(b'\n', &mut line))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "idle"))??;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        if line == b".\r\n" || line == b".\n" {
            break;
        }
        let line = if line.starts_with(b".") {
            &line[1..]
        } else {
            &line[..]
        };
        if message.len() + line.len() > MAX_MESSAGE {
            too_big = true;
        }
        if !too_big {
            message.extend_from_slice(line);
        }
    }
    Ok((!too_big).then_some(message))
}

/// `FROM:<a@b> SIZE=1` → `a@b`.
fn address_arg(rest: &str, prefix: &str) -> Option<String> {
    let rest = rest.trim();
    if rest.len() < prefix.len() || !rest[..prefix.len()].eq_ignore_ascii_case(prefix) {
        return None;
    }
    let value = rest[prefix.len()..].trim_start();
    let address = match value.strip_prefix('<') {
        Some(inner) => inner.split('>').next()?,
        None => value.split_whitespace().next().unwrap_or_default(),
    };
    Some(address.to_owned())
}

/// Handle `AUTH PLAIN` / `AUTH LOGIN`, accepting any credentials. Returns
/// the username, which becomes the mailbox.
async fn authenticate<R, W>(
    rest: &str,
    reader: &mut R,
    write: &mut W,
) -> std::io::Result<Option<String>>
where
    R: AsyncBufReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    let (mechanism, initial) = match rest.split_once(' ') {
        Some((m, i)) => (m.to_ascii_uppercase(), Some(i.trim().to_owned())),
        None => (rest.to_ascii_uppercase(), None),
    };
    let decode = |text: &str| {
        BASE64
            .decode(text.trim())
            .ok()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let mut ask = async |prompt: &[u8]| -> std::io::Result<String> {
        write.write_all(prompt).await?;
        Ok(read_line(reader).await?.unwrap_or_default())
    };
    let user = match mechanism.as_str() {
        "PLAIN" => {
            let response = match initial {
                Some(i) => i,
                None => ask(b"334 \r\n").await?,
            };
            // authzid NUL authcid NUL password
            decode(&response).and_then(|text| text.split('\0').nth(1).map(str::to_owned))
        }
        "LOGIN" => {
            let user = match initial {
                Some(i) => i,
                None => ask(b"334 VXNlcm5hbWU6\r\n").await?,
            };
            let _password = ask(b"334 UGFzc3dvcmQ6\r\n").await?;
            decode(&user)
        }
        _ => None,
    };
    Ok(user.filter(|u| !u.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_envelope_addresses() {
        assert_eq!(
            address_arg("FROM:<a@b.test> SIZE=10", "FROM:"),
            Some("a@b.test".into())
        );
        assert_eq!(address_arg("to: <x@y>", "TO:"), Some("x@y".into()));
        assert_eq!(address_arg("FROM:<>", "FROM:"), Some(String::new()));
        assert_eq!(address_arg("bogus", "TO:"), None);
    }

    #[test]
    fn rejects_ids_that_are_paths() {
        assert!(valid_id("0001760800000000-00af"));
        assert!(!valid_id("../state"));
        assert!(!valid_id(""));
    }

    #[test]
    fn stores_lists_and_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let store = MailStore::open(dir.path().join("mail"));
        let raw = b"From: App <app@example.test>\r\nTo: you@example.test\r\nSubject: Hi\r\n\
            MIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=b\r\n\r\n\
            --b\r\nContent-Type: text/plain\r\n\r\nHello\r\n--b\r\nContent-Type: text/html\r\n\r\n\
            <p>Hello</p>\r\n--b--\r\n";
        let summary = store
            .save(
                raw,
                Some("blog".into()),
                "app@example.test".into(),
                vec!["you@example.test".into(), "bcc@example.test".into()],
            )
            .unwrap();
        assert_eq!(summary.subject, "Hi");
        assert_eq!(summary.from, "App <app@example.test>");
        assert!(summary.has_html);
        assert!(!summary.read);

        // A fresh store finds it on disk.
        let store = MailStore::open(dir.path().join("mail"));
        assert_eq!(store.list(Some("blog")).len(), 1);
        assert!(store.list(Some("other")).is_empty());

        let (message, changed) = store.message(&summary.id).unwrap();
        assert!(changed);
        assert_eq!(message.text.as_deref().map(str::trim), Some("Hello"));
        assert!(message.html.unwrap().contains("<p>Hello</p>"));
        assert_eq!(message.envelope_to.len(), 2);
        assert!(
            message
                .headers
                .iter()
                .any(|h| h.name == "Subject" && h.value == "Hi")
        );
        assert!(store.list(None)[0].read);

        assert!(store.delete(&summary.id));
        assert!(!store.delete(&summary.id));
        assert!(store.list(None).is_empty());
    }
}
