//! Database and cache services RED can run next to the sites: MySQL or
//! MariaDB, PostgreSQL, and Redis or Valkey.
//!
//! RED does not ship these servers. It uses binaries already on the machine
//! (Homebrew, Postgres.app, distribution packages) and gives each service
//! instance its own data directory, port and log.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Which kind of server a service runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    /// MySQL or MariaDB (`mysqld` / `mariadbd`).
    Mysql,
    /// PostgreSQL (`postgres`).
    Postgres,
    /// Redis or Valkey (`redis-server` / `valkey-server`).
    Redis,
}

impl ServiceKind {
    pub const ALL: [Self; 3] = [Self::Mysql, Self::Postgres, Self::Redis];

    /// Name used in commands and as the default instance name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
            Self::Redis => "redis",
        }
    }

    /// Port the server conventionally listens on.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Mysql => 3306,
            Self::Postgres => 5432,
            Self::Redis => 6379,
        }
    }

    /// Parse a kind, accepting the names of the compatible servers too.
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "mysql" | "mariadb" => Some(Self::Mysql),
            "postgres" | "postgresql" | "pgsql" | "pg" => Some(Self::Postgres),
            "redis" | "valkey" => Some(Self::Redis),
            _ => None,
        }
    }

    /// Environment variables a Laravel app needs to talk to this service.
    pub fn laravel_env(self, port: u16) -> Vec<(&'static str, String)> {
        match self {
            Self::Mysql => vec![
                ("DB_CONNECTION", "mysql".into()),
                ("DB_HOST", "127.0.0.1".into()),
                ("DB_PORT", port.to_string()),
                ("DB_USERNAME", "root".into()),
                ("DB_PASSWORD", String::new()),
            ],
            Self::Postgres => vec![
                ("DB_CONNECTION", "pgsql".into()),
                ("DB_HOST", "127.0.0.1".into()),
                ("DB_PORT", port.to_string()),
                ("DB_USERNAME", "root".into()),
                ("DB_PASSWORD", String::new()),
            ],
            Self::Redis => vec![
                ("REDIS_HOST", "127.0.0.1".into()),
                ("REDIS_PORT", port.to_string()),
                ("REDIS_PASSWORD", "null".into()),
            ],
        }
    }
}

impl std::fmt::Display for ServiceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One configured service instance, stored in [`crate::State`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceConfig {
    pub kind: ServiceKind,
    /// The server binary (`mysqld`, `postgres`, `redis-server`, ...).
    pub binary: PathBuf,
    /// Version of `binary` when the service was added, e.g. `16.4`.
    #[serde(default)]
    pub version: Option<String>,
    /// TCP port on 127.0.0.1.
    pub port: u16,
    /// Start the service whenever redd starts.
    #[serde(default = "yes")]
    pub autostart: bool,
}

fn yes() -> bool {
    true
}

/// Which server a binary turned out to be, from its `--version` output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerVersion {
    pub kind: ServiceKind,
    /// `mysql`, `mariadb`, `postgresql`, `redis` or `valkey`.
    pub flavor: String,
    /// `major.minor`, e.g. `10.11`.
    pub version: String,
    /// Everything the binary reported, e.g. `10.11.14`.
    pub full_version: String,
}

/// Parse the output of `<server> --version`.
///
/// ```text
/// mysqld  Ver 8.4.0 for macos14.2 on arm64 (Homebrew)
/// mariadbd  Ver 10.11.14-MariaDB-0ubuntu0.24.04.1 for debian-linux-gnu on x86_64 (Ubuntu 24.04)
/// postgres (PostgreSQL) 16.10 (Ubuntu 16.10-0ubuntu0.24.04.1)
/// Redis server v=7.0.15 sha=00000000:0 malloc=jemalloc-5.3.0 bits=64 build=...
/// Valkey server v=8.0.1 sha=00000000:0 malloc=libc bits=64 build=...
/// ```
pub fn parse_server_version(kind: ServiceKind, output: &str) -> Option<ServerVersion> {
    let line = output.lines().find(|l| !l.trim().is_empty())?;
    let lower = line.to_ascii_lowercase();
    let flavor = match kind {
        ServiceKind::Mysql if lower.contains("mariadb") => "mariadb",
        ServiceKind::Mysql if lower.contains("mysqld") || lower.contains("mysql") => "mysql",
        ServiceKind::Postgres if lower.contains("postgresql") => "postgresql",
        ServiceKind::Redis if lower.starts_with("valkey") => "valkey",
        ServiceKind::Redis if lower.starts_with("redis") => "redis",
        _ => return None,
    };
    let full_version = line
        .split(|c: char| c.is_whitespace() || c == '=' || c == '-')
        .find(|word| {
            let mut parts = word.split('.');
            parts.next().is_some_and(is_number) && parts.next().is_some_and(is_number)
        })?
        .to_owned();
    let mut parts = full_version.split('.');
    let version = format!("{}.{}", parts.next()?, parts.next()?);
    Some(ServerVersion {
        kind,
        flavor: flavor.to_owned(),
        version,
        full_version,
    })
}

fn is_number(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kinds() {
        assert_eq!(ServiceKind::parse("MariaDB"), Some(ServiceKind::Mysql));
        assert_eq!(ServiceKind::parse("pgsql"), Some(ServiceKind::Postgres));
        assert_eq!(ServiceKind::parse("valkey"), Some(ServiceKind::Redis));
        assert_eq!(ServiceKind::parse("mongo"), None);
    }

    #[test]
    fn parses_versions() {
        let cases = [
            (
                ServiceKind::Mysql,
                "mysqld  Ver 8.4.0 for macos14.2 on arm64 (Homebrew)",
                "mysql",
                "8.4",
                "8.4.0",
            ),
            (
                ServiceKind::Mysql,
                "mariadbd  Ver 10.11.14-MariaDB-0ubuntu0.24.04.1 for debian-linux-gnu on x86_64",
                "mariadb",
                "10.11",
                "10.11.14",
            ),
            (
                ServiceKind::Postgres,
                "postgres (PostgreSQL) 16.10 (Ubuntu 16.10-0ubuntu0.24.04.1)",
                "postgresql",
                "16.10",
                "16.10",
            ),
            (
                ServiceKind::Redis,
                "Redis server v=7.0.15 sha=00000000:0 malloc=jemalloc-5.3.0 bits=64",
                "redis",
                "7.0",
                "7.0.15",
            ),
            (
                ServiceKind::Redis,
                "Valkey server v=8.0.1 sha=00000000:0 malloc=libc bits=64",
                "valkey",
                "8.0",
                "8.0.1",
            ),
        ];
        for (kind, output, flavor, version, full) in cases {
            let parsed = parse_server_version(kind, output).unwrap();
            assert_eq!(parsed.flavor, flavor, "{output}");
            assert_eq!(parsed.version, version, "{output}");
            assert_eq!(parsed.full_version, full, "{output}");
        }
    }

    #[test]
    fn rejects_the_wrong_server() {
        assert!(parse_server_version(ServiceKind::Redis, "postgres (PostgreSQL) 16.1").is_none());
        assert!(parse_server_version(ServiceKind::Postgres, "").is_none());
    }

    #[test]
    fn config_defaults_to_autostart() {
        let config: ServiceConfig = serde_json::from_str(
            r#"{"kind":"redis","binary":"/usr/bin/redis-server","port":6379}"#,
        )
        .unwrap();
        assert!(config.autostart);
        assert_eq!(config.version, None);
    }
}
