//! Splitting log text into entries for the log viewer.
//!
//! Three formats are recognised, line by line:
//!
//! * Laravel / Monolog: `[2026-10-18 15:35:43] local.ERROR: message`
//! * PHP-FPM: `[18-Oct-2026 15:35:43] WARNING: [pool red] message`
//! * redd (tracing): `2026-10-18T15:35:43.421Z  INFO red_daemon::web: message`
//!
//! A line that does not start an entry belongs to the previous one (stack
//! traces, multi-line messages). Logs in no known format get one entry per
//! line.

/// One parsed entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    pub time: Option<String>,
    pub level: Option<String>,
    pub channel: Option<String>,
    pub message: String,
}

/// Parse `text` into entries, oldest first.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    let mut structured = false;
    for raw in text.lines() {
        let line = strip_ansi(raw);
        if let Some(entry) = parse_header(&line) {
            structured = true;
            entries.push(entry);
        } else if structured && let Some(last) = entries.last_mut() {
            last.message.push('\n');
            last.message.push_str(&line);
        } else if !line.trim().is_empty() {
            entries.push(Entry {
                message: line,
                ..Entry::default()
            });
        }
    }
    entries
}

fn parse_header(line: &str) -> Option<Entry> {
    monolog(line)
        .or_else(|| fpm(line))
        .or_else(|| tracing(line))
}

/// `[2026-10-18 15:35:43] local.ERROR: message`, also with an ISO-8601
/// timestamp (`[2026-10-18T15:35:43.123456+00:00]`).
fn monolog(line: &str) -> Option<Entry> {
    let rest = line.strip_prefix('[')?;
    let (time, rest) = rest.split_once("] ")?;
    if !looks_like_iso_date(time) {
        return None;
    }
    let (head, message) = rest.split_once(": ").or_else(|| rest.split_once(':'))?;
    let (channel, level) = head.rsplit_once('.')?;
    if channel.contains(' ') || level.is_empty() || !level.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    Some(Entry {
        time: Some(time.to_owned()),
        level: Some(level.to_ascii_lowercase()),
        channel: Some(channel.to_owned()),
        message: message.to_owned(),
    })
}

/// `[18-Oct-2026 15:35:43] WARNING: [pool red] message`
fn fpm(line: &str) -> Option<Entry> {
    let rest = line.strip_prefix('[')?;
    let (time, rest) = rest.split_once("] ")?;
    let bytes = time.as_bytes();
    if bytes.len() < 20 || bytes[2] != b'-' || bytes[6] != b'-' {
        return None;
    }
    let (level, message) = rest.split_once(": ")?;
    if !level.chars().all(|c| c.is_ascii_uppercase()) || level.is_empty() {
        return None;
    }
    Some(Entry {
        time: Some(time.to_owned()),
        level: Some(normalise_level(level)),
        channel: None,
        message: message.to_owned(),
    })
}

/// `2026-10-18T15:35:43.421343Z  INFO red_daemon::web: message`
fn tracing(line: &str) -> Option<Entry> {
    let (time, rest) = line.split_once(' ')?;
    if !looks_like_iso_date(time) || !time.contains('T') {
        return None;
    }
    let rest = rest.trim_start();
    let (level, rest) = rest.split_once(' ')?;
    if !["TRACE", "DEBUG", "INFO", "WARN", "ERROR"].contains(&level) {
        return None;
    }
    let rest = rest.trim_start();
    let (channel, message) = match rest.split_once(": ") {
        Some((target, message)) if !target.contains(' ') => (Some(target.to_owned()), message),
        _ => (None, rest),
    };
    Some(Entry {
        time: Some(time.to_owned()),
        level: Some(normalise_level(level)),
        channel,
        message: message.to_owned(),
    })
}

fn normalise_level(level: &str) -> String {
    match level.to_ascii_lowercase().as_str() {
        "warn" => "warning".into(),
        other => other.to_owned(),
    }
}

fn looks_like_iso_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() >= 10
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[7] == b'-'
        && b[8..10].iter().all(u8::is_ascii_digit)
}

/// Remove ANSI colour codes (`ESC [ ... m`).
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_laravel_entries_with_traces() {
        let text = "[2026-10-18 10:00:00] local.ERROR: Division by zero {\"exception\":\"x\"}\n\
                    #0 /app/routes/web.php(5): closure()\n\
                    #1 {main}\n\
                    [2026-10-18 10:00:05] production.info: Done\n";
        let entries = parse(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level.as_deref(), Some("error"));
        assert_eq!(entries[0].channel.as_deref(), Some("local"));
        assert!(entries[0].message.contains("#1 {main}"));
        assert_eq!(entries[1].level.as_deref(), Some("info"));
        assert_eq!(entries[1].message, "Done");
    }

    #[test]
    fn parses_iso_monolog_timestamps() {
        let entries = parse("[2026-10-18T10:00:00.123456+00:00] app.WARNING: Careful\n");
        assert_eq!(
            entries[0].time.as_deref(),
            Some("2026-10-18T10:00:00.123456+00:00")
        );
        assert_eq!(entries[0].level.as_deref(), Some("warning"));
    }

    #[test]
    fn parses_fpm_and_tracing_lines() {
        let fpm = parse("[18-Oct-2026 15:35:43] WARNING: [pool red] child 12 said into stderr\n");
        assert_eq!(fpm[0].level.as_deref(), Some("warning"));
        assert!(fpm[0].message.starts_with("[pool red]"));

        let redd = parse(
            "\u{1b}[2m2026-10-18T15:35:43.421343Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m red_daemon::services: service running service=redis\n",
        );
        assert_eq!(redd[0].level.as_deref(), Some("info"));
        assert_eq!(redd[0].channel.as_deref(), Some("red_daemon::services"));
        assert_eq!(redd[0].message, "service running service=redis");
    }

    #[test]
    fn unknown_formats_get_one_entry_per_line() {
        let entries = parse("2026-10-18  1:00 [Note] InnoDB: ready\nsecond line\n\n");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].message, "second line");
        assert_eq!(entries[1].level, None);
    }
}
