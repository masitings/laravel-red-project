//! Global, user-editable configuration (`config.toml`).
//!
//! The file is optional: a missing file means "all defaults". RED never
//! rewrites this file on its own; state that commands change (parked paths,
//! links, PHP choices) lives in [`crate::State`] instead.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Top-level domain used when none is configured. Reserved by RFC 6761.
pub const DEFAULT_TLD: &str = "test";

/// Port the web server listens on when none is configured.
pub const DEFAULT_HTTP_PORT: u16 = 80;

/// Port used instead of [`DEFAULT_HTTP_PORT`] when RED cannot bind a
/// privileged port (no helper installed yet).
pub const FALLBACK_HTTP_PORT: u16 = 8180;

/// Port the HTTPS server listens on when none is configured.
pub const DEFAULT_HTTPS_PORT: u16 = 443;

/// Port used instead of [`DEFAULT_HTTPS_PORT`] without the helper.
pub const FALLBACK_HTTPS_PORT: u16 = 8443;

/// Loopback port of the DNS responder. Unprivileged, so no root is needed;
/// the resolver file tells the OS to send `.<tld>` queries here.
pub const DEFAULT_DNS_PORT: u16 = 53530;

/// Contents of `config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Top-level domain sites are served under, without the leading dot.
    pub tld: String,
    /// Port for plain HTTP. `0` picks a free port (used by tests).
    pub http_port: u16,
    /// Port for HTTPS. `0` picks a free port.
    pub https_port: u16,
    /// Port of the DNS responder on 127.0.0.1. `0` picks a free port.
    pub dns_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tld: DEFAULT_TLD.to_owned(),
            http_port: DEFAULT_HTTP_PORT,
            https_port: DEFAULT_HTTPS_PORT,
            dns_port: DEFAULT_DNS_PORT,
        }
    }
}

/// Errors produced while loading or validating the configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
    #[error("invalid tld {0:?}: use lowercase letters, digits and '-' only")]
    InvalidTld(String),
}

impl Config {
    /// Load the configuration from `path`. A missing file yields defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).map_err(|err| match err {
                ConfigError::Parse { source, .. } => ConfigError::Parse {
                    path: path.to_owned(),
                    source,
                },
                other => other,
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Parse and validate configuration text.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: PathBuf::new(),
            source: Box::new(source),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Check values that serde cannot express.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let valid = !self.tld.is_empty()
            && !self.tld.starts_with('-')
            && !self.tld.ends_with('-')
            && self
                .tld
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if valid {
            Ok(())
        } else {
            Err(ConfigError::InvalidTld(self.tld.clone()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::load(&dir.path().join("config.toml")).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.tld, "test");
        assert_eq!(config.http_port, 80);
        assert_eq!(config.https_port, 443);
        assert_eq!(config.dns_port, 53530);
    }

    #[test]
    fn parses_values() {
        let config = Config::parse(
            r#"
            tld = "localhost"
            http_port = 8080
            https_port = 8443
            dns_port = 5300
            "#,
        )
        .unwrap();
        assert_eq!(config.tld, "localhost");
        assert_eq!(config.http_port, 8080);
        assert_eq!(config.https_port, 8443);
        assert_eq!(config.dns_port, 5300);
    }

    #[test]
    fn rejects_bad_tld() {
        for tld in ["", "Test", ".test", "-x", "a b"] {
            let text = format!("tld = {tld:?}");
            assert!(
                matches!(Config::parse(&text), Err(ConfigError::InvalidTld(_))),
                "{tld:?} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_unknown_keys() {
        assert!(matches!(
            Config::parse("tdl = \"test\""),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn load_reports_path_on_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "tld = [").unwrap();
        match Config::load(&path) {
            Err(ConfigError::Parse { path: reported, .. }) => assert_eq!(reported, path),
            other => panic!("unexpected: {other:?}"),
        }
    }
}
