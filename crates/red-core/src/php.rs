//! PHP versions: parsing and ordering.

use std::cmp::Ordering;

/// Parse the first line of `php-fpm -v` (or `php -v`), e.g.
/// `PHP 8.3.6 (fpm-fcgi) (built: ...)`, into `("8.3", "8.3.6")`.
pub fn parse_version_output(output: &str) -> Option<(String, String)> {
    let full = output
        .lines()
        .next()?
        .strip_prefix("PHP ")?
        .split_whitespace()
        .next()?;
    let minor = minor_version(full)?;
    Some((minor, full.to_owned()))
}

/// `"8.3.6"` or `"8.3"` → `"8.3"`. Returns `None` unless the input starts
/// with two numeric components.
pub fn minor_version(version: &str) -> Option<String> {
    let mut parts = version.split(['.', '-']);
    let major = parts.next()?;
    let minor = parts.next()?;
    let numeric = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
    (numeric(major) && numeric(minor)).then(|| format!("{major}.{minor}"))
}

/// Compare versions numerically, component by component (`8.10` > `8.9`).
pub fn compare(a: &str, b: &str) -> Ordering {
    let parse = |v: &str| -> Vec<u64> {
        v.split(['.', '-'])
            .map_while(|p| p.parse::<u64>().ok())
            .collect()
    };
    parse(a).cmp(&parse(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fpm_version_output() {
        let out =
            "PHP 8.3.6 (fpm-fcgi) (built: Sep  2 2026 12:56:02)\nCopyright (c) The PHP Group\n";
        assert_eq!(
            parse_version_output(out),
            Some(("8.3".to_owned(), "8.3.6".to_owned()))
        );
        assert_eq!(
            parse_version_output("PHP 8.5.0RC1 (fpm-fcgi)").map(|v| v.0),
            Some("8.5".to_owned())
        );
        assert_eq!(parse_version_output("garbage"), None);
    }

    #[test]
    fn minor_version_requires_two_numbers() {
        assert_eq!(minor_version("8.4.1").as_deref(), Some("8.4"));
        assert_eq!(minor_version("8.4").as_deref(), Some("8.4"));
        assert_eq!(minor_version("8"), None);
        assert_eq!(minor_version("latest"), None);
    }

    #[test]
    fn compares_numerically() {
        assert_eq!(compare("8.10", "8.9"), Ordering::Greater);
        assert_eq!(compare("8.3", "8.3"), Ordering::Equal);
        assert_eq!(compare("7.4", "8.0"), Ordering::Less);
    }
}
