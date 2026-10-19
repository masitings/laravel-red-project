//! `red mail*`, `red dumps*` and `red logs`: looking at what apps sent.

use red_api::client::Method;
use red_api::{
    DumpEntry, DumpSettings, DumpsResponse, LogEntry, LogFile, LogResponse, MailMessage,
    MailSummary, endpoints,
};

use crate::{Ctx, print_table};

// ---- mail ------------------------------------------------------------------------

pub(crate) async fn mail_list(ctx: &Ctx<'_>, mailbox: Option<&str>) -> anyhow::Result<()> {
    let path = match mailbox {
        Some(m) => format!("{}?mailbox={}", endpoints::MAIL, encode(m)),
        None => endpoints::MAIL.to_owned(),
    };
    let list: Vec<MailSummary> = ctx.client.get(&path).await?;
    ctx.output(&list, |list| {
        if list.is_empty() {
            println!("No mail yet. Point your app at 127.0.0.1 with MAIL_PORT from `red status`.");
            return;
        }
        let rows: Vec<[String; 5]> = list
            .iter()
            .map(|m| {
                [
                    format!("{}{}", if m.read { " " } else { "*" }, m.id),
                    red_core::time::datetime_from_unix(m.received),
                    m.mailbox.clone().unwrap_or_else(|| "-".into()),
                    m.to.join(", "),
                    m.subject.clone(),
                ]
            })
            .collect();
        print_table(&[" ID", "RECEIVED", "MAILBOX", "TO", "SUBJECT"], &rows);
    })
}

pub(crate) async fn mail_show(ctx: &Ctx<'_>, id: &str) -> anyhow::Result<()> {
    let message: MailMessage = ctx.client.get(&endpoints::mail(id)).await?;
    ctx.output(&message, |m| {
        println!("From:    {}", m.summary.from);
        println!("To:      {}", m.summary.to.join(", "));
        if !m.cc.is_empty() {
            println!("Cc:      {}", m.cc.join(", "));
        }
        println!("Subject: {}", m.summary.subject);
        println!(
            "Date:    {}",
            red_core::time::datetime_from_unix(m.summary.received)
        );
        for a in &m.attachments {
            println!(
                "Attach:  {} ({}, {} bytes)",
                a.filename, a.content_type, a.size
            );
        }
        println!();
        match (&m.text, &m.html) {
            (Some(text), _) => println!("{}", text.trim_end()),
            (None, Some(html)) => println!("{}", html_to_text(html)),
            (None, None) => println!("(no body)"),
        }
    })
}

pub(crate) async fn mail_clear(ctx: &Ctx<'_>, mailbox: Option<&str>) -> anyhow::Result<()> {
    let path = match mailbox {
        Some(m) => format!("{}?mailbox={}", endpoints::MAIL, encode(m)),
        None => endpoints::MAIL.to_owned(),
    };
    let _: serde_json::Value = ctx.send(Method::DELETE, &path, None::<&()>).await?;
    ctx.output(&serde_json::json!({ "cleared": true }), |_| {
        println!("Deleted the caught mail.");
    })
}

// ---- dumps -----------------------------------------------------------------------

pub(crate) async fn dumps(ctx: &Ctx<'_>, follow: bool) -> anyhow::Result<()> {
    let response: DumpsResponse = ctx.client.get(endpoints::DUMPS).await?;
    if ctx.cli.json && !follow {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }
    if !response.enabled {
        eprintln!("Dump capturing is off; turn it on with `red dumps:on`.");
    }
    for dump in &response.dumps {
        print_dump(ctx, dump);
    }
    if follow {
        return dumps_follow(ctx, response.dumps.last().map_or(0, |d| d.id)).await;
    }
    if response.dumps.is_empty() && response.enabled {
        println!("No dumps yet. Call dump() in a site and it shows up here.");
    }
    Ok(())
}

/// Follow dumps by polling; simpler and more robust than re-subscribing.
async fn dumps_follow(ctx: &Ctx<'_>, mut last: u64) -> anyhow::Result<()> {
    loop {
        let response: DumpsResponse = ctx.client.get(endpoints::DUMPS).await?;
        let since = last;
        for dump in response.dumps.iter().filter(|d| d.id > since) {
            print_dump(ctx, dump);
            last = dump.id;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}

fn print_dump(ctx: &Ctx<'_>, dump: &DumpEntry) {
    if ctx.cli.json {
        println!("{}", serde_json::to_string(dump).unwrap_or_default());
        return;
    }
    let place = dump
        .source
        .as_ref()
        .map(|s| match s.line {
            Some(line) => format!("{}:{line}", s.name),
            None => s.name.clone(),
        })
        .unwrap_or_default();
    let origin = dump.origin.as_deref().unwrap_or_default();
    println!(
        "── {} {place} {origin}",
        red_core::time::datetime_from_unix(dump.received / 1000)
    );
    println!("{}\n", html_to_text(&dump.html));
}

pub(crate) async fn dumps_set(ctx: &Ctx<'_>, enabled: bool) -> anyhow::Result<()> {
    let response: DumpsResponse = ctx
        .send(
            Method::PUT,
            endpoints::DUMP_SETTINGS,
            Some(&DumpSettings { enabled }),
        )
        .await?;
    ctx.output(&response, |r| {
        if r.enabled {
            println!("dump() output from sites now goes to RED (`red dumps`, or the app).");
        } else {
            println!("dump() output is shown in the page again.");
        }
    })
}

pub(crate) async fn dumps_clear(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let _: serde_json::Value = ctx
        .send(Method::DELETE, endpoints::DUMPS, None::<&()>)
        .await?;
    ctx.output(&serde_json::json!({ "cleared": true }), |_| {
        println!("Cleared the captured dumps.");
    })
}

// ---- logs ------------------------------------------------------------------------

pub(crate) async fn logs(
    ctx: &Ctx<'_>,
    id: Option<&str>,
    lines: usize,
    follow: bool,
) -> anyhow::Result<()> {
    let Some(id) = id else {
        let files: Vec<LogFile> = ctx.client.get(endpoints::LOGS).await?;
        return ctx.output(&files, |files| {
            if files.is_empty() {
                println!("No logs yet.");
                return;
            }
            let rows: Vec<[String; 3]> = files
                .iter()
                .map(|f| [f.id.clone(), human_size(f.size), f.path.clone()])
                .collect();
            print_table(&["ID", "SIZE", "PATH"], &rows);
            println!("\nShow one with `red logs <ID>`.");
        });
    };
    let read = |n: usize| {
        let path = format!("{}?id={}&lines={n}", endpoints::LOG_READ, encode(id));
        async move { ctx.client.get::<LogResponse>(&path).await }
    };
    let response = read(lines).await?;
    if ctx.cli.json && !follow {
        println!("{}", serde_json::to_string_pretty(&response)?);
        return Ok(());
    }
    for entry in &response.entries {
        print_entry(entry);
    }
    if !follow {
        return Ok(());
    }
    let mut last = response.entries.last().cloned();
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let response = read(200).await?;
        let start = match &last {
            Some(last) => response
                .entries
                .iter()
                .rposition(|e| e == last)
                .map_or(0, |i| i + 1),
            None => 0,
        };
        for entry in &response.entries[start..] {
            print_entry(entry);
        }
        if let Some(entry) = response.entries.last() {
            last = Some(entry.clone());
        }
    }
}

fn print_entry(entry: &LogEntry) {
    match (&entry.time, &entry.level) {
        (Some(time), Some(level)) => {
            println!("{time} {:<7} {}", level.to_uppercase(), entry.message)
        }
        _ => println!("{}", entry.message),
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// Percent-encode a query value.
pub(crate) fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Rough plain-text version of HTML for the terminal: drops scripts,
/// styles and tags, and decodes the common entities.
pub(crate) fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        if lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
            let close = if lower[i..].starts_with("<script") {
                "</script>"
            } else {
                "</style>"
            };
            i = lower[i..]
                .find(close)
                .map_or(html.len(), |end| i + end + close.len());
            continue;
        }
        let c = html[i..].chars().next().expect("in bounds");
        if c == '<' {
            let tag_end = html[i..].find('>').map_or(html.len(), |end| i + end + 1);
            let tag = &lower[i..tag_end];
            if tag.starts_with("<br")
                || tag.starts_with("</p")
                || tag.starts_with("</div")
                || tag.starts_with("</tr")
                || tag.starts_with("</li")
                || tag.starts_with("</h")
            {
                text.push('\n');
            }
            i = tag_end;
            continue;
        }
        text.push(c);
        i += c.len_utf8();
    }
    let decoded = text
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#9660;", "▼")
        .replace("&#9654;", "▶")
        .replace("&amp;", "&");
    let lines: Vec<&str> = decoded.lines().map(str::trim_end).collect();
    lines
        .join("\n")
        .split("\n\n\n")
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_html_into_text() {
        let html = "<script>var x = 1;</script><style>p{}</style><pre class=sf-dump>\
                    <span class=sf-dump-note>array:1</span> [\n  \"<span>a</span>\" =&gt; 1\n]</pre>";
        assert_eq!(html_to_text(html), "array:1 [\n  \"a\" => 1\n]");
        assert_eq!(
            html_to_text("<p>Hi</p><p>there &amp; you</p>"),
            "Hi\nthere & you"
        );
        assert_eq!(
            html_to_text("<h1>Reset</h1><p>Click <a href=x>here</a></p>"),
            "Reset\nClick here"
        );
    }

    #[test]
    fn encodes_query_values() {
        assert_eq!(
            encode("site:blog:storage/logs/laravel.log"),
            "site:blog:storage/logs/laravel.log"
        );
        assert_eq!(encode("a b&c"), "a%20b%26c");
    }
}
