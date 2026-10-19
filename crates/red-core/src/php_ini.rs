//! php.ini settings RED applies per PHP version, on top of the
//! installation's own php.ini: passed as `-d key=value` to PHP-FPM and to
//! `red php`, so they work the same for RED's builds, Homebrew and apt.

use std::collections::BTreeMap;

/// Settings the app offers as fields, with a short explanation.
pub const COMMON: [(&str, &str); 6] = [
    (
        "memory_limit",
        "Memory one request may use, e.g. 512M or -1",
    ),
    ("upload_max_filesize", "Largest uploaded file, e.g. 64M"),
    (
        "post_max_size",
        "Largest request body; at least the upload size",
    ),
    (
        "max_execution_time",
        "Seconds a request may run (0: no limit)",
    ),
    ("display_errors", "Show errors in the page: On or Off"),
    ("date.timezone", "Default time zone, e.g. Asia/Jakarta"),
];

/// Check one setting: ini-style key, single-line value.
pub fn validate(key: &str, value: &str) -> Result<(), String> {
    let key_ok = !key.is_empty()
        && key.len() <= 128
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        && key.starts_with(|c: char| c.is_ascii_alphabetic());
    if !key_ok {
        return Err(format!("{key:?} is not a php.ini setting name"));
    }
    if value.contains(['\n', '\r', '\0']) || value.len() > 1024 {
        return Err(format!("the value of {key} must be a single line"));
    }
    if key == "zend_extension" || key == "extension" {
        return Err("extensions are not set here; install them for the PHP version".into());
    }
    Ok(())
}

/// `-d` arguments (without the `-d`) for `settings`.
pub fn args(settings: &BTreeMap<String, String>) -> Vec<String> {
    settings.iter().map(|(k, v)| format!("{k}={v}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_settings() {
        assert!(validate("memory_limit", "512M").is_ok());
        assert!(validate("date.timezone", "Asia/Jakarta").is_ok());
        assert!(validate("opcache.enable", "0").is_ok());
        assert!(validate("", "1").is_err());
        assert!(validate("1abc", "1").is_err());
        assert!(validate("a b", "1").is_err());
        assert!(validate("memory_limit", "1\nauto_prepend_file=/x").is_err());
        assert!(validate("extension", "foo.so").is_err());
    }

    #[test]
    fn builds_arguments() {
        let settings = BTreeMap::from([
            ("memory_limit".to_owned(), "1G".to_owned()),
            ("display_errors".to_owned(), "On".to_owned()),
        ]);
        assert_eq!(args(&settings), ["display_errors=On", "memory_limit=1G"]);
    }
}
