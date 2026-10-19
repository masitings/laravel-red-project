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
    /// A project creation printed something, moved on, finished or failed.
    ProjectsChanged,
    /// A share started, got its URL, stopped or failed.
    SharesChanged,
    /// A Laravel site ran a database query (while capture is on).
    QueryReceived,
    /// Queries were cleared or capturing was switched on or off.
    QueriesChanged,
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
    /// What `red doctor --fix` and the app's Fix button do about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub action: Option<FixAction>,
}

/// A fix RED can apply by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum FixAction {
    /// Install the privileged helper (asks for the password).
    InstallHelper,
    /// Start redd through the login agent (also moves it onto the
    /// helper's ports).
    StartAtLogin,
    /// Send `.<tld>` to RED's DNS responder.
    InstallDns,
    /// Trust RED's local CA.
    TrustCa,
    /// Download RED's PHP build.
    InstallPhp,
    /// Restart services that failed.
    RestartServices,
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
    /// The `xdebug.so` RED loads for this version on demand, if it has one.
    #[serde(default)]
    pub xdebug: Option<String>,
    /// Xdebug is loaded by this installation's own `php.ini`, so it always
    /// runs and RED's on-demand switch does not apply.
    #[serde(default)]
    pub xdebug_loaded: bool,
    /// php.ini settings RED adds for this version (`PUT
    /// /v1/php/{version}/settings`).
    #[serde(default)]
    #[ts(type = "Record<string, string>")]
    pub settings: std::collections::BTreeMap<String, String>,
}

/// Body of `PUT /v1/php/{version}/settings`: the whole set, replacing the
/// previous one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpSettingsRequest {
    #[ts(type = "Record<string, string>")]
    pub settings: std::collections::BTreeMap<String, String>,
}

/// Response of `GET /v1/php/{version}/info`: what PHP itself reports, with
/// RED's settings applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpVersionInfo {
    pub version: String,
    /// The installation's own php.ini, if it reads one.
    pub ini_file: Option<String>,
    /// Loaded extensions, sorted.
    pub extensions: Vec<String>,
    /// Effective values of the common settings.
    #[ts(type = "Record<string, string>")]
    pub values: std::collections::BTreeMap<String, String>,
}

/// When sites run with Xdebug.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum XdebugMode {
    /// Never.
    Off,
    /// For requests with an Xdebug trigger (the `XDEBUG_SESSION` cookie, or
    /// `XDEBUG_SESSION_START` / `XDEBUG_TRIGGER` in the query).
    #[default]
    Auto,
    /// For every request.
    On,
}

/// Response of `GET /v1/php`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PhpListResponse {
    /// Version used by sites that are not isolated.
    pub default: Option<String>,
    /// Installed versions, newest first.
    pub installs: Vec<PhpInstallInfo>,
    /// When sites run with Xdebug.
    #[serde(default)]
    pub xdebug: XdebugMode,
    /// Port Xdebug connects to (where the IDE listens).
    #[serde(default)]
    pub xdebug_port: u16,
}

/// Body of `PUT /v1/xdebug`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct XdebugRequest {
    pub mode: XdebugMode,
}

// ---- queries -------------------------------------------------------------------

/// One database query a Laravel site ran.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueryEntry {
    #[ts(type = "number")]
    pub id: u64,
    /// Unix time in milliseconds.
    #[ts(type = "number")]
    pub received: u64,
    /// The SQL with `?` placeholders.
    pub sql: String,
    /// Binding values as SQL literals (`'text'`, `5`, `NULL`).
    pub bindings: Vec<String>,
    /// The SQL with the bindings filled in.
    pub bound: String,
    /// How long it took, in milliseconds.
    pub time_ms: f64,
    pub connection: Option<String>,
    /// `GET /users`, the request that ran it.
    pub request: Option<String>,
    /// Groups the queries of one request.
    pub request_id: Option<String>,
    /// Where in the app it came from (the first frame outside vendor/).
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Response of `GET /v1/queries`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QueriesResponse {
    /// When `true`, Laravel sites report their queries to RED.
    pub enabled: bool,
    /// Newest last.
    pub queries: Vec<QueryEntry>,
}

/// Body of `PUT /v1/queries/settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct QuerySettings {
    pub enabled: bool,
}

// ---- sharing -------------------------------------------------------------------

/// Where a share is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ShareState {
    /// Downloading cloudflared or connecting to Cloudflare.
    Starting,
    Running,
    Failed,
}

/// A site shared on the internet through a Cloudflare Tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShareInfo {
    pub site: String,
    /// The public URL, once Cloudflare assigned it.
    pub url: Option<String>,
    /// Your own hostname, for a share on your Cloudflare account; `None`
    /// for a one-off `trycloudflare.com` URL.
    pub hostname: Option<String>,
    pub state: ShareState,
    /// What it is doing, e.g. "Connecting to Cloudflare".
    pub step: String,
    pub error: Option<String>,
    /// Unix time it was started.
    #[ts(type = "number")]
    pub started: u64,
}

/// Response of `GET /v1/shares`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SharesResponse {
    pub shares: Vec<ShareInfo>,
    /// cloudflared is logged in to a Cloudflare account (for own hostnames).
    pub logged_in: bool,
    /// The cloudflared RED uses, if installed.
    pub cloudflared: Option<String>,
}

/// Body of `POST /v1/shares/{site}`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShareRequest {
    /// Serve the site at this hostname on your Cloudflare account (needs
    /// `POST /v1/shares/login` once). Without it, a one-off
    /// `*.trycloudflare.com` URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub hostname: Option<String>,
}

/// Response of `POST /v1/shares/login`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ShareLoginResponse {
    /// Open this in a browser to authorize cloudflared.
    pub url: Option<String>,
    pub logged_in: bool,
}

// ---- settings ------------------------------------------------------------------

/// Response of `GET /v1/settings`: RED's config file, and whether redd runs
/// with what it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsResponse {
    pub tld: String,
    pub http_port: u16,
    pub https_port: u16,
    pub smtp_port: u16,
    pub dump_port: u16,
    pub xdebug_port: u16,
    pub runtimes_auto_update: bool,
    /// Where these live (`config.toml`).
    pub config_file: String,
    /// The file changed since redd started; restart it to apply.
    pub restart_required: bool,
}

/// Body of `PUT /v1/settings`; fields left out stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SettingsUpdate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub smtp_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub dump_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub xdebug_port: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runtimes_auto_update: Option<bool>,
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

/// What `POST /v1/projects` creates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProjectKind {
    /// `laravel/laravel`.
    Laravel,
    /// Laravel's React starter kit.
    LaravelReact,
    /// Laravel's Vue starter kit.
    LaravelVue,
    /// Laravel's Livewire starter kit.
    LaravelLivewire,
    /// The latest WordPress, configured for a MySQL service.
    Wordpress,
    /// An empty site with `public/index.php`.
    Blank,
}

/// Body of `POST /v1/projects`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectRequest {
    /// Directory and site name: lowercase letters, digits and `-`.
    pub name: String,
    pub kind: ProjectKind,
    /// Absolute directory to create the project in. Defaults to the first
    /// parked directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub directory: Option<String>,
    /// Serve it over HTTPS.
    #[serde(default)]
    pub secure: bool,
    /// Service to create the project's database in (a MySQL or PostgreSQL
    /// service name). WordPress uses the first MySQL service when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub database: Option<String>,
}

/// Where a project creation is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProjectState {
    Running,
    Done,
    Failed,
}

/// One project creation started with `POST /v1/projects`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProjectJob {
    #[ts(type = "number")]
    pub id: u64,
    pub name: String,
    pub kind: ProjectKind,
    /// The project directory.
    pub path: String,
    pub state: ProjectState,
    /// What it is doing, e.g. "Running composer create-project".
    pub step: String,
    /// The last lines the tools printed.
    pub log: Vec<String>,
    /// The site's URL, once it is served.
    pub url: Option<String>,
    pub error: Option<String>,
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
    /// Switch to the installed server with this version (same flavor; for
    /// PostgreSQL the same major version, since data directories do not
    /// move between majors).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub version: Option<String>,
    /// Switch to this server binary (same rules as `version`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub binary: Option<String>,
}

/// A database in a MySQL, MariaDB or PostgreSQL service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseInfo {
    pub name: String,
    /// Size on disk in bytes, when the server reports it.
    #[ts(type = "number | null")]
    pub size: Option<u64>,
}

/// Response of `GET /v1/services/{name}/databases`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabasesResponse {
    pub service: String,
    pub databases: Vec<DatabaseInfo>,
    /// Backups of this service's databases, newest first.
    pub backups: Vec<BackupInfo>,
}

/// Body of `POST /v1/services/{name}/databases`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DatabaseRequest {
    pub name: String,
}

/// A backup file written by `POST .../databases/{db}/backup`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct BackupInfo {
    /// Absolute path of the `.sql.gz` file.
    pub file: String,
    /// The database it was taken from.
    pub database: String,
    #[ts(type = "number")]
    pub size: u64,
    /// Unix time it was written.
    #[ts(type = "number")]
    pub created: u64,
}

/// Body of `POST .../databases/{db}/restore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RestoreRequest {
    /// A `.sql` or `.sql.gz` file (a RED backup or any SQL dump).
    pub file: String,
    /// Create the database first if it does not exist (default `true`).
    #[serde(default = "yes")]
    pub create: bool,
}

fn yes() -> bool {
    true
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
    Valkey,
    Node,
    Composer,
    Mariadb,
    Bun,
    /// Cloudflare's tunnel client, for sharing sites.
    Cloudflared,
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
    /// Host it was downloaded from (`github.com` for RED's own builds).
    #[serde(default)]
    pub source: Option<String>,
    /// Version to update to (`POST /v1/runtimes` with this line), when the
    /// catalog has a newer build for the line, or RED's own build of a PHP
    /// that came from a fallback source.
    #[serde(default)]
    pub update: Option<String>,
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
