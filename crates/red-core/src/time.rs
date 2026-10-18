//! Tiny date helpers, enough to show certificate expiry dates without
//! pulling in a date library.

/// Format a Unix timestamp as a UTC calendar date, `YYYY-MM-DD`.
pub fn date_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Format a Unix timestamp as UTC date and time, `YYYY-MM-DD HH:MM:SSZ`.
pub fn datetime_from_unix(secs: u64) -> String {
    let rem = secs % 86_400;
    format!(
        "{} {:02}:{:02}:{:02}Z",
        date_from_unix(secs),
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_date_and_time() {
        assert_eq!(datetime_from_unix(1_760_000_000), "2025-10-09 08:53:20Z");
    }

    #[test]
    fn formats_dates() {
        assert_eq!(date_from_unix(0), "1970-01-01");
        assert_eq!(date_from_unix(951_782_400), "2000-02-29");
        assert_eq!(date_from_unix(1_792_281_600), "2026-10-18");
    }
}
