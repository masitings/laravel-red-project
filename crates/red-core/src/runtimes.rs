//! Runtimes RED can download and manage itself:
//!
//! * PHP and Valkey built by RED Project's own CI and listed in its runtime
//!   manifest (a JSON file published with GitHub releases); PHP falls back
//!   to static-php-cli's prebuilt binaries for versions the manifest lacks.
//! * PostgreSQL: relocatable builds published by the zonky project on Maven
//!   Central.
//! * MySQL: Oracle's official tarballs.
//! * Node.js: the official builds from nodejs.org.
//! * Composer: the official `composer.phar` from getcomposer.org.
//! * MariaDB: RED's own builds from the manifest, else the official Linux
//!   x86-64 tarballs from mariadb.org.
//! * Bun and cloudflared: the latest GitHub release of each project, checked
//!   against the digest GitHub publishes for every file.
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
    Valkey,
    Node,
    Composer,
    Mariadb,
    Bun,
    Cloudflared,
}

impl RuntimeKind {
    pub const ALL: [Self; 9] = [
        Self::Php,
        Self::Mysql,
        Self::Postgres,
        Self::Valkey,
        Self::Node,
        Self::Composer,
        Self::Mariadb,
        Self::Bun,
        Self::Cloudflared,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Mysql => "mysql",
            Self::Postgres => "postgres",
            Self::Valkey => "valkey",
            Self::Node => "node",
            Self::Composer => "composer",
            Self::Mariadb => "mariadb",
            Self::Bun => "bun",
            Self::Cloudflared => "cloudflared",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "php" => Some(Self::Php),
            "mysql" => Some(Self::Mysql),
            "postgres" | "postgresql" | "pgsql" | "pg" => Some(Self::Postgres),
            "valkey" | "redis" => Some(Self::Valkey),
            "node" | "nodejs" | "node.js" => Some(Self::Node),
            "composer" => Some(Self::Composer),
            "mariadb" => Some(Self::Mariadb),
            "bun" => Some(Self::Bun),
            "cloudflared" | "tunnel" => Some(Self::Cloudflared),
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
    Zip,
    /// A Java archive (zip) holding one `.txz` (tar + xz).
    JarTxz,
    /// Not an archive: the file itself (used with [`Layout::Single`]).
    Plain,
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
    /// For a sums file listing several files (`<hash>  <name>` lines), the
    /// name to look up.
    pub sha256_name: Option<String>,
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
                sha256_name: None,
                format: Format::TarGz,
                layout: Layout::Single {
                    to: "bin/php".into(),
                },
            },
            Artifact {
                url: format!("{PHP_BASE_URL}/{}", file("fpm")),
                sha256: None,
                sha256_url: None,
                sha256_name: None,
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
            sha256_name: None,
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
                sha256_name: None,
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

// ---- RED Project's own builds ----------------------------------------------------

/// Where RED Project's CI publishes the list of runtimes it built (PHP,
/// Valkey). Overridable with `runtimes_manifest` in `config.toml`.
pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/masitings/laravel-red-project/releases/download/runtimes/manifest.json";

/// The runtime manifest published by RED Project's CI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    #[serde(default)]
    pub runtimes: Vec<ManifestEntry>,
}

/// One build in the manifest: a `.tar.gz` whose entries form the runtime
/// directory (`bin/php`, `sbin/php-fpm`, `bin/valkey-server`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub kind: RuntimeKind,
    pub line: String,
    pub version: String,
    /// `macos-aarch64`, `linux-x86_64`, ...
    pub platform: String,
    pub url: String,
    pub sha256: String,
}

impl Manifest {
    fn entries(
        &self,
        kind: RuntimeKind,
        platform: Platform,
    ) -> impl Iterator<Item = &ManifestEntry> {
        let name = platform.name();
        self.runtimes
            .iter()
            .filter(move |e| e.kind == kind && e.platform == name && e.sha256.len() == 64)
    }

    /// Newest build per line for `kind`, newest line first.
    pub fn lines(&self, kind: RuntimeKind, platform: Platform) -> Vec<(String, String)> {
        let versions: Vec<String> = self
            .entries(kind, platform)
            .map(|e| e.version.clone())
            .collect();
        let mut result = lines(&versions, |v| {
            self.runtimes
                .iter()
                .find(|e| e.kind == kind && e.version == v)
                .map(|e| e.line.clone())
                .unwrap_or_else(|| minor_line(v))
        });
        result.sort_by(|(a, _), (b, _)| compare(b, a));
        result
    }

    /// The newest build of `kind` matching `wanted`.
    pub fn release(&self, kind: RuntimeKind, wanted: &str, platform: Platform) -> Option<Release> {
        let entry = self
            .entries(kind, platform)
            .filter(|e| version_matches(&e.version, wanted) || e.line == wanted.trim())
            .max_by(|a, b| compare(&a.version, &b.version))?;
        Some(Release {
            kind,
            line: entry.line.clone(),
            version: entry.version.clone(),
            artifacts: vec![Artifact {
                url: entry.url.clone(),
                sha256: Some(entry.sha256.to_ascii_lowercase()),
                sha256_url: None,
                sha256_name: None,
                format: Format::TarGz,
                layout: Layout::Flat,
            }],
        })
    }
}

/// PHP lines from RED's own builds, completed with static-php-cli's for
/// lines the manifest does not have.
pub fn php_lines_from(
    manifest: &Manifest,
    listing: &[ListingEntry],
    platform: Platform,
) -> Vec<(String, String)> {
    let mut all = manifest.lines(RuntimeKind::Php, platform);
    for (line, version) in php_lines(listing, platform) {
        if !all.iter().any(|(l, _)| *l == line) {
            all.push((line, version));
        }
    }
    all.sort_by(|(a, _), (b, _)| compare(b, a));
    all
}

/// The PHP release for `wanted`: RED's own build when the manifest has one,
/// static-php-cli's otherwise.
pub fn php_release_from(
    manifest: &Manifest,
    listing: &[ListingEntry],
    wanted: &str,
    platform: Platform,
) -> Option<Release> {
    manifest
        .release(RuntimeKind::Php, wanted, platform)
        .or_else(|| php_release(listing, wanted, platform))
}

// ---- Node.js: nodejs.org ----------------------------------------------------------

/// Every Node.js release, newest first.
pub const NODE_INDEX_URL: &str = "https://nodejs.org/dist/index.json";
/// Oldest Node.js major RED offers.
const NODE_OLDEST: u64 = 20;

/// One entry of `index.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeEntry {
    /// `v22.11.0`
    pub version: String,
    /// `false`, or the LTS code name.
    #[serde(default)]
    pub lts: serde_json::Value,
    #[serde(default)]
    pub files: Vec<String>,
}

fn node_platform(platform: Platform) -> (&'static str, &'static str) {
    match (platform.os, platform.arch) {
        (Os::Macos, Arch::Aarch64) => ("darwin-arm64", "osx-arm64-tar"),
        (Os::Macos, Arch::X86_64) => ("darwin-x64", "osx-x64-tar"),
        (Os::Linux, Arch::Aarch64) => ("linux-arm64", "linux-arm64"),
        (Os::Linux, Arch::X86_64) => ("linux-x64", "linux-x64"),
    }
}

/// Versions worth offering: LTS lines and the newest major, from 20 on.
pub fn node_versions(index: &[NodeEntry], platform: Platform) -> Vec<String> {
    let (_, file) = node_platform(platform);
    let newest_major = index
        .iter()
        .filter_map(|e| {
            major_line(e.version.trim_start_matches('v'))
                .parse::<u64>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    index
        .iter()
        .filter(|e| e.files.iter().any(|f| f == file))
        .filter_map(|e| {
            let version = e.version.trim_start_matches('v').to_owned();
            let major: u64 = major_line(&version).parse().ok()?;
            let lts = !matches!(
                e.lts,
                serde_json::Value::Bool(false) | serde_json::Value::Null
            );
            (major >= NODE_OLDEST && (lts || major == newest_major)).then_some(version)
        })
        .collect()
}

/// Newest Node.js per major, newest first.
pub fn node_lines(index: &[NodeEntry], platform: Platform) -> Vec<(String, String)> {
    lines(&node_versions(index, platform), major_line)
}

/// The Node.js release for `wanted` (`22`, `22.11.0`; empty for the newest LTS).
pub fn node_release(index: &[NodeEntry], wanted: &str, platform: Platform) -> Option<Release> {
    let versions = node_versions(index, platform);
    let wanted = if wanted.trim().is_empty() {
        // Default to the newest LTS rather than the newest "current" release.
        let lts: Vec<String> = index
            .iter()
            .filter(|e| {
                !matches!(
                    e.lts,
                    serde_json::Value::Bool(false) | serde_json::Value::Null
                )
            })
            .map(|e| e.version.trim_start_matches('v').to_owned())
            .filter(|v| versions.contains(v))
            .collect();
        newest(lts.iter().map(String::as_str), "")
            .map(major_line)
            .unwrap_or_default()
    } else {
        wanted.trim().trim_start_matches('v').to_owned()
    };
    let version = newest(versions.iter().map(String::as_str), &wanted)?.to_owned();
    let (name, _) = node_platform(platform);
    let file = format!("node-v{version}-{name}.tar.gz");
    Some(Release {
        kind: RuntimeKind::Node,
        line: major_line(&version),
        artifacts: vec![Artifact {
            url: format!("https://nodejs.org/dist/v{version}/{file}"),
            sha256: None,
            sha256_url: Some(format!("https://nodejs.org/dist/v{version}/SHASUMS256.txt")),
            sha256_name: Some(file),
            format: Format::TarGz,
            layout: Layout::StripTopDir,
        }],
        version,
    })
}

// ---- Composer: getcomposer.org ----------------------------------------------------

/// Composer's release channels as JSON.
pub const COMPOSER_VERSIONS_URL: &str = "https://getcomposer.org/versions";

/// `stable` entries of the versions document.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ComposerVersions {
    #[serde(default)]
    pub stable: Vec<ComposerVersion>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ComposerVersion {
    pub version: String,
}

/// The newest stable Composer 2.
pub fn composer_release(versions: &ComposerVersions, wanted: &str) -> Option<Release> {
    let version = newest(
        versions
            .stable
            .iter()
            .map(|v| v.version.as_str())
            .filter(|v| v.starts_with("2.")),
        wanted,
    )?
    .to_owned();
    let url = format!("https://getcomposer.org/download/{version}/composer.phar");
    Some(Release {
        kind: RuntimeKind::Composer,
        line: major_line(&version),
        artifacts: vec![Artifact {
            sha256_url: Some(format!("{url}.sha256sum")),
            url,
            sha256: None,
            sha256_name: None,
            format: Format::Plain,
            layout: Layout::Single {
                to: "bin/composer.phar".into(),
            },
        }],
        version,
    })
}

// ---- GitHub releases: Bun, cloudflared ----------------------------------------------

/// Latest release of Bun, from GitHub's API.
pub const BUN_RELEASE_URL: &str = "https://api.github.com/repos/oven-sh/bun/releases/latest";
/// Latest release of cloudflared, from GitHub's API.
pub const CLOUDFLARED_RELEASE_URL: &str =
    "https://api.github.com/repos/cloudflare/cloudflared/releases/latest";

/// The parts of a GitHub release RED reads.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    /// `sha256:<hex>`, published by GitHub for each file.
    #[serde(default)]
    pub digest: Option<String>,
}

impl GithubRelease {
    fn asset(&self, name: &str) -> Option<&GithubAsset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

fn digest_sha256(asset: &GithubAsset) -> Option<String> {
    let hex = asset.digest.as_deref()?.strip_prefix("sha256:")?;
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| hex.to_ascii_lowercase())
}

/// The latest Bun for `platform` (Bun publishes only its newest version).
pub fn bun_release(release: &GithubRelease, platform: Platform) -> Option<Release> {
    let version = release.tag_name.strip_prefix("bun-v")?.to_owned();
    let name = match (platform.os, platform.arch) {
        (Os::Macos, Arch::Aarch64) => "bun-darwin-aarch64.zip",
        (Os::Macos, Arch::X86_64) => "bun-darwin-x64.zip",
        (Os::Linux, Arch::Aarch64) => "bun-linux-aarch64.zip",
        (Os::Linux, Arch::X86_64) => "bun-linux-x64.zip",
    };
    let asset = release.asset(name)?;
    Some(Release {
        kind: RuntimeKind::Bun,
        line: major_line(&version),
        artifacts: vec![Artifact {
            url: asset.browser_download_url.clone(),
            sha256: digest_sha256(asset),
            sha256_url: None,
            sha256_name: None,
            format: Format::Zip,
            layout: Layout::Single {
                to: "bin/bun".into(),
            },
        }],
        version,
    })
}

/// The latest cloudflared for `platform`.
pub fn cloudflared_release(release: &GithubRelease, platform: Platform) -> Option<Release> {
    let version = release.tag_name.trim_start_matches('v').to_owned();
    let (name, format) = match (platform.os, platform.arch) {
        (Os::Macos, Arch::Aarch64) => ("cloudflared-darwin-arm64.tgz", Format::TarGz),
        (Os::Macos, Arch::X86_64) => ("cloudflared-darwin-amd64.tgz", Format::TarGz),
        (Os::Linux, Arch::Aarch64) => ("cloudflared-linux-arm64", Format::Plain),
        (Os::Linux, Arch::X86_64) => ("cloudflared-linux-amd64", Format::Plain),
    };
    let asset = release.asset(name)?;
    Some(Release {
        kind: RuntimeKind::Cloudflared,
        line: major_line(&version),
        artifacts: vec![Artifact {
            url: asset.browser_download_url.clone(),
            sha256: digest_sha256(asset),
            sha256_url: None,
            sha256_name: None,
            format,
            layout: Layout::Single {
                to: "bin/cloudflared".into(),
            },
        }],
        version,
    })
}

// ---- MariaDB: downloads.mariadb.org -------------------------------------------------

/// MariaDB's release series.
pub const MARIADB_MAJORS_URL: &str = "https://downloads.mariadb.org/rest-api/mariadb/";

/// The latest release of series `major`, with its files.
pub fn mariadb_latest_url(major: &str) -> String {
    format!("https://downloads.mariadb.org/rest-api/mariadb/{major}/latest/")
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MariadbMajors {
    #[serde(default)]
    pub major_releases: Vec<MariadbMajor>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MariadbMajor {
    pub release_id: String,
    #[serde(default)]
    pub release_status: Option<String>,
    #[serde(default)]
    pub release_support_type: Option<String>,
}

/// Stable long-term-support series, newest first (at most four).
pub fn mariadb_lts_majors(majors: &MariadbMajors) -> Vec<String> {
    let mut lts: Vec<String> = majors
        .major_releases
        .iter()
        .filter(|m| m.release_status.as_deref() == Some("Stable"))
        .filter(|m| m.release_support_type.as_deref() == Some("Long Term Support"))
        .map(|m| m.release_id.clone())
        .collect();
    lts.sort_by(|a, b| compare(b, a));
    lts.truncate(4);
    lts
}

/// `/rest-api/mariadb/<major>/latest/`: one release and its files.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MariadbLatest {
    #[serde(default)]
    pub releases: std::collections::BTreeMap<String, MariadbRelease>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MariadbRelease {
    pub release_id: String,
    #[serde(default)]
    pub files: Vec<MariadbFile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MariadbFile {
    pub file_name: String,
    #[serde(default)]
    pub file_download_url: Option<String>,
    #[serde(default)]
    pub checksum: Option<MariadbChecksum>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MariadbChecksum {
    #[serde(default)]
    pub sha256sum: Option<String>,
}

/// The official binary tarball for `platform` (mariadb.org publishes them
/// for Linux x86-64 only).
pub fn mariadb_official(
    latest: &MariadbLatest,
    major: &str,
    platform: Platform,
) -> Option<Release> {
    if platform.os != Os::Linux || platform.arch != Arch::X86_64 {
        return None;
    }
    let release = latest.releases.values().next()?;
    let wanted = format!("mariadb-{}-linux-systemd-x86_64.tar.gz", release.release_id);
    let file = release.files.iter().find(|f| f.file_name == wanted)?;
    let url = file.file_download_url.clone().unwrap_or_else(|| {
        format!(
            "https://archive.mariadb.org/mariadb-{0}/bintar-linux-systemd-x86_64/{wanted}",
            release.release_id
        )
    });
    Some(Release {
        kind: RuntimeKind::Mariadb,
        line: major.to_owned(),
        version: release.release_id.clone(),
        artifacts: vec![Artifact {
            url,
            sha256: file
                .checksum
                .as_ref()
                .and_then(|c| c.sha256sum.clone())
                .filter(|h| h.len() == 64),
            sha256_url: None,
            sha256_name: None,
            format: Format::TarGz,
            layout: Layout::StripTopDir,
        }],
    })
}

/// Pick the hash for `name` out of a sums file (`<hash>  <name>` per line),
/// or the first hash when `name` is `None`.
pub fn sha256_from_sums(text: &str, name: Option<&str>) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next().map(|f| f.trim_start_matches('*'));
        let wanted = match name {
            Some(name) => file == Some(name),
            None => true,
        };
        (wanted && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
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
    /// Host the runtime was downloaded from (`github.com` for RED's own
    /// builds, `dl.static-php.dev`, `nodejs.org`, ...). Missing in markers
    /// written before RED recorded it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// SHA-256 of the archive it came from, when the catalog published one,
    /// so a rebuilt archive of the same version (e.g. now with Xdebug) is
    /// offered as an update.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

/// The host part of `url`: `https://nodejs.org/dist/x` → `nodejs.org`.
pub fn url_host(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    let host = host.split(':').next()?;
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The version to update an installed runtime to, if any: a newer patch on
/// the same line, or RED's own build of the same version when the installed
/// one came from elsewhere (a fallback PHP build has fewer extensions) or
/// from an older build of it (RED rebuilt the archive, e.g. to add Xdebug).
pub fn update_for(
    installed_version: &str,
    installed_source: Option<&str>,
    installed_sha256: Option<&str>,
    release: &Release,
    red_builds_host: Option<&str>,
) -> Option<String> {
    match compare(&release.version, installed_version) {
        std::cmp::Ordering::Greater => Some(release.version.clone()),
        std::cmp::Ordering::Equal => {
            let artifact = release.artifacts.first();
            let release_source = artifact.and_then(|a| url_host(&a.url));
            let is_red_build =
                red_builds_host.is_some() && release_source.as_deref() == red_builds_host;
            let other_source = installed_source != release_source.as_deref();
            let rebuilt = artifact
                .and_then(|a| a.sha256.as_deref())
                .is_some_and(|sum| installed_sha256 != Some(sum));
            (is_red_build && (other_source || rebuilt)).then(|| release.version.clone())
        }
        std::cmp::Ordering::Less => None,
    }
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
    fn prefers_own_builds_from_the_manifest() {
        let manifest: Manifest = serde_json::from_str(
            r#"{"schema":1,"runtimes":[
              {"kind":"php","line":"8.4","version":"8.4.24","platform":"macos-aarch64",
               "url":"https://example.test/php-8.4.24-macos-aarch64.tar.gz","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
              {"kind":"valkey","line":"8.1","version":"8.1.4","platform":"macos-aarch64",
               "url":"https://example.test/valkey.tar.gz","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},
              {"kind":"valkey","line":"8.1","version":"8.1.4","platform":"linux-x86_64",
               "url":"https://example.test/valkey-linux.tar.gz","sha256":"bad"}
            ]}"#,
        )
        .unwrap();
        let listing = listing(&[
            "php-8.3.32-cli-macos-aarch64.tar.gz",
            "php-8.3.32-fpm-macos-aarch64.tar.gz",
            "php-8.4.23-cli-macos-aarch64.tar.gz",
            "php-8.4.23-fpm-macos-aarch64.tar.gz",
        ]);
        let lines = php_lines_from(&manifest, &listing, MAC_ARM);
        assert_eq!(
            lines,
            vec![
                ("8.4".to_owned(), "8.4.24".to_owned()),
                ("8.3".to_owned(), "8.3.32".to_owned())
            ]
        );
        let own = php_release_from(&manifest, &listing, "8.4", MAC_ARM).unwrap();
        assert_eq!(own.artifacts[0].layout, Layout::Flat);
        assert_eq!(own.artifacts[0].sha256.as_deref().map(str::len), Some(64));
        let fallback = php_release_from(&manifest, &listing, "8.3", MAC_ARM).unwrap();
        assert!(fallback.artifacts[0].url.contains("static-php"));
        assert_eq!(
            manifest
                .release(RuntimeKind::Valkey, "", MAC_ARM)
                .unwrap()
                .version,
            "8.1.4"
        );
        // Entries without a proper checksum are ignored.
        let linux = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        assert!(manifest.release(RuntimeKind::Valkey, "", linux).is_none());
    }

    #[test]
    fn picks_node_lts_lines() {
        let index: Vec<NodeEntry> = serde_json::from_str(
            r#"[
              {"version":"v26.1.0","lts":false,"files":["osx-arm64-tar"]},
              {"version":"v25.9.0","lts":false,"files":["osx-arm64-tar"]},
              {"version":"v24.11.0","lts":"Krypton","files":["osx-arm64-tar"]},
              {"version":"v24.10.0","lts":"Krypton","files":["osx-arm64-tar"]},
              {"version":"v22.21.0","lts":"Jod","files":["osx-arm64-tar"]},
              {"version":"v18.20.0","lts":"Hydrogen","files":["osx-arm64-tar"]}
            ]"#,
        )
        .unwrap();
        let lines = node_lines(&index, MAC_ARM);
        let majors: Vec<&str> = lines.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(majors, ["26", "24", "22"]);
        let default = node_release(&index, "", MAC_ARM).unwrap();
        assert_eq!(default.version, "24.11.0");
        assert_eq!(
            default.artifacts[0].url,
            "https://nodejs.org/dist/v24.11.0/node-v24.11.0-darwin-arm64.tar.gz"
        );
        assert_eq!(
            default.artifacts[0].sha256_name.as_deref(),
            Some("node-v24.11.0-darwin-arm64.tar.gz")
        );
        assert_eq!(
            node_release(&index, "22", MAC_ARM).unwrap().version,
            "22.21.0"
        );
    }

    #[test]
    fn picks_composer_and_reads_sums() {
        let versions: ComposerVersions = serde_json::from_str(
            r#"{"stable":[{"path":"/download/2.10.3/composer.phar","version":"2.10.3"},
                          {"path":"/download/2.2.30/composer.phar","version":"2.2.30"}]}"#,
        )
        .unwrap();
        let release = composer_release(&versions, "").unwrap();
        assert_eq!(release.version, "2.10.3");
        assert_eq!(release.artifacts[0].format, Format::Plain);
        let sums = "1111111111111111111111111111111111111111111111111111111111111111  a.tar.gz\n\
                    2222222222222222222222222222222222222222222222222222222222222222  b.tar.gz\n";
        assert_eq!(
            sha256_from_sums(sums, Some("b.tar.gz"))
                .unwrap()
                .chars()
                .next(),
            Some('2')
        );
        assert_eq!(
            sha256_from_sums(sums, None).unwrap().chars().next(),
            Some('1')
        );
        assert_eq!(sha256_from_sums(sums, Some("c.tar.gz")), None);
    }

    #[test]
    fn picks_bun_and_cloudflared_from_github() {
        let mac = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        let linux = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        let hash = "ab".repeat(32);
        let bun: GithubRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "bun-v1.4.3",
            "assets": [
                {"name": "bun-darwin-aarch64.zip", "browser_download_url": "https://x/bun-darwin-aarch64.zip", "digest": format!("sha256:{hash}")},
                {"name": "bun-linux-x64.zip", "browser_download_url": "https://x/bun-linux-x64.zip"}
            ]
        }))
        .unwrap();
        let release = bun_release(&bun, mac).unwrap();
        assert_eq!(
            (release.line.as_str(), release.version.as_str()),
            ("1", "1.4.3")
        );
        assert_eq!(release.artifacts[0].sha256.as_deref(), Some(hash.as_str()));
        assert_eq!(release.artifacts[0].format, Format::Zip);
        assert_eq!(
            release.artifacts[0].layout,
            Layout::Single {
                to: "bin/bun".into()
            }
        );
        assert_eq!(bun_release(&bun, linux).unwrap().artifacts[0].sha256, None);

        let cf: GithubRelease = serde_json::from_value(serde_json::json!({
            "tag_name": "2026.10.1",
            "assets": [
                {"name": "cloudflared-darwin-arm64.tgz", "browser_download_url": "https://x/a.tgz", "digest": format!("sha256:{hash}")},
                {"name": "cloudflared-linux-amd64", "browser_download_url": "https://x/l", "digest": format!("sha256:{hash}")}
            ]
        }))
        .unwrap();
        let release = cloudflared_release(&cf, mac).unwrap();
        assert_eq!(release.version, "2026.10.1");
        assert_eq!(release.artifacts[0].format, Format::TarGz);
        assert_eq!(
            cloudflared_release(&cf, linux).unwrap().artifacts[0].format,
            Format::Plain
        );
        assert!(
            cloudflared_release(
                &cf,
                Platform {
                    os: Os::Linux,
                    arch: Arch::Aarch64
                }
            )
            .is_none()
        );
    }

    #[test]
    fn picks_mariadb_releases() {
        let majors: MariadbMajors = serde_json::from_value(serde_json::json!({
            "major_releases": [
                {"release_id": "13.0", "release_status": "Stable", "release_support_type": "Rolling"},
                {"release_id": "12.3", "release_status": "Stable", "release_support_type": "Long Term Support"},
                {"release_id": "10.11", "release_status": "Stable", "release_support_type": "Long Term Support"},
                {"release_id": "11.4", "release_status": "Stable", "release_support_type": "Long Term Support"}
            ]
        }))
        .unwrap();
        assert_eq!(mariadb_lts_majors(&majors), ["12.3", "11.4", "10.11"]);
        let hash = "cd".repeat(32);
        let latest: MariadbLatest = serde_json::from_value(serde_json::json!({
            "releases": {"11.4.13": {"release_id": "11.4.13", "files": [
                {"file_name": "mariadb-11.4.13.tar.gz"},
                {"file_name": "mariadb-11.4.13-linux-systemd-x86_64.tar.gz",
                 "file_download_url": "https://dl/m.tar.gz", "checksum": {"sha256sum": hash}}
            ]}}
        }))
        .unwrap();
        let linux = Platform {
            os: Os::Linux,
            arch: Arch::X86_64,
        };
        let release = mariadb_official(&latest, "11.4", linux).unwrap();
        assert_eq!(release.line, "11.4");
        assert_eq!(release.version, "11.4.13");
        assert_eq!(release.artifacts[0].url, "https://dl/m.tar.gz");
        assert_eq!(release.artifacts[0].sha256.as_deref(), Some(hash.as_str()));
        assert_eq!(release.artifacts[0].layout, Layout::StripTopDir);
        let mac = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        assert!(mariadb_official(&latest, "11.4", mac).is_none());
    }

    #[test]
    fn matches_versions() {
        assert!(version_matches("8.4.23", "8.4"));
        assert!(version_matches("8.4.23", ""));
        assert!(!version_matches("8.4.23", "8.40"));
        assert!(version_matches("18.6.0", "18"));
    }

    #[test]
    fn finds_updates() {
        assert_eq!(
            url_host("https://nodejs.org/dist/v22/x.tar.gz").as_deref(),
            Some("nodejs.org")
        );
        assert_eq!(
            url_host("https://u@Example.COM:8443/a").as_deref(),
            Some("example.com")
        );
        assert_eq!(url_host("not a url"), None);

        let release = |version: &str, url: &str| Release {
            kind: RuntimeKind::Php,
            line: "8.4".into(),
            version: version.into(),
            artifacts: vec![Artifact {
                url: url.into(),
                sha256: url.contains("github.com").then(|| "aa".repeat(32)),
                sha256_url: None,
                sha256_name: None,
                format: Format::TarGz,
                layout: Layout::Flat,
            }],
        };
        let red = "https://github.com/masitings/laravel-red-project/releases/download/runtimes/php.tar.gz";
        let spc = "https://dl.static-php.dev/static-php-cli/bulk/php.tar.gz";
        let host = Some("github.com");
        // Newer patch.
        assert_eq!(
            update_for(
                "8.4.25",
                Some("github.com"),
                Some(&"aa".repeat(32)),
                &release("8.4.26", red),
                host
            )
            .as_deref(),
            Some("8.4.26")
        );
        // Same version, already RED's build.
        assert_eq!(
            update_for(
                "8.4.26",
                Some("github.com"),
                Some(&"aa".repeat(32)),
                &release("8.4.26", red),
                host
            ),
            None
        );
        // Same version, but a fallback build: switch to RED's.
        assert_eq!(
            update_for(
                "8.4.26",
                Some("dl.static-php.dev"),
                Some(&"aa".repeat(32)),
                &release("8.4.26", red),
                host
            )
            .as_deref(),
            Some("8.4.26")
        );
        assert_eq!(
            update_for(
                "8.4.26",
                None,
                Some(&"aa".repeat(32)),
                &release("8.4.26", red),
                host
            )
            .as_deref(),
            Some("8.4.26")
        );
        // Only a fallback build is available: nothing to switch to.
        assert_eq!(
            update_for(
                "8.4.26",
                None,
                Some(&"aa".repeat(32)),
                &release("8.4.26", spc),
                host
            ),
            None
        );
        // Older than installed.
        assert_eq!(
            update_for(
                "8.4.27",
                None,
                Some(&"aa".repeat(32)),
                &release("8.4.26", red),
                host
            ),
            None
        );
        // RED builds turned off.
        assert_eq!(
            update_for(
                "8.4.26",
                None,
                Some(&"aa".repeat(32)),
                &release("8.4.26", red),
                None
            ),
            None
        );
        // RED rebuilt the same version (new checksum): offer it.
        assert_eq!(
            update_for(
                "8.4.26",
                Some("github.com"),
                Some(&"bb".repeat(32)),
                &release("8.4.26", red),
                host
            )
            .as_deref(),
            Some("8.4.26")
        );
        // Installed before RED recorded checksums.
        assert_eq!(
            update_for(
                "8.4.26",
                Some("github.com"),
                None,
                &release("8.4.26", red),
                host
            )
            .as_deref(),
            Some("8.4.26")
        );
    }
}
