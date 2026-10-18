//! Request and response bodies of the Control API.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Response of `GET /v1/status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusResponse {
    /// Version of the running `redd` binary.
    pub version: String,
    /// Control API major version the daemon speaks.
    pub api_version: u32,
    /// Operating system the daemon runs on (`macos` or `linux`).
    pub os: String,
    /// Process id of the daemon.
    pub pid: u32,
    /// Seconds since the daemon started.
    #[ts(type = "number")]
    pub uptime_secs: u64,
    /// Top-level domain sites are served under.
    pub tld: String,
    /// Port the HTTP server listens on, or `null` if it could not start.
    pub http_port: Option<u16>,
    /// Port the HTTPS server listens on, or `null` if it could not start.
    pub https_port: Option<u16>,
    /// Loopback port of the DNS responder, or `null` if it could not start.
    pub dns_port: Option<u16>,
    /// `true` when ports 80/443 were opened by the privileged helper.
    pub helper: bool,
    /// Port of the mail catcher (SMTP) on 127.0.0.1, if it runs.
    pub smtp_port: Option<u16>,
    /// Port of the dump server on 127.0.0.1, if it runs.
    pub dump_port: Option<u16>,
    /// Where this installation keeps its files.
    pub paths: StatusPaths,
}

/// How a site came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SiteKind {
    /// A sub-directory of a parked path.
    Parked,
    /// Linked explicitly by name.
    Linked,
}

/// One served site, as returned by `GET /v1/sites`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SiteInfo {
    /// Site name; the site answers on `<name>.<tld>`.
    pub name: String,
    /// Project directory.
    pub path: String,
    /// URL to open the site in a browser.
    pub url: String,
    pub kind: SiteKind,
    /// Detected framework driver, e.g. `laravel`.
    pub driver: String,
    /// PHP version (`"8.3"`) the site runs on, or `null` if none is installed.
    pub php: Option<String>,
    /// `true` when the PHP version is pinned for this site.
    pub isolated: bool,
    /// `true` when the site is served over HTTPS.
    pub secure: bool,
    /// Expiry of the site's certificate (Unix time), when secured.
    #[ts(type = "number | null")]
    pub cert_expires: Option<u64>,
}

/// Something changed in the daemon. Sent on `GET /v1/events` as
/// Server-Sent Events; clients refetch what the event names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export)]
pub enum Event {
    /// Sent once when a client subscribes.
    Connected,
    /// Sites, parked paths, links, isolation or HTTPS changed.
    SitesChanged,
    /// Proxies changed.
    ProxiesChanged,
    /// PHP installations, the default version or running pools changed.
    PhpChanged,
    /// Services were added, removed, started, stopped or failed.
    ServicesChanged,
    /// A message reached the mail catcher.
    MailReceived { id: String },
    /// Messages were deleted or marked as read.
    MailChanged,
    /// A `dump()` reached the dump server.
    DumpReceived,
    /// Dumps were cleared or capturing was switched on or off.
    DumpsChanged,
    /// A runtime download progressed, finished or failed, or a runtime was
    /// removed.
    RuntimesChanged,
}

/// Outcome of one doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum CheckStatus {
    Ok,
    /// Works, but not the way RED is meant to run (e.g. fallback ports).
    Warn,
    /// Broken; sites will not work until fixed.
    Fail,
}

/// One check in `GET /v1/doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Check {
    pub id: String,
    pub label: String,
    pub status: CheckStatus,
    pub detail: String,
    /// Command or action that fixes the problem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub fix: Option<String>,
}

/// Response of `GET /v1/doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DoctorReport {
    pub checks: Vec<Check>,
}

/// One proxy, as returned by `GET /v1/proxies`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyInfo {
    /// The proxy answers on `<name>.<tld>`.
    pub name: String,
    /// Upstream URL, e.g. `http://127.0.0.1:5173`.
    pub target: String,
    /// URL to open in a browser.
    pub url: String,
    pub secure: bool,
    #[ts(type = "number | null")]
    pub cert_expires: Option<u64>,
}

/// Body of `POST /v1/proxies`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProxyRequest {
    pub name: String,
    /// A port (`5173`), `host:port`, or an `http://` URL.
    pub target: String,
}

/// Body of `POST /v1/paths` and `POST /v1/links`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PathRequest {
    /// Absolute directory path.
    pub path: String,
    /// Site name for links. Defaults to the directory name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
}

/// Body of `PUT /v1/php/default` and `PUT /v1/sites/{name}/php`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpVersionRequest {
    /// Minor version, e.g. `"8.3"`.
    pub version: String,
}

/// One PHP installation RED found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpInstallInfo {
    /// Minor version, e.g. `"8.3"`.
    pub version: String,
    /// Full version, e.g. `"8.3.6"`.
    pub full_version: String,
    /// Path of the `php-fpm` binary.
    pub fpm_path: String,
    /// `true` while RED runs a PHP-FPM pool for this version.
    pub running: bool,
}

/// Response of `GET /v1/php`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpListResponse {
    /// Version used by sites that are not isolated.
    pub default: Option<String>,
    /// Installed versions, newest first.
    pub installs: Vec<PhpInstallInfo>,
}

// ---- services ------------------------------------------------------------------

/// Kind of server a service runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServiceKind {
    /// MySQL or MariaDB.
    Mysql,
    /// PostgreSQL.
    Postgres,
    /// Redis or Valkey.
    Redis,
}

/// Whether a service is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ServiceStatus {
    Stopped,
    /// Started, not accepting connections yet.
    Starting,
    Running,
    /// Could not start or kept crashing; see `error` and the log.
    Failed,
}

/// A `NAME=value` pair, e.g. for a project's `.env`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

/// One service instance, as returned by `GET /v1/services`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServiceInfo {
    /// Instance name, e.g. `mysql` or `postgres-15`.
    pub name: String,
    pub kind: ServiceKind,
    /// `mysql`, `mariadb`, `postgresql`, `redis` or `valkey`.
    pub flavor: Option<String>,
    /// Server version, e.g. `16.4`.
    pub version: Option<String>,
    /// The server binary.
    pub binary: String,
    /// TCP port on 127.0.0.1.
    pub port: u16,
    /// Started whenever redd starts.
    pub autostart: bool,
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    /// Why the service failed, when `status` is `failed`.
    pub error: Option<String>,
    pub data_dir: String,
    pub log_file: String,
    /// Settings for a Laravel `.env`.
    pub env: Vec<EnvVar>,
}

/// A server binary RED found and can run as a service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServerInstall {
    pub kind: ServiceKind,
    pub flavor: String,
    /// `major.minor`, e.g. `10.11`.
    pub version: String,
    pub full_version: String,
    pub binary: String,
}

/// Response of `GET /v1/services`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServicesResponse {
    pub services: Vec<ServiceInfo>,
    /// Server binaries found on this machine.
    pub available: Vec<ServerInstall>,
}

/// Body of `POST /v1/services`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServiceRequest {
    pub kind: ServiceKind,
    /// Instance name. Defaults to the kind (`mysql`, `postgres`, `redis`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// Defaults to the kind's usual port (3306, 5432, 6379).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub port: Option<u16>,
    /// Pick the installed server with this version (prefix match, e.g. `16`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    /// Use this server binary instead of a discovered one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub binary: Option<String>,
    /// Defaults to `true`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub autostart: Option<bool>,
}

/// Body of `PUT /v1/services/{name}`; absent fields stay unchanged.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ServiceUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub autostart: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub port: Option<u16>,
}

// ---- mail ------------------------------------------------------------------------

/// A caught message, as listed by `GET /v1/mail`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailSummary {
    pub id: String,
    /// The SMTP username the app logged in with (Laravel: `MAIL_USERNAME`),
    /// used to tell apps apart.
    pub mailbox: Option<String>,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    /// Unix time the message arrived.
    #[ts(type = "number")]
    pub received: u64,
    #[ts(type = "number")]
    pub size: u64,
    pub has_html: bool,
    pub attachments: u32,
    pub read: bool,
}

/// One header line of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailHeader {
    pub name: String,
    pub value: String,
}

/// An attachment of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailAttachment {
    pub index: u32,
    pub filename: String,
    pub content_type: String,
    #[ts(type = "number")]
    pub size: u64,
}

/// Response of `GET /v1/mail/{id}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct MailMessage {
    pub summary: MailSummary,
    pub cc: Vec<String>,
    /// Envelope recipients (`RCPT TO`), which include Bcc addresses.
    pub envelope_to: Vec<String>,
    pub text: Option<String>,
    pub html: Option<String>,
    pub headers: Vec<MailHeader>,
    pub attachments: Vec<MailAttachment>,
}

// ---- dumps -----------------------------------------------------------------------

/// Where a `dump()` call was made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DumpSource {
    pub file: String,
    pub line: Option<u32>,
    /// File name as shown to the user (relative to the project when known).
    pub name: String,
}

/// One captured dump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DumpEntry {
    #[ts(type = "number")]
    pub id: u64,
    /// Unix time in milliseconds.
    #[ts(type = "number")]
    pub received: u64,
    /// The dumped value rendered as HTML by the project's own VarDumper.
    pub html: String,
    pub source: Option<DumpSource>,
    /// Request (`GET /path`) or command line the dump came from.
    pub origin: Option<String>,
}

/// Response of `GET /v1/dumps`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DumpsResponse {
    /// When `true`, sites send `dump()` output to RED instead of the page.
    pub enabled: bool,
    pub port: Option<u16>,
    /// Newest last.
    pub dumps: Vec<DumpEntry>,
}

/// Body of `PUT /v1/dumps/settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DumpSettings {
    pub enabled: bool,
}

// ---- logs ------------------------------------------------------------------------

/// What a log file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum LogKind {
    /// redd itself.
    Daemon,
    /// A database or cache service.
    Service,
    /// A PHP-FPM pool.
    Php,
    /// A site's own log, e.g. `storage/logs/laravel.log`.
    Site,
}

/// A log file RED knows about, as listed by `GET /v1/logs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogFile {
    /// Pass to `GET /v1/logs/read?id=`.
    pub id: String,
    pub kind: LogKind,
    pub label: String,
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    /// Unix time of the last change.
    #[ts(type = "number")]
    pub modified: u64,
}

/// One entry of a log. Laravel/Monolog lines are split into their parts;
/// other logs have one entry per line with only `message` set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogEntry {
    pub time: Option<String>,
    /// Lower-case level, e.g. `error`.
    pub level: Option<String>,
    pub channel: Option<String>,
    /// The message, including any stack trace that followed it.
    pub message: String,
}

/// Response of `GET /v1/logs/read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LogResponse {
    pub file: LogFile,
    /// Oldest first.
    pub entries: Vec<LogEntry>,
    /// `true` when older entries were left out.
    pub truncated: bool,
}

// ---- runtimes --------------------------------------------------------------------

/// Something RED can download and manage itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RuntimeKind {
    Php,
    Mysql,
    Postgres,
}

/// A runtime installed by RED.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeInfo {
    pub kind: RuntimeKind,
    /// The line, e.g. `8.4` or `18`; also the directory name.
    pub line: String,
    /// Exact version, e.g. `8.4.23`.
    pub version: String,
    pub path: String,
    /// Unix time of the installation.
    #[ts(type = "number")]
    pub installed: u64,
}

/// A version RED can download.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeRelease {
    pub kind: RuntimeKind,
    pub line: String,
    pub version: String,
}

/// Progress of a download.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum JobState {
    Downloading,
    Extracting,
    Done,
    Failed,
}

/// One install started with `POST /v1/runtimes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeJob {
    #[ts(type = "number")]
    pub id: u64,
    pub kind: RuntimeKind,
    pub line: String,
    pub version: String,
    pub state: JobState,
    /// Bytes downloaded so far, over all files.
    #[ts(type = "number")]
    pub received: u64,
    /// Total bytes to download, when the server said.
    #[ts(type = "number | null")]
    pub total: Option<u64>,
    pub error: Option<String>,
}

/// Response of `GET /v1/runtimes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimesResponse {
    /// `macos-aarch64`, `linux-x86_64`, ...
    pub platform: String,
    pub installed: Vec<RuntimeInfo>,
    /// Newest version of every line RED can download for this platform.
    pub available: Vec<RuntimeRelease>,
    /// Recent and running installs, newest first.
    pub jobs: Vec<RuntimeJob>,
    /// Why some catalogs could not be fetched (offline, ...).
    pub catalog_errors: Vec<String>,
}

/// Body of `POST /v1/runtimes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RuntimeRequest {
    pub kind: RuntimeKind,
    /// A line (`8.4`), an exact version, or empty for the newest.
    #[serde(default)]
    pub version: String,
}

/// Directories reported by [`StatusResponse`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct StatusPaths {
    pub config_dir: String,
    pub data_dir: String,
    pub socket: String,
    /// The local CA certificate to add to the system trust store.
    pub ca_cert: String,
}

/// Machine-readable error category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ErrorCode {
    /// The path or resource does not exist.
    NotFound,
    /// The request was malformed or failed validation.
    BadRequest,
    /// The request conflicts with the current state.
    Conflict,
    /// Something went wrong inside the daemon.
    Internal,
    /// A required component (such as the helper) is not available.
    Unavailable,
}

/// Body of every non-2xx response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ApiError {
    pub code: ErrorCode,
    /// Human-readable description of what went wrong.
    pub message: String,
    /// Suggested next step, shown by the CLI and GUI when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hint: Option<String>,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " (hint: {hint})")?;
        }
        Ok(())
    }
}

impl std::error::Error for ApiError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_is_snake_case() {
        let json = serde_json::to_string(&ErrorCode::NotFound).unwrap();
        assert_eq!(json, "\"not_found\"");
    }

    #[test]
    fn hint_is_omitted_when_absent() {
        let json = serde_json::to_value(ApiError::new(ErrorCode::Internal, "boom")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"code": "internal", "message": "boom"})
        );
    }
}
