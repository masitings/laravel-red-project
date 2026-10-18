//! Runtimes RED can download and manage itself: PHP (static builds from
//! static-php-cli), PostgreSQL (relocatable builds published by the zonky
//! project on Maven Central) and MySQL (Oracle's official tarballs).
//!
//! This module only decides *what* to download: catalogs, versions and
//! archive layouts. The daemon fetches, verifies and unpacks.
//!
//! Each runtime is installed under `<data>/runtimes/<kind>/<version>/`
//! where `version` is the line users pick (`8.4` for PHP and MySQL, `18`
//! for PostgreSQL), so updating a line replaces it in place.

use serde::{Deserialize, Serialize};

/// Something RED can install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Php,
    Mysql,
    Postgres,
}

impl RuntimeKind {
    pub const ALL: [Self; 3] = [Self::Php, Self::Mysql, Self::Postgres];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "php" => Some(Self::Php),
            "mysql" => Some(Self::Mysql),
            "postgres" | "postgresql" | "pgsql" | "pg" => Some(Self::Postgres),
            _ => None,
        }
    }
}

impl std::fmt::Display for RuntimeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Macos,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    Aarch64,
    X86_64,
}

/// Operating system and CPU a runtime is built for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Platform {
    pub os: Os,
    pub arch: Arch,
}

impl Platform {
    /// The platform this binary was built for, if RED supports it.
    pub fn current() -> Option<Self> {
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            return None;
        };
        let arch = if cfg!(target_arch = "aarch64") {
            Arch::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            Arch::X86_64
        } else {
            return None;
        };
        Some(Self { os, arch })
    }

    /// `macos-aarch64`, `linux-x86_64`, ...
    pub fn name(self) -> String {
        format!(
            "{}-{}",
            match self.os {
                Os::Macos => "macos",
                Os::Linux => "linux",
            },
            match self.arch {
                Arch::Aarch64 => "aarch64",
                Arch::X86_64 => "x86_64",
            }
        )
    }
}

/// How a downloaded file is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    TarGz,
    TarXz,
    /// A Java archive (zip) holding one `.txz` (tar + xz).
    JarTxz,
}

/// Where an archive's contents go inside the runtime directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Layout {
    /// The archive has one top-level directory whose contents become the
    /// runtime directory.
    StripTopDir,
    /// The archive's entries go into the runtime directory as they are.
    Flat,
    /// The archive holds a single file, placed at this relative path.
    Single { to: String },
}

/// One file to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub url: String,
    /// Expected SHA-256, when known up front.
    pub sha256: Option<String>,
    /// Where to fetch the expected SHA-256 from, when published.
    pub sha256_url: Option<String>,
    pub format: Format,
    pub layout: Layout,
}

/// A concrete installable version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub kind: RuntimeKind,
    /// The line users pick and the directory name: `8.4`, `18`.
    pub line: String,
    /// The exact version: `8.4.23`, `18.6.0`.
    pub version: String,
    pub artifacts: Vec<Artifact>,
}

/// `true` when `version` (e.g. `8.4.23`) is on `wanted` (`8`, `8.4` or
/// `8.4.23`).
pub fn version_matches(version: &str, wanted: &str) -> bool {
    let wanted = wanted.trim().trim_start_matches('v');
    if wanted.is_empty() {
        return true;
    }
    let mut have = version.split('.');
    wanted.split('.').all(|part| have.next() == Some(part))
}

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(a).cmp(&parts(b))
}

/// The newest of `versions` on line `wanted`.
fn newest<'a>(versions: impl Iterator<Item = &'a str>, wanted: &str) -> Option<&'a str> {
    versions
        .filter(|v| version_matches(v, wanted))
        .max_by(|a, b| compare(a, b))
}

/// Newest version per line, newest line first.
fn lines(versions: &[String], line_of: impl Fn(&str) -> String) -> Vec<(String, String)> {
    let mut best: Vec<(String, String)> = Vec::new();
    for version in versions {
        let line = line_of(version);
        match best.iter_mut().find(|(l, _)| *l == line) {
            Some((_, current)) if compare(version, current).is_gt() => *current = version.clone(),
            Some(_) => {}
            None => best.push((line, version.clone())),
        }
    }
    best.sort_by(|(a, _), (b, _)| compare(b, a));
    best
}

fn minor_line(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

fn major_line(version: &str) -> String {
    version.split('.').next().unwrap_or_default().to_owned()
}

// ---- PHP: static-php-cli ---------------------------------------------------------

/// Directory listing of static-php-cli's prebuilt binaries, as JSON.
pub const PHP_LISTING_URL: &str = "https://dl.static-php.dev/static-php-cli/bulk/?format=json";
const PHP_BASE_URL: &str = "https://dl.static-php.dev/static-php-cli/bulk";
/// Oldest PHP line RED offers (older lines are end of life).
const PHP_OLDEST: &str = "8.1";

/// One entry of the static-php-cli listing.
#[derive(Debug, Clone, Deserialize)]
pub struct ListingEntry {
    pub name: String,
}

/// Exact PHP versions that have both a CLI and an FPM build for `platform`.
pub fn php_versions(listing: &[ListingEntry], platform: Platform) -> Vec<String> {
    let suffix = format!("-{}.tar.gz", platform.name());
    let mut cli = Vec::new();
    let mut fpm = Vec::new();
    for entry in listing {
        let Some(rest) = entry.name.strip_prefix("php-") else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(&suffix) else {
            continue;
        };
        if let Some(version) = rest.strip_suffix("-cli") {
            cli.push(version.to_owned());
        } else if let Some(version) = rest.strip_suffix("-fpm") {
            fpm.push(version.to_owned());
        }
    }
    cli.retain(|v| fpm.contains(v) && compare(v, PHP_OLDEST).is_ge());
    cli
}

/// Newest PHP per minor line, e.g. `[("8.5", "8.5.8"), ("8.4", "8.4.23")]`.
pub fn php_lines(listing: &[ListingEntry], platform: Platform) -> Vec<(String, String)> {
    lines(&php_versions(listing, platform), minor_line)
}

/// The PHP release for `wanted` (`8.4`, `8.4.23`; empty for the newest).
pub fn php_release(listing: &[ListingEntry], wanted: &str, platform: Platform) -> Option<Release> {
    let versions = php_versions(listing, platform);
    let version = newest(versions.iter().map(String::as_str), wanted)?.to_owned();
    let file = |sapi: &str| format!("php-{version}-{sapi}-{}.tar.gz", platform.name());
    Some(Release {
        kind: RuntimeKind::Php,
        line: minor_line(&version),
        artifacts: vec![
            Artifact {
                url: format!("{PHP_BASE_URL}/{}", file("cli")),
                sha256: None,
                sha256_url: None,
                format: Format::TarGz,
                layout: Layout::Single {
                    to: "bin/php".into(),
                },
            },
            Artifact {
                url: format!("{PHP_BASE_URL}/{}", file("fpm")),
                sha256: None,
                sha256_url: None,
                format: Format::TarGz,
                layout: Layout::Single {
                    to: "sbin/php-fpm".into(),
                },
            },
        ],
        version,
    })
}

/// `php.ini` written next to RED's own PHP builds, which come without one.
pub const PHP_INI: &str = "; php.ini for a PHP build installed by RED.
; RED writes this file once; your changes are kept.

memory_limit = 512M
upload_max_filesize = 100M
post_max_size = 100M
max_execution_time = 60
date.timezone = UTC
display_errors = On
error_reporting = E_ALL
opcache.enable = 1
opcache.enable_cli = 0
opcache.validate_timestamps = 1
opcache.revalidate_freq = 0
";

// ---- PostgreSQL: zonky embedded-postgres-binaries --------------------------------

const PG_BASE_URL: &str = "https://repo1.maven.org/maven2/io/zonky/test/postgres";
/// Oldest PostgreSQL major RED offers.
const PG_OLDEST: u64 = 14;

fn pg_artifact(platform: Platform) -> &'static str {
    match (platform.os, platform.arch) {
        (Os::Macos, Arch::Aarch64) => "embedded-postgres-binaries-darwin-arm64v8",
        (Os::Macos, Arch::X86_64) => "embedded-postgres-binaries-darwin-amd64",
        (Os::Linux, Arch::Aarch64) => "embedded-postgres-binaries-linux-arm64v8",
        (Os::Linux, Arch::X86_64) => "embedded-postgres-binaries-linux-amd64",
    }
}

/// Maven metadata listing the published PostgreSQL versions.
pub fn pg_metadata_url(platform: Platform) -> String {
    format!("{PG_BASE_URL}/{}/maven-metadata.xml", pg_artifact(platform))
}

/// Versions in a `maven-metadata.xml`.
pub fn parse_maven_versions(xml: &str) -> Vec<String> {
    xml.split("<version>")
        .skip(1)
        .filter_map(|rest| rest.split("</version>").next())
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
        .filter(|v| major_line(v).parse::<u64>().is_ok_and(|m| m >= PG_OLDEST))
        .collect()
}

/// Newest PostgreSQL per major, e.g. `[("18", "18.6.0"), ("17", "17.11.0")]`.
pub fn pg_lines(versions: &[String]) -> Vec<(String, String)> {
    lines(versions, major_line)
}

/// The PostgreSQL release for `wanted` (`18`, `18.6.0`; empty for the newest).
pub fn pg_release(versions: &[String], wanted: &str, platform: Platform) -> Option<Release> {
    let version = newest(versions.iter().map(String::as_str), wanted)?.to_owned();
    let artifact = pg_artifact(platform);
    let url = format!("{PG_BASE_URL}/{artifact}/{version}/{artifact}-{version}.jar");
    Some(Release {
        kind: RuntimeKind::Postgres,
        line: major_line(&version),
        artifacts: vec![Artifact {
            sha256_url: Some(format!("{url}.sha256")),
            url,
            sha256: None,
            format: Format::JarTxz,
            layout: Layout::Flat,
        }],
        version,
    })
}

// ---- MySQL: Oracle's tarballs ----------------------------------------------------

/// MySQL builds RED knows, with checksums recorded when they were added.
/// Oracle keeps these files at the archive URL permanently.
const MYSQL_BUILDS: &[(&str, Os, Arch, &str, &str)] = &[
    (
        "8.4.10",
        Os::Macos,
        Arch::Aarch64,
        "mysql-8.4.10-macos15-arm64.tar.gz",
        "282618afd5cb662b94ac837f210b0ccb87ef156dd4c03eb88e094702a5c9ea1f",
    ),
    (
        "8.4.10",
        Os::Macos,
        Arch::X86_64,
        "mysql-8.4.10-macos15-x86_64.tar.gz",
        "2e726a1d5b4313b2eadc93c2b97ec26111f310340e3faee371e1d92120e87171",
    ),
    (
        "8.4.10",
        Os::Linux,
        Arch::X86_64,
        "mysql-8.4.10-linux-glibc2.28-x86_64-minimal.tar.xz",
        "88ac9cc85bed076c4c8c6e816c3369a6febfbb5db13e6e174f1d8971161e0b77",
    ),
];
/// Oracle's permanent archive; files live under `mysql-<line>/`.
const MYSQL_ARCHIVE_URL: &str = "https://cdn.mysql.com/archives";

/// MySQL releases for `platform`, newest first.
pub fn mysql_releases(platform: Platform) -> Vec<Release> {
    let mut releases: Vec<Release> = MYSQL_BUILDS
        .iter()
        .filter(|(_, os, arch, _, _)| *os == platform.os && *arch == platform.arch)
        .map(|(version, _, _, file, sha256)| Release {
            kind: RuntimeKind::Mysql,
            line: minor_line(version),
            version: (*version).to_owned(),
            artifacts: vec![Artifact {
                url: format!("{MYSQL_ARCHIVE_URL}/mysql-{}/{file}", minor_line(version)),
                sha256: Some((*sha256).to_owned()),
                sha256_url: None,
                format: if file.ends_with(".tar.xz") {
                    Format::TarXz
                } else {
                    Format::TarGz
                },
                layout: Layout::StripTopDir,
            }],
        })
        .collect();
    releases.sort_by(|a, b| compare(&b.version, &a.version));
    releases
}

/// The MySQL release for `wanted` (`8.4`; empty for the newest).
pub fn mysql_release(wanted: &str, platform: Platform) -> Option<Release> {
    mysql_releases(platform)
        .into_iter()
        .find(|r| version_matches(&r.version, wanted))
}

/// The marker written into every installed runtime directory.
pub const MARKER_FILE: &str = ".red-runtime.json";

/// Contents of [`MARKER_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    pub kind: RuntimeKind,
    pub line: String,
    pub version: String,
    /// Unix time of the installation.
    pub installed: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC_ARM: Platform = Platform {
        os: Os::Macos,
        arch: Arch::Aarch64,
    };

    fn listing(names: &[&str]) -> Vec<ListingEntry> {
        names
            .iter()
            .map(|n| ListingEntry {
                name: (*n).to_owned(),
            })
            .collect()
    }

    #[test]
    fn picks_php_builds_with_cli_and_fpm() {
        let listing = listing(&[
            "README.txt",
            "php-8.4.9-cli-macos-aarch64.tar.gz",
            "php-8.4.9-fpm-macos-aarch64.tar.gz",
            "php-8.4.23-cli-macos-aarch64.tar.gz",
            "php-8.4.23-fpm-macos-aarch64.tar.gz",
            "php-8.4.24-cli-macos-aarch64.tar.gz", // no FPM build yet
            "php-8.5.8-cli-macos-aarch64.tar.gz",
            "php-8.5.8-fpm-macos-aarch64.tar.gz",
            "php-8.5.8-fpm-linux-x86_64.tar.gz",
            "php-8.0.30-cli-macos-aarch64.tar.gz",
            "php-8.0.30-fpm-macos-aarch64.tar.gz",
            "php-8.5.8-micro-macos-aarch64.tar.gz",
        ]);
        assert_eq!(
            php_lines(&listing, MAC_ARM),
            vec![
                ("8.5".to_owned(), "8.5.8".to_owned()),
                ("8.4".to_owned(), "8.4.23".to_owned())
            ]
        );
        let release = php_release(&listing, "8.4", MAC_ARM).unwrap();
        assert_eq!(release.version, "8.4.23");
        assert_eq!(release.line, "8.4");
        assert!(
            release.artifacts[1]
                .url
                .ends_with("/php-8.4.23-fpm-macos-aarch64.tar.gz")
        );
        assert_eq!(
            release.artifacts[1].layout,
            Layout::Single {
                to: "sbin/php-fpm".into()
            }
        );
        assert_eq!(php_release(&listing, "", MAC_ARM).unwrap().version, "8.5.8");
        assert!(php_release(&listing, "7.4", MAC_ARM).is_none());
    }

    #[test]
    fn picks_postgres_versions() {
        let xml = "<metadata><versioning><versions><version>13.9.0</version>\
                   <version>17.11.0</version><version>18.2.0</version>\
                   <version>18.6.0</version><version>18.10.0-beta</version>\
                   </versions></versioning></metadata>";
        let versions = parse_maven_versions(xml);
        assert_eq!(versions, ["17.11.0", "18.2.0", "18.6.0"]);
        assert_eq!(
            pg_lines(&versions)[0],
            ("18".to_owned(), "18.6.0".to_owned())
        );
        let release = pg_release(&versions, "17", MAC_ARM).unwrap();
        assert_eq!(release.line, "17");
        assert!(release.artifacts[0].url.ends_with(
            "embedded-postgres-binaries-darwin-arm64v8/17.11.0/embedded-postgres-binaries-darwin-arm64v8-17.11.0.jar"
        ));
        assert!(
            release.artifacts[0]
                .sha256_url
                .as_deref()
                .unwrap()
                .ends_with(".jar.sha256")
        );
    }

    #[test]
    fn knows_mysql_builds() {
        let release = mysql_release("8.4", MAC_ARM).unwrap();
        assert_eq!(release.version, "8.4.10");
        assert_eq!(
            release.artifacts[0].sha256.as_deref().map(str::len),
            Some(64)
        );
        assert_eq!(release.artifacts[0].layout, Layout::StripTopDir);
        assert_eq!(
            release.artifacts[0].url,
            "https://cdn.mysql.com/archives/mysql-8.4/mysql-8.4.10-macos15-arm64.tar.gz"
        );
        let linux_arm = Platform {
            os: Os::Linux,
            arch: Arch::Aarch64,
        };
        assert!(mysql_releases(linux_arm).is_empty());
    }

    #[test]
    fn matches_versions() {
        assert!(version_matches("8.4.23", "8.4"));
        assert!(version_matches("8.4.23", ""));
        assert!(!version_matches("8.4.23", "8.40"));
        assert!(version_matches("18.6.0", "18"));
    }
}
