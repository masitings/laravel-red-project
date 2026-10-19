//! Downloading, verifying and unpacking runtimes (PHP, MySQL, PostgreSQL)
//! into `<data>/runtimes/<kind>/<line>/`.
//!
//! Installs run as background jobs; `GET /v1/runtimes` reports their
//! progress and `RuntimesChanged` events announce changes. A runtime is
//! unpacked into a staging directory and only renamed into place once it is
//! complete, so a failed or interrupted download never leaves a broken
//! runtime behind.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use red_api::{Event, JobState, RuntimeInfo, RuntimeJob, RuntimeRelease};
use red_core::runtimes::{
    self, Artifact, ComposerVersions, Format, Layout, ListingEntry, MARKER_FILE, Manifest, Marker,
    NodeEntry, Platform, Release, RuntimeKind,
};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::AppState;

/// Catalogs are fetched again after this long.
const CATALOG_TTL: Duration = Duration::from_secs(600);
/// How often redd looks for runtime updates (the first check runs shortly
/// after it starts).
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const FIRST_UPDATE_CHECK: Duration = Duration::from_secs(120);
/// Jobs kept in the list once finished.
const KEEP_JOBS: usize = 20;
/// Progress events are sent at most this often per job.
const PROGRESS_EVERY: Duration = Duration::from_millis(400);

/// What the remote catalogs said, for this platform.
#[derive(Debug, Clone, Default)]
struct Catalog {
    manifest: Manifest,
    php: Vec<ListingEntry>,
    postgres: Vec<String>,
    node: Vec<NodeEntry>,
    composer: ComposerVersions,
    errors: Vec<String>,
}

/// Owns runtime downloads.
#[derive(Debug)]
pub struct RuntimeManager {
    root: PathBuf,
    /// RED Project's own builds; `None` when turned off in `config.toml`.
    manifest_url: Option<String>,
    platform: Option<Platform>,
    events: tokio::sync::broadcast::Sender<Event>,
    jobs: Mutex<Vec<RuntimeJob>>,
    next_id: AtomicU64,
    catalog: tokio::sync::Mutex<Option<(Instant, Catalog)>>,
    http: std::sync::OnceLock<Result<reqwest::Client, String>>,
}

pub(crate) fn api_kind(kind: RuntimeKind) -> red_api::RuntimeKind {
    match kind {
        RuntimeKind::Php => red_api::RuntimeKind::Php,
        RuntimeKind::Mysql => red_api::RuntimeKind::Mysql,
        RuntimeKind::Postgres => red_api::RuntimeKind::Postgres,
        RuntimeKind::Valkey => red_api::RuntimeKind::Valkey,
        RuntimeKind::Node => red_api::RuntimeKind::Node,
        RuntimeKind::Composer => red_api::RuntimeKind::Composer,
    }
}

pub(crate) fn core_kind(kind: red_api::RuntimeKind) -> RuntimeKind {
    match kind {
        red_api::RuntimeKind::Php => RuntimeKind::Php,
        red_api::RuntimeKind::Mysql => RuntimeKind::Mysql,
        red_api::RuntimeKind::Postgres => RuntimeKind::Postgres,
        red_api::RuntimeKind::Valkey => RuntimeKind::Valkey,
        red_api::RuntimeKind::Node => RuntimeKind::Node,
        red_api::RuntimeKind::Composer => RuntimeKind::Composer,
    }
}

impl RuntimeManager {
    pub fn new(
        data_dir: &Path,
        manifest_url: Option<&str>,
        events: tokio::sync::broadcast::Sender<Event>,
    ) -> Self {
        let manifest_url = match manifest_url {
            Some("") => None,
            Some(url) => Some(url.to_owned()),
            None => Some(runtimes::DEFAULT_MANIFEST_URL.to_owned()),
        };
        Self {
            root: data_dir.join("runtimes"),
            manifest_url,
            platform: Platform::current(),
            events,
            jobs: Mutex::new(Vec::new()),
            next_id: AtomicU64::new(1),
            catalog: tokio::sync::Mutex::new(None),
            http: std::sync::OnceLock::new(),
        }
    }

    pub fn platform_name(&self) -> String {
        self.platform
            .map(Platform::name)
            .unwrap_or_else(|| "unsupported".into())
    }

    fn http(&self) -> Result<reqwest::Client, String> {
        self.http
            .get_or_init(|| {
                use rustls_platform_verifier::BuilderVerifierExt;
                let provider = Arc::new(rustls::crypto::ring::default_provider());
                let tls = rustls::ClientConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(|e| e.to_string())?
                    .with_platform_verifier()
                    .map_err(|e| e.to_string())?
                    .with_no_client_auth();
                reqwest::Client::builder()
                    .tls_backend_preconfigured(tls)
                    .user_agent(concat!("RED/", env!("CARGO_PKG_VERSION")))
                    .connect_timeout(Duration::from_secs(20))
                    .read_timeout(Duration::from_secs(60))
                    .build()
                    .map_err(|e| e.to_string())
            })
            .clone()
    }

    /// Runtimes installed by RED, by kind then newest line first.
    pub fn installed(&self) -> Vec<RuntimeInfo> {
        let mut found = Vec::new();
        for kind in RuntimeKind::ALL {
            let Ok(entries) = std::fs::read_dir(self.root.join(kind.as_str())) else {
                continue;
            };
            for entry in entries.flatten() {
                let dir = entry.path();
                let Ok(bytes) = std::fs::read(dir.join(MARKER_FILE)) else {
                    continue;
                };
                if let Ok(marker) = serde_json::from_slice::<Marker>(&bytes) {
                    found.push(RuntimeInfo {
                        kind: api_kind(marker.kind),
                        line: marker.line,
                        version: marker.version,
                        path: dir.display().to_string(),
                        installed: marker.installed,
                        source: marker.source,
                        update: None,
                    });
                }
            }
        }
        found.sort_by(|a, b| {
            (a.kind as u8)
                .cmp(&(b.kind as u8))
                .then_with(|| compare(&b.line, &a.line))
        });
        found
    }

    /// Fill in `update` for each runtime the catalogs have something newer
    /// for (see [`runtimes::update_for`]).
    pub async fn with_updates(&self, mut installed: Vec<RuntimeInfo>) -> Vec<RuntimeInfo> {
        let red_host = self.manifest_url.as_deref().and_then(runtimes::url_host);
        for info in &mut installed {
            let sha256 = std::fs::read(Path::new(&info.path).join(MARKER_FILE))
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Marker>(&bytes).ok())
                .and_then(|marker| marker.sha256);
            if let Ok(release) = self.resolve(core_kind(info.kind), &info.line).await
                && release.line == info.line
            {
                info.update = runtimes::update_for(
                    &info.version,
                    info.source.as_deref(),
                    sha256.as_deref(),
                    &release,
                    red_host.as_deref(),
                );
            }
        }
        installed
    }

    /// Recent jobs, newest first.
    pub fn jobs(&self) -> Vec<RuntimeJob> {
        let mut jobs = self.jobs.lock().expect("jobs poisoned").clone();
        jobs.reverse();
        jobs
    }

    async fn catalog(&self) -> Catalog {
        let mut cached = self.catalog.lock().await;
        if let Some((at, catalog)) = cached.as_ref()
            && at.elapsed() < CATALOG_TTL
            && catalog.errors.is_empty()
        {
            return catalog.clone();
        }
        let mut catalog = Catalog::default();
        let Some(platform) = self.platform else {
            catalog.errors.push("this platform is not supported".into());
            return catalog;
        };
        match self.http() {
            Err(err) => catalog.errors.push(format!("HTTP client: {err}")),
            Ok(http) => {
                let text = |url: String| {
                    let http = http.clone();
                    async move { http.get(url).send().await?.error_for_status()?.text().await }
                };
                let manifest = async {
                    match &self.manifest_url {
                        Some(url) => Some(text(url.clone()).await),
                        None => None,
                    }
                };
                let (manifest, php, pg, node, composer) = tokio::join!(
                    manifest,
                    text(runtimes::PHP_LISTING_URL.to_owned()),
                    text(runtimes::pg_metadata_url(platform)),
                    text(runtimes::NODE_INDEX_URL.to_owned()),
                    text(runtimes::COMPOSER_VERSIONS_URL.to_owned()),
                );
                match manifest {
                    None => {}
                    // Not published yet (404) is normal; RED falls back to
                    // upstream builds.
                    Some(Err(err)) if err.status().is_some_and(|s| s.as_u16() == 404) => {}
                    Some(Err(err)) => catalog.errors.push(format!("RED builds: {err}")),
                    Some(Ok(text)) => match serde_json::from_str::<Manifest>(&text) {
                        Ok(manifest) => catalog.manifest = manifest,
                        Err(err) => catalog
                            .errors
                            .push(format!("RED builds: bad manifest: {err}")),
                    },
                }
                match php.map(|text| serde_json::from_str::<Vec<ListingEntry>>(&text)) {
                    Ok(Ok(listing)) => catalog.php = listing,
                    Ok(Err(err)) => catalog
                        .errors
                        .push(format!("PHP builds: unexpected listing: {err}")),
                    Err(err) => catalog.errors.push(format!("PHP builds: {err}")),
                }
                match pg {
                    Ok(xml) => catalog.postgres = runtimes::parse_maven_versions(&xml),
                    Err(err) => catalog.errors.push(format!("PostgreSQL builds: {err}")),
                }
                match node.map(|text| serde_json::from_str::<Vec<NodeEntry>>(&text)) {
                    Ok(Ok(index)) => catalog.node = index,
                    Ok(Err(err)) => catalog
                        .errors
                        .push(format!("Node.js: unexpected index: {err}")),
                    Err(err) => catalog.errors.push(format!("Node.js: {err}")),
                }
                match composer.map(|text| serde_json::from_str::<ComposerVersions>(&text)) {
                    Ok(Ok(versions)) => catalog.composer = versions,
                    Ok(Err(err)) => catalog
                        .errors
                        .push(format!("Composer: unexpected versions: {err}")),
                    Err(err) => catalog.errors.push(format!("Composer: {err}")),
                }
            }
        }
        *cached = Some((Instant::now(), catalog.clone()));
        catalog
    }

    /// Newest version of every line available for this platform, and why
    /// some catalogs could not be read.
    pub async fn available(&self) -> (Vec<RuntimeRelease>, Vec<String>) {
        let catalog = self.catalog().await;
        let Some(platform) = self.platform else {
            return (Vec::new(), catalog.errors);
        };
        let mut releases = Vec::new();
        let mut push = |kind: RuntimeKind, line: String, version: String| {
            releases.push(RuntimeRelease {
                kind: api_kind(kind),
                line,
                version,
            });
        };
        for (line, version) in runtimes::php_lines_from(&catalog.manifest, &catalog.php, platform) {
            push(RuntimeKind::Php, line, version);
        }
        let mut mysql_lines: Vec<String> = Vec::new();
        for release in runtimes::mysql_releases(platform) {
            if !mysql_lines.contains(&release.line) {
                mysql_lines.push(release.line.clone());
                push(RuntimeKind::Mysql, release.line, release.version);
            }
        }
        for (line, version) in runtimes::pg_lines(&catalog.postgres) {
            push(RuntimeKind::Postgres, line, version);
        }
        for (line, version) in catalog.manifest.lines(RuntimeKind::Valkey, platform) {
            push(RuntimeKind::Valkey, line, version);
        }
        for (line, version) in runtimes::node_lines(&catalog.node, platform) {
            push(RuntimeKind::Node, line, version);
        }
        if let Some(release) = runtimes::composer_release(&catalog.composer, "") {
            push(RuntimeKind::Composer, release.line, release.version);
        }
        (releases, catalog.errors)
    }

    async fn resolve(&self, kind: RuntimeKind, wanted: &str) -> Result<Release, String> {
        let platform = self
            .platform
            .ok_or("RED cannot download runtimes for this platform")?;
        let catalog = self.catalog().await;
        let release = match kind {
            RuntimeKind::Php => {
                runtimes::php_release_from(&catalog.manifest, &catalog.php, wanted, platform)
            }
            RuntimeKind::Mysql => runtimes::mysql_release(wanted, platform),
            RuntimeKind::Postgres => runtimes::pg_release(&catalog.postgres, wanted, platform),
            RuntimeKind::Valkey => catalog
                .manifest
                .release(RuntimeKind::Valkey, wanted, platform),
            RuntimeKind::Node => runtimes::node_release(&catalog.node, wanted, platform),
            RuntimeKind::Composer => runtimes::composer_release(&catalog.composer, wanted),
        };
        release.ok_or_else(|| {
            let mut message = if wanted.is_empty() {
                format!("no {kind} build is available for {}", platform.name())
            } else {
                format!(
                    "no {kind} {wanted} build is available for {}",
                    platform.name()
                )
            };
            if !catalog.errors.is_empty() {
                message.push_str(&format!(" ({})", catalog.errors.join("; ")));
            }
            message
        })
    }

    fn update_job(&self, id: u64, change: impl FnOnce(&mut RuntimeJob)) {
        if let Some(job) = self
            .jobs
            .lock()
            .expect("jobs poisoned")
            .iter_mut()
            .find(|j| j.id == id)
        {
            change(job);
        }
        let _ = self.events.send(Event::RuntimesChanged);
    }

    /// Remove an installed runtime.
    pub fn remove(&self, kind: RuntimeKind, line: &str) -> Result<bool, String> {
        if !valid_line(line) {
            return Err(format!("{line:?} is not a version line"));
        }
        let dir = self.root.join(kind.as_str()).join(line);
        if !dir.join(MARKER_FILE).is_file() {
            return Ok(false);
        }
        std::fs::remove_dir_all(&dir)
            .map_err(|e| format!("cannot delete {}: {e}", dir.display()))?;
        let _ = self.events.send(Event::RuntimesChanged);
        Ok(true)
    }
}

/// Start installing `kind` `wanted`. Returns the job right away; the
/// download continues in the background.
pub async fn install(
    app: Arc<AppState>,
    kind: RuntimeKind,
    wanted: &str,
) -> Result<RuntimeJob, String> {
    let manager = &app.runtimes;
    let release = manager.resolve(kind, wanted).await?;
    {
        let jobs = manager.jobs.lock().expect("jobs poisoned");
        if let Some(running) = jobs.iter().find(|j| {
            j.kind == api_kind(kind)
                && j.line == release.line
                && matches!(j.state, JobState::Downloading | JobState::Extracting)
        }) {
            return Ok(running.clone());
        }
    }
    let job = RuntimeJob {
        id: manager.next_id.fetch_add(1, Ordering::Relaxed),
        kind: api_kind(kind),
        line: release.line.clone(),
        version: release.version.clone(),
        state: JobState::Downloading,
        received: 0,
        total: None,
        error: None,
    };
    {
        let mut jobs = manager.jobs.lock().expect("jobs poisoned");
        jobs.push(job.clone());
        let excess = jobs.len().saturating_sub(KEEP_JOBS);
        jobs.drain(..excess);
    }
    let _ = manager.events.send(Event::RuntimesChanged);
    let id = job.id;
    tokio::spawn(async move {
        let result = run(&app, id, &release).await;
        let manager = &app.runtimes;
        match result {
            Ok(()) => {
                tracing::info!(kind = %release.kind, version = %release.version, "runtime installed");
                manager.update_job(id, |j| j.state = JobState::Done);
                after_install(&app, &release).await;
            }
            Err(err) => {
                tracing::warn!(kind = %release.kind, version = %release.version, %err, "runtime install failed");
                manager.update_job(id, |j| {
                    j.state = JobState::Failed;
                    j.error = Some(err);
                });
            }
        }
    });
    Ok(job)
}

/// Look for runtime updates every few hours. New ones are announced with a
/// `RuntimesChanged` event (the app shows them) and, with
/// `runtimes_auto_update = true` in `config.toml`, installed right away.
pub async fn watch_updates(app: Arc<AppState>, cancel: tokio_util::sync::CancellationToken) {
    let mut known: Vec<(red_api::RuntimeKind, String, String)> = Vec::new();
    let mut wait = FIRST_UPDATE_CHECK;
    loop {
        tokio::select! {
            () = cancel.cancelled() => return,
            () = tokio::time::sleep(wait) => {}
        }
        wait = UPDATE_CHECK_EVERY;
        let installed = app.runtimes.installed();
        if installed.is_empty() {
            continue;
        }
        let updates: Vec<_> = app
            .runtimes
            .with_updates(installed)
            .await
            .into_iter()
            .filter_map(|i| i.update.map(|version| (i.kind, i.line, version)))
            .collect();
        if updates != known {
            for (kind, line, version) in &updates {
                tracing::info!(kind = %core_kind(*kind), %line, %version, "runtime update available");
            }
            let _ = app.events.send(Event::RuntimesChanged);
            known.clone_from(&updates);
        }
        if app.config.runtimes_auto_update {
            for (kind, line, _) in &updates {
                if let Err(err) = install(app.clone(), core_kind(*kind), line).await {
                    tracing::warn!(kind = %core_kind(*kind), %line, %err, "automatic runtime update failed");
                }
            }
        }
    }
}

/// Let the rest of the daemon see the new runtime.
async fn after_install(app: &AppState, release: &Release) {
    match release.kind {
        RuntimeKind::Php => {
            // A pool still running the replaced binary is restarted on its
            // next request with the new one.
            app.php.restart(&release.line).await;
            app.php.discover().await;
        }
        RuntimeKind::Mysql | RuntimeKind::Postgres | RuntimeKind::Valkey => {
            app.services.discover().await;
            let _ = app.events.send(Event::ServicesChanged);
        }
        RuntimeKind::Node | RuntimeKind::Composer => {}
    }
}

async fn run(app: &AppState, id: u64, release: &Release) -> Result<(), String> {
    let manager = &app.runtimes;
    let http = manager.http()?;
    let kind_dir = manager.root.join(release.kind.as_str());
    let downloads = manager.root.join(".downloads");
    let staging = kind_dir.join(format!(".staging-{}-{id}", release.line));
    for dir in [&kind_dir, &downloads] {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| e.to_string())?;

    let result = async {
        // Download everything first, so the total is known as early as possible.
        let mut files = Vec::new();
        let mut received_before = 0;
        for (index, artifact) in release.artifacts.iter().enumerate() {
            let file = downloads.join(format!("{id}-{index}"));
            let size = download(&http, artifact, &file, |received, total| {
                manager.update_job_throttled(
                    id,
                    received_before + received,
                    total.map(|t| received_before + t),
                );
            })
            .await?;
            received_before += size;
            files.push(file);
        }
        manager.update_job(id, |j| {
            j.state = JobState::Extracting;
            j.received = received_before;
            j.total = Some(received_before);
        });
        let staging_clone = staging.clone();
        let artifacts = release.artifacts.clone();
        let files_clone = files.clone();
        tokio::task::spawn_blocking(move || {
            for (artifact, file) in artifacts.iter().zip(&files_clone) {
                unpack(file, artifact.format, &artifact.layout, &staging_clone)?;
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|e| e.to_string())??;
        for file in &files {
            let _ = std::fs::remove_file(file);
        }
        finish(&staging, release)?;
        let dest = kind_dir.join(&release.line);
        // An update: services running the old build are stopped while it is
        // swapped and started again on the new one.
        let mut stopped = Vec::new();
        if dest.exists() {
            for (name, config) in app.state_snapshot().services {
                if config.binary.starts_with(&dest)
                    && app.services.runtime(&name).status != red_api::ServiceStatus::Stopped
                {
                    app.services.stop(&name).await;
                    stopped.push((name, config));
                }
            }
            // Keep the user's php.ini across updates.
            let ini = dest.join("etc/php.ini");
            if ini.is_file() {
                let _ = std::fs::copy(&ini, staging.join("etc/php.ini"));
            }
        }
        let swapped = swap_into_place(&staging, &dest);
        for (name, config) in &stopped {
            app.services.start(name, config).await;
        }
        swapped
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Replace `dest` with `staging`. The old directory is moved aside first
/// and put back if the rename fails, so a failed update keeps the old build.
fn swap_into_place(staging: &Path, dest: &Path) -> Result<(), String> {
    let old = dest.with_file_name(format!(
        ".old-{}",
        dest.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("runtime")
    ));
    let _ = std::fs::remove_dir_all(&old);
    let had_old = dest.exists();
    if had_old {
        std::fs::rename(dest, &old)
            .map_err(|e| format!("cannot replace {}: {e}", dest.display()))?;
    }
    if let Err(err) = std::fs::rename(staging, dest) {
        if had_old {
            let _ = std::fs::rename(&old, dest);
        }
        return Err(format!("cannot install into {}: {err}", dest.display()));
    }
    if had_old {
        let _ = std::fs::remove_dir_all(&old);
    }
    Ok(())
}

impl RuntimeManager {
    fn update_job_throttled(&self, id: u64, received: u64, total: Option<u64>) {
        static LAST: Mutex<Option<Instant>> = Mutex::new(None);
        let mut jobs = self.jobs.lock().expect("jobs poisoned");
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id) {
            job.received = received;
            if total.is_some() {
                job.total = total;
            }
        }
        drop(jobs);
        let mut last = LAST.lock().expect("throttle poisoned");
        if last.is_none_or(|t| t.elapsed() >= PROGRESS_EVERY) {
            *last = Some(Instant::now());
            let _ = self.events.send(Event::RuntimesChanged);
        }
    }
}

/// Download `artifact` to `dest`, checking its SHA-256 when one is known.
/// Returns the number of bytes.
async fn download(
    http: &reqwest::Client,
    artifact: &Artifact,
    dest: &Path,
    mut progress: impl FnMut(u64, Option<u64>),
) -> Result<u64, String> {
    let expected = match (&artifact.sha256, &artifact.sha256_url) {
        (Some(sum), _) => Some(sum.to_ascii_lowercase()),
        (None, Some(url)) => {
            let text = http
                .get(url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|e| format!("checksum for {}: {e}", artifact.url))?
                .text()
                .await
                .map_err(|e| e.to_string())?;
            Some(
                runtimes::sha256_from_sums(&text, artifact.sha256_name.as_deref())
                    .ok_or_else(|| format!("no checksum for {} in {url}", artifact.url))?,
            )
        }
        (None, None) => None,
    };
    let response = http
        .get(&artifact.url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("{}: {e}", artifact.url))?;
    let total = response.content_length();
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| format!("cannot write {}: {e}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("download interrupted: {e}"))?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        progress(received, total);
    }
    file.flush().await.map_err(|e| e.to_string())?;
    if let Some(total) = total
        && total != received
    {
        return Err(format!("download incomplete: {received} of {total} bytes"));
    }
    let actual = hex(&hasher.finalize());
    if let Some(expected) = expected
        && expected != actual
    {
        let _ = std::fs::remove_file(dest);
        return Err(format!(
            "checksum mismatch for {}: expected {expected}, got {actual}",
            artifact.url
        ));
    }
    Ok(received)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Unpack `file` into `dest` according to `layout`.
pub fn unpack(file: &Path, format: Format, layout: &Layout, dest: &Path) -> Result<(), String> {
    let open = || std::fs::File::open(file).map_err(|e| e.to_string());
    match format {
        Format::Plain => {
            let Layout::Single { to } = layout else {
                return Err("a plain file needs a target path".into());
            };
            let target = dest.join(to);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            std::fs::copy(file, &target).map_err(|e| e.to_string())?;
            set_executable(&target)
        }
        Format::TarGz => untar(flate2::read::GzDecoder::new(open()?), layout, dest),
        Format::TarXz => untar(
            lzma_rust2::XzReader::new(std::io::BufReader::new(open()?), true),
            layout,
            dest,
        ),
        Format::JarTxz => {
            let mut zip = zip::ZipArchive::new(open()?).map_err(|e| format!("bad archive: {e}"))?;
            let mut index = None;
            for i in 0..zip.len() {
                let entry = zip.by_index(i).map_err(|e| e.to_string())?;
                if entry.name().is_ok_and(|n| n.ends_with(".txz")) {
                    index = Some(i);
                    break;
                }
            }
            let index = index.ok_or("the archive holds no .txz")?;
            let entry = zip.by_index(index).map_err(|e| e.to_string())?;
            untar(
                lzma_rust2::XzReader::new(std::io::BufReader::new(entry), true),
                layout,
                dest,
            )
        }
    }
}

fn untar(reader: impl Read, layout: &Layout, dest: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(reader);
    archive.set_preserve_permissions(true);
    archive.set_overwrite(true);
    match layout {
        Layout::Flat => archive
            .unpack(dest)
            .map_err(|e| format!("cannot unpack: {e}")),
        Layout::StripTopDir => {
            let temp = dest.join(".unpack");
            archive
                .unpack(&temp)
                .map_err(|e| format!("cannot unpack: {e}"))?;
            let top: Vec<PathBuf> = std::fs::read_dir(&temp)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .collect();
            let [top] = top.as_slice() else {
                return Err("expected one top-level directory in the archive".into());
            };
            for entry in std::fs::read_dir(top).map_err(|e| e.to_string())?.flatten() {
                std::fs::rename(entry.path(), dest.join(entry.file_name()))
                    .map_err(|e| e.to_string())?;
            }
            std::fs::remove_dir_all(&temp).map_err(|e| e.to_string())
        }
        Layout::Single { to } => {
            let mut entries = archive.entries().map_err(|e| e.to_string())?;
            let mut entry = loop {
                let entry = entries
                    .next()
                    .ok_or("the archive is empty")?
                    .map_err(|e| e.to_string())?;
                if entry.header().entry_type().is_file() {
                    break entry;
                }
            };
            let target = dest.join(to);
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let mut out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            set_executable(&target)
        }
    }
}

fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())
}

/// Final touches inside the staging directory: `php.ini` for PHP, the
/// marker for every runtime, and a quick check that the binary runs.
fn finish(staging: &Path, release: &Release) -> Result<(), String> {
    let binary = match release.kind {
        RuntimeKind::Php => {
            let etc = staging.join("etc");
            std::fs::create_dir_all(&etc).map_err(|e| e.to_string())?;
            std::fs::write(etc.join("php.ini"), runtimes::PHP_INI).map_err(|e| e.to_string())?;
            staging.join("sbin/php-fpm")
        }
        RuntimeKind::Mysql => staging.join("bin/mysqld"),
        RuntimeKind::Postgres => staging.join("bin/postgres"),
        RuntimeKind::Valkey => staging.join("bin/valkey-server"),
        RuntimeKind::Node => staging.join("bin/node"),
        RuntimeKind::Composer => staging.join("bin/composer.phar"),
    };
    if !binary.is_file() {
        return Err(format!("{} is missing from the download", binary.display()));
    }
    if cfg!(target_os = "macos") {
        ensure_signed(staging);
    }
    // Composer is a PHP archive; running it needs PHP, so only its checksum
    // is checked. A `composer` script next to it makes it usable from a
    // shell once `red env` put RED's PHP on PATH.
    if release.kind == RuntimeKind::Composer {
        let script = staging.join("bin/composer");
        std::fs::write(
            &script,
            "#!/bin/sh\n# Added by RED Project: runs the composer.phar next to this script.\n\
             exec php \"$(dirname \"$0\")/composer.phar\" \"$@\"\n",
        )
        .map_err(|e| e.to_string())?;
        set_executable(&script)?;
    } else {
        check_runs(&binary, release.kind)?;
    }
    write_marker(staging, release)
}

fn check_runs(binary: &Path, kind: RuntimeKind) -> Result<(), String> {
    let output = std::process::Command::new(binary)
        .arg(if kind == RuntimeKind::Php {
            "-v"
        } else {
            "--version"
        })
        .output()
        .map_err(|e| format!("cannot run {}: {e}", binary.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut message = format!(
            "{} does not run on this machine: {}",
            binary.display(),
            stderr.trim()
        );
        if stderr.contains("libaio") {
            message.push_str(
                " (install libaio: `sudo apt install libaio1t64` and, on Ubuntu 24.04+, \
                 `sudo ln -s libaio.so.1t64 /usr/lib/x86_64-linux-gnu/libaio.so.1`)",
            );
        }
        return Err(message);
    }
    Ok(())
}

fn write_marker(staging: &Path, release: &Release) -> Result<(), String> {
    let marker = Marker {
        kind: release.kind,
        line: release.line.clone(),
        version: release.version.clone(),
        installed: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
        source: release
            .artifacts
            .first()
            .and_then(|a| runtimes::url_host(&a.url)),
        sha256: release.artifacts.first().and_then(|a| a.sha256.clone()),
    };
    std::fs::write(
        staging.join(MARKER_FILE),
        serde_json::to_vec_pretty(&marker).expect("marker serialises"),
    )
    .map_err(|e| e.to_string())
}

/// Apple Silicon refuses to run executables without a valid signature.
/// Give any executable in `bin/` or `sbin/` whose signature does not verify
/// an ad-hoc one, which is what a local build would have.
fn ensure_signed(root: &Path) {
    for dir in ["bin", "sbin"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.is_symlink() {
                continue;
            }
            let verified = std::process::Command::new("codesign")
                .arg("--verify")
                .arg(&path)
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
            if !verified {
                let signed = std::process::Command::new("codesign")
                    .args(["--force", "--sign", "-"])
                    .arg(&path)
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success());
                tracing::debug!(path = %path.display(), signed, "ad-hoc signed a runtime binary");
            }
        }
    }
}

/// A version line is digits and dots only (it becomes a directory name).
fn valid_line(line: &str) -> bool {
    !line.is_empty()
        && line.len() <= 16
        && !line.starts_with('.')
        && line.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

fn compare(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> { v.split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parts(a).cmp(&parts(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_gz(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (path, data) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn unpacks_single_files_and_trees() {
        let dir = tempfile::tempdir().unwrap();
        let single = dir.path().join("fpm.tar.gz");
        std::fs::write(&single, tar_gz(&[("php-fpm", b"binary")])).unwrap();
        let dest = dir.path().join("rt");
        std::fs::create_dir_all(&dest).unwrap();
        unpack(
            &single,
            Format::TarGz,
            &Layout::Single {
                to: "sbin/php-fpm".into(),
            },
            &dest,
        )
        .unwrap();
        assert_eq!(std::fs::read(dest.join("sbin/php-fpm")).unwrap(), b"binary");

        let tree = dir.path().join("mysql.tar.gz");
        std::fs::write(
            &tree,
            tar_gz(&[
                ("mysql-8.4/bin/mysqld", b"x"),
                ("mysql-8.4/share/errmsg.sys", b"y"),
            ]),
        )
        .unwrap();
        let dest = dir.path().join("mysql");
        std::fs::create_dir_all(&dest).unwrap();
        unpack(&tree, Format::TarGz, &Layout::StripTopDir, &dest).unwrap();
        assert!(dest.join("bin/mysqld").is_file());
        assert!(dest.join("share/errmsg.sys").is_file());
        assert!(!dest.join(".unpack").exists());
    }

    #[test]
    fn version_lines_cannot_escape() {
        assert!(valid_line("8.4"));
        assert!(!valid_line("../8"));
        assert!(!valid_line(".."));
        assert!(!valid_line(""));
    }
}
