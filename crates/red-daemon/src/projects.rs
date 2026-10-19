//! Creating new projects: Laravel (plain or a starter kit), WordPress or an
//! empty site, ready to open in the browser.
//!
//! A creation runs in the background as a job. It writes the files, creates
//! the project's database in a RED service, points the app's settings at
//! RED (database, mail catcher, URL), and serves the directory as a site.
//! What the tools print is kept so the app can show progress.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use red_api::{Event, ProjectJob, ProjectKind, ProjectRequest, ProjectState, ServiceStatus};
use red_core::services::ServiceKind;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::AppState;

/// Lines of output kept per job.
const LOG_LINES: usize = 300;
/// Finished jobs kept for the app to show.
const KEEP_JOBS: usize = 10;
const WORDPRESS_URL: &str = "https://wordpress.org/latest.tar.gz";

/// Running and recent project creations.
#[derive(Debug, Default)]
pub struct Projects {
    jobs: std::sync::Mutex<Vec<Job>>,
    next_id: AtomicU64,
}

#[derive(Debug)]
struct Job {
    info: ProjectJob,
    log: VecDeque<String>,
    /// When subscribers last heard about new output.
    notified: Instant,
}

impl Projects {
    pub fn list(&self) -> Vec<ProjectJob> {
        self.jobs
            .lock()
            .expect("projects poisoned")
            .iter()
            .map(|j| ProjectJob {
                log: j.log.iter().cloned().collect(),
                ..j.info.clone()
            })
            .collect()
    }

    fn add(&self, info: ProjectJob) {
        let mut jobs = self.jobs.lock().expect("projects poisoned");
        jobs.push(Job {
            info,
            log: VecDeque::new(),
            notified: Instant::now(),
        });
        let finished = jobs
            .iter()
            .filter(|j| j.info.state != ProjectState::Running)
            .count();
        if finished > KEEP_JOBS
            && let Some(oldest) = jobs
                .iter()
                .position(|j| j.info.state != ProjectState::Running)
        {
            jobs.remove(oldest);
        }
    }

    /// Change job `id`; returns whether subscribers should hear about it.
    fn update(&self, id: u64, change: impl FnOnce(&mut Job)) -> bool {
        let mut jobs = self.jobs.lock().expect("projects poisoned");
        let Some(job) = jobs.iter_mut().find(|j| j.info.id == id) else {
            return false;
        };
        change(job);
        true
    }

    fn running(&self, path: &Path) -> bool {
        self.jobs
            .lock()
            .expect("projects poisoned")
            .iter()
            .any(|j| j.info.state == ProjectState::Running && Path::new(&j.info.path) == path)
    }
}

/// Handle to one job: report steps and output.
struct Reporter {
    app: Arc<AppState>,
    id: u64,
}

impl Reporter {
    fn step(&self, step: &str) {
        tracing::info!(project = self.id, step, "project");
        self.app.projects.update(self.id, |job| {
            job.info.step = step.to_owned();
            push_line(&mut job.log, format!("==> {step}"));
        });
        self.app.notify(Event::ProjectsChanged);
    }

    fn line(&self, line: String) {
        let mut notify = false;
        self.app.projects.update(self.id, |job| {
            push_line(&mut job.log, line);
            // At most a few updates a second while a tool is chatty.
            if job.notified.elapsed() >= Duration::from_millis(400) {
                job.notified = Instant::now();
                notify = true;
            }
        });
        if notify {
            self.app.notify(Event::ProjectsChanged);
        }
    }

    fn finish(&self, result: Result<String, String>) {
        self.app.projects.update(self.id, |job| match result {
            Ok(url) => {
                job.info.state = ProjectState::Done;
                job.info.step = "Ready".to_owned();
                job.info.url = Some(url);
            }
            Err(err) => {
                push_line(&mut job.log, format!("error: {err}"));
                job.info.state = ProjectState::Failed;
                job.info.error = Some(err);
            }
        });
        self.app.notify(Event::ProjectsChanged);
    }
}

fn push_line(log: &mut VecDeque<String>, line: String) {
    // Progress bars redraw with \r; keep the last state of the line.
    let line = line
        .rsplit('\r')
        .next()
        .unwrap_or_default()
        .trim_end()
        .to_owned();
    if line.is_empty() {
        return;
    }
    log.push_back(line);
    while log.len() > LOG_LINES {
        log.pop_front();
    }
}

/// `true` for a name that works as a directory, a host name and (with `-`
/// as `_`) a database name.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Database name for a project name.
pub fn database_name(name: &str) -> String {
    name.replace('-', "_")
}

/// Composer package that `composer create-project` starts from.
pub fn package(kind: ProjectKind) -> Option<&'static str> {
    match kind {
        ProjectKind::Laravel => Some("laravel/laravel"),
        ProjectKind::LaravelReact => Some("laravel/react-starter-kit"),
        ProjectKind::LaravelVue => Some("laravel/vue-starter-kit"),
        ProjectKind::LaravelLivewire => Some("laravel/livewire-starter-kit"),
        ProjectKind::Wordpress | ProjectKind::Blank => None,
    }
}

/// Starter kits come with a frontend that has to be built.
fn needs_frontend(kind: ProjectKind) -> bool {
    matches!(
        kind,
        ProjectKind::LaravelReact | ProjectKind::LaravelVue | ProjectKind::LaravelLivewire
    )
}

/// A database service chosen for the project.
#[derive(Debug, Clone)]
struct Database {
    service: String,
    kind: ServiceKind,
    port: u16,
    binary: PathBuf,
    name: String,
}

/// Start creating a project. Checks what can be checked up front, so the
/// app gets a clear error instead of a failed job.
pub fn start(app: Arc<AppState>, request: ProjectRequest) -> Result<ProjectJob, String> {
    let name = request.name.trim().to_owned();
    if !valid_name(&name) {
        return Err(format!(
            "{name:?} is not a valid project name; use lowercase letters, digits and '-'"
        ));
    }
    let state = app.state_snapshot();
    let parent = match &request.directory {
        Some(dir) => PathBuf::from(dir),
        None => state
            .parked
            .first()
            .cloned()
            .ok_or("choose a directory, or park one first (`red park`)")?,
    };
    if !parent.is_absolute() || !parent.is_dir() {
        return Err(format!("{} is not a directory", parent.display()));
    }
    let path = parent.join(&name);
    if path.exists() || app.projects.running(&path) {
        return Err(format!("{} already exists", path.display()));
    }
    if red_core::sites::find(&state, &name).is_some() {
        return Err(format!("a site named {name} already exists"));
    }
    let database = choose_database(&app, &request, &name)?;
    if package(request.kind).is_some() {
        php_cli(&app).ok_or("no PHP installed; install one on the PHP page")?;
        composer(&app).ok_or("Composer is not installed; install it on the PHP page")?;
    }

    let info = ProjectJob {
        id: app.projects.next_id.fetch_add(1, Ordering::Relaxed) + 1,
        name: name.clone(),
        kind: request.kind,
        path: path.display().to_string(),
        state: ProjectState::Running,
        step: "Starting".to_owned(),
        log: Vec::new(),
        url: None,
        error: None,
    };
    app.projects.add(info.clone());
    app.notify(Event::ProjectsChanged);
    let reporter = Reporter {
        app: app.clone(),
        id: info.id,
    };
    let secure = request.secure;
    let kind = request.kind;
    tokio::spawn(async move {
        let result = create(&reporter, kind, &name, &parent, &path, database, secure).await;
        if let Err(err) = &result {
            tracing::warn!(%err, project = %name, "project creation failed");
            // The directory did not exist before; remove what was made of
            // it so creating the project again works.
            if path.exists() && std::fs::remove_dir_all(&path).is_ok() {
                reporter.line(format!("Removed the unfinished {}", path.display()));
            }
        }
        reporter.finish(result);
    });
    Ok(info)
}

fn choose_database(
    app: &AppState,
    request: &ProjectRequest,
    name: &str,
) -> Result<Option<Database>, String> {
    if request.kind == ProjectKind::Blank && request.database.is_none() {
        return Ok(None);
    }
    let state = app.state_snapshot();
    let chosen = match &request.database {
        Some(service) => Some(
            state
                .services
                .get_key_value(service)
                .ok_or_else(|| format!("no service named {service}"))?,
        ),
        // WordPress needs MySQL; Laravel uses SQLite unless asked.
        None if request.kind == ProjectKind::Wordpress => Some(
            state
                .services
                .iter()
                .find(|(_, c)| c.kind == ServiceKind::Mysql)
                .ok_or("WordPress needs MySQL; add a MySQL service on the Services page")?,
        ),
        None => None,
    };
    let Some((service, config)) = chosen else {
        return Ok(None);
    };
    if config.kind == ServiceKind::Redis {
        return Err(format!("{service} is not a database"));
    }
    if request.kind == ProjectKind::Wordpress && config.kind != ServiceKind::Mysql {
        return Err("WordPress needs a MySQL service".to_owned());
    }
    Ok(Some(Database {
        service: service.clone(),
        kind: config.kind,
        port: config.port,
        binary: config.binary.clone(),
        name: database_name(name),
    }))
}

/// PHP CLI of the default version, else the newest.
fn php_cli(app: &AppState) -> Option<PathBuf> {
    let state = app.state_snapshot();
    state
        .default_php
        .as_deref()
        .and_then(|v| app.php.find(v))
        .and_then(|i| red_platform::binaries::php_cli_for_fpm(&i.fpm))
        .or_else(|| app.php_cli_binaries().into_iter().next())
}

/// RED's Composer (a phar run with PHP), else `composer` on `PATH`.
fn composer(app: &AppState) -> Option<PathBuf> {
    red_platform::binaries::red_composer(&app.paths.data_dir).or_else(|| {
        red_platform::binaries::in_path("composer")
            .into_iter()
            .next()
    })
}

fn npm(app: &AppState) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    red_platform::binaries::node_candidates(&home, &app.paths.data_dir)
        .into_iter()
        .filter_map(|node| node.parent().map(|dir| dir.join("npm")))
        .find(|npm| npm.is_file())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    r: &Reporter,
    kind: ProjectKind,
    name: &str,
    parent: &Path,
    path: &Path,
    database: Option<Database>,
    secure: bool,
) -> Result<String, String> {
    let app = &r.app;
    let php = php_cli(app);
    // Tools find PHP (and Node for starter kits) first on PATH.
    let mut path_dirs: Vec<PathBuf> = Vec::new();
    if let Some(dir) = php.as_ref().and_then(|p| p.parent()) {
        path_dirs.push(dir.to_path_buf());
    }
    let npm = npm(app);
    if let Some(dir) = npm.as_ref().and_then(|p| p.parent()) {
        path_dirs.push(dir.to_path_buf());
    }
    let env_path = std::env::join_paths(path_dirs.iter().cloned().chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .map_err(|e| e.to_string())?;
    let tool = |program: &Path| {
        let mut command = tokio::process::Command::new(program);
        command
            .env("PATH", &env_path)
            .env("COMPOSER_NO_INTERACTION", "1")
            .env("NO_COLOR", "1");
        command
    };

    // 1. Files.
    match kind {
        ProjectKind::Blank => {
            r.step("Creating an empty site");
            let public = path.join("public");
            std::fs::create_dir_all(&public).map_err(|e| e.to_string())?;
            std::fs::write(public.join("index.php"), blank_index(name))
                .map_err(|e| e.to_string())?;
        }
        ProjectKind::Wordpress => {
            r.step("Downloading WordPress");
            download_wordpress(app, path).await?;
        }
        _ => {
            let package = package(kind).expect("Laravel kinds have a package");
            let php = php.clone().ok_or("no PHP installed")?;
            let composer = composer(app).ok_or("Composer is not installed")?;
            r.step(&format!("Running composer create-project {package}"));
            let mut command = if composer.extension().is_some_and(|e| e == "phar") {
                let mut command = tool(&php);
                command.arg(&composer);
                command
            } else {
                tool(&composer)
            };
            command
                .args(["create-project", "--prefer-dist", "--no-interaction"])
                .arg(package)
                .arg(path)
                .current_dir(parent);
            run(r, command).await?;
        }
    }

    // 2. Database.
    if let Some(db) = &database {
        r.step(&format!("Creating database {} in {}", db.name, db.service));
        ensure_running(app, &db.service).await?;
        create_database(r, db).await?;
    }

    // 3. Settings.
    let url = planned_url(app, name, secure);
    let smtp = app.smtp_port.get().copied();
    match kind {
        ProjectKind::Blank => {}
        ProjectKind::Wordpress => {
            r.step("Writing wp-config.php");
            let db = database.as_ref().ok_or("WordPress needs a database")?;
            let sample = std::fs::read_to_string(path.join("wp-config-sample.php"))
                .map_err(|e| format!("wp-config-sample.php: {e}"))?;
            let config = wp_config(&sample, &db.name, db.port, &random_salts()?);
            std::fs::write(path.join("wp-config.php"), config).map_err(|e| e.to_string())?;
        }
        _ => {
            r.step("Pointing .env at RED");
            let env_file = path.join(".env");
            let env = std::fs::read_to_string(&env_file).map_err(|e| format!(".env: {e}"))?;
            let env = apply_env(&env, &laravel_settings(name, &url, smtp, database.as_ref()));
            std::fs::write(&env_file, env).map_err(|e| e.to_string())?;
            if database.is_some() {
                r.step("Running migrations");
                let php = php.clone().ok_or("no PHP installed")?;
                let mut command = tool(&php);
                command
                    .args(["artisan", "migrate", "--force", "--no-interaction"])
                    .current_dir(path);
                run(r, command).await?;
            }
            if needs_frontend(kind) {
                match &npm {
                    Some(npm) => {
                        r.step("Installing frontend packages (npm install)");
                        let mut command = tool(npm);
                        command.arg("install").current_dir(path);
                        run(r, command).await?;
                        r.step("Building the frontend (npm run build)");
                        let mut command = tool(npm);
                        command.args(["run", "build"]).current_dir(path);
                        run(r, command).await?;
                    }
                    None => r.line(
                        "Node.js was not found; install it on the PHP page, then run \
                         `npm install && npm run build` in the project."
                            .to_owned(),
                    ),
                }
            }
        }
    }

    // 4. Serve it.
    r.step("Serving the site");
    serve(app, name, parent, path, secure)
}

fn blank_index(name: &str) -> String {
    format!(
        "<?php\n\n// {name}: served by RED. Edit public/index.php to start.\n\n\
         echo '<h1>{name}</h1><p>PHP ' . PHP_VERSION . '</p>';\n"
    )
}

/// The site's URL once served: from the configured ports, before it exists.
fn planned_url(app: &AppState, name: &str, secure: bool) -> String {
    let tld = &app.config.tld;
    let (scheme, port, default) = if secure {
        ("https", app.https_port.get().copied(), 443)
    } else {
        ("http", app.http_port.get().copied(), 80)
    };
    match port {
        Some(p) if p != default => format!("{scheme}://{name}.{tld}:{p}"),
        _ => format!("{scheme}://{name}.{tld}"),
    }
}

/// Run `command`, copying what it prints into the job's log.
async fn run(r: &Reporter, mut command: tokio::process::Command) -> Result<(), String> {
    let program = command
        .as_std()
        .get_program()
        .to_string_lossy()
        .into_owned();
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not run {program}: {e}"))?;
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let mut out = BufReader::new(stdout).lines();
    let mut err = BufReader::new(stderr).lines();
    let (mut out_done, mut err_done) = (false, false);
    while !(out_done && err_done) {
        tokio::select! {
            line = out.next_line(), if !out_done => match line {
                Ok(Some(line)) => r.line(line),
                _ => out_done = true,
            },
            line = err.next_line(), if !err_done => match line {
                Ok(Some(line)) => r.line(line),
                _ => err_done = true,
            },
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        let name = Path::new(&program)
            .file_name()
            .map_or(program.clone(), |n| n.to_string_lossy().into_owned());
        Err(format!("{name} failed ({status}); see the log above"))
    }
}

pub(crate) async fn ensure_running(app: &AppState, service: &str) -> Result<(), String> {
    let config = app
        .state_snapshot()
        .services
        .get(service)
        .cloned()
        .ok_or_else(|| format!("no service named {service}"))?;
    if app.services.runtime(service).status != ServiceStatus::Running {
        app.services.start(service, &config).await;
    }
    for _ in 0..60 {
        match app.services.runtime(service).status {
            ServiceStatus::Running => return Ok(()),
            ServiceStatus::Failed => return Err(format!("{service} failed to start")),
            _ => tokio::time::sleep(Duration::from_millis(500)).await,
        }
    }
    Err(format!("{service} did not start in time"))
}

async fn create_database(r: &Reporter, db: &Database) -> Result<(), String> {
    let port = db.port.to_string();
    let command = match db.kind {
        ServiceKind::Mysql => {
            let client = red_platform::binaries::client_tool(&db.binary, &["mysql", "mariadb"])
                .ok_or_else(|| {
                    crate::databases::missing_client(db.kind, "the mysql client", &db.binary)
                })?;
            let mut command = tokio::process::Command::new(client);
            command.args([
                "--protocol=TCP",
                "-h",
                "127.0.0.1",
                "-P",
                &port,
                "-u",
                "root",
                "-e",
                &format!("CREATE DATABASE IF NOT EXISTS `{}`", db.name),
            ]);
            command
        }
        ServiceKind::Postgres => {
            let client = red_platform::binaries::client_tool(&db.binary, &["psql"])
                .ok_or_else(|| crate::databases::missing_client(db.kind, "psql", &db.binary))?;
            let psql = |sql: String| {
                let mut command = tokio::process::Command::new(&client);
                command
                    .args([
                        "-h",
                        "127.0.0.1",
                        "-p",
                        &port,
                        "-U",
                        "root",
                        "-d",
                        "postgres",
                    ])
                    .args(["-v", "ON_ERROR_STOP=1", "-Atc"])
                    .arg(sql)
                    .env("PGCONNECT_TIMEOUT", "10");
                command
            };
            // CREATE DATABASE has no IF NOT EXISTS: look first.
            let exists = psql(format!(
                "SELECT 1 FROM pg_database WHERE datname = '{}'",
                db.name
            ))
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|e| format!("could not run psql: {e}"))?;
            if !exists.status.success() {
                return Err(format!(
                    "psql failed: {}",
                    String::from_utf8_lossy(&exists.stderr).trim()
                ));
            }
            if String::from_utf8_lossy(&exists.stdout).trim() == "1" {
                r.line(format!("Database {} already exists", db.name));
                return Ok(());
            }
            psql(format!("CREATE DATABASE \"{}\"", db.name))
        }
        ServiceKind::Redis => return Err(format!("{} is not a database", db.service)),
    };
    run(r, command).await
}

/// Settings a new Laravel app gets in `.env`.
fn laravel_settings(
    name: &str,
    url: &str,
    smtp: Option<u16>,
    database: Option<&Database>,
) -> Vec<(String, String)> {
    let mut settings = vec![
        ("APP_NAME".to_owned(), quote_env(name)),
        ("APP_URL".to_owned(), url.to_owned()),
    ];
    if let Some(port) = smtp {
        settings.extend([
            ("MAIL_MAILER".to_owned(), "smtp".to_owned()),
            ("MAIL_HOST".to_owned(), "127.0.0.1".to_owned()),
            ("MAIL_PORT".to_owned(), port.to_string()),
            // The SMTP user becomes the mailbox, so each app's mail stays apart.
            ("MAIL_USERNAME".to_owned(), name.to_owned()),
            ("MAIL_PASSWORD".to_owned(), "null".to_owned()),
        ]);
    }
    if let Some(db) = database {
        for (key, value) in db.kind.laravel_env(db.port) {
            settings.push((key.to_owned(), value));
        }
        settings.push(("DB_DATABASE".to_owned(), db.name.clone()));
    }
    settings
}

fn quote_env(value: &str) -> String {
    if value.chars().any(|c| c.is_whitespace() || c == '#') {
        format!("\"{value}\"")
    } else {
        value.to_owned()
    }
}

/// Set `settings` in a `.env` file: replace `KEY=…`, or uncomment
/// `# KEY=…` (Laravel ships the database settings commented out), or append.
pub fn apply_env(env: &str, settings: &[(String, String)]) -> String {
    let mut lines: Vec<String> = env.lines().map(str::to_owned).collect();
    for (key, value) in settings {
        let line = format!("{key}={value}");
        let set = |l: &str| l.split_once('=').is_some_and(|(k, _)| k.trim() == key);
        let commented = |l: &str| {
            l.trim_start()
                .strip_prefix('#')
                .is_some_and(|rest| set(rest.trim_start()))
        };
        if let Some(existing) = lines.iter_mut().find(|l| set(l)) {
            *existing = line;
        } else if let Some(existing) = lines.iter_mut().find(|l| commented(l)) {
            *existing = line;
        } else {
            lines.push(line);
        }
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// `wp-config.php` from WordPress's sample: RED's MySQL and fresh salts.
pub fn wp_config(sample: &str, database: &str, port: u16, salts: &[String]) -> String {
    let mut config = sample
        .replace("database_name_here", database)
        .replace("username_here", "root")
        .replace("password_here", "")
        .replace("'localhost'", &format!("'127.0.0.1:{port}'"));
    for salt in salts {
        match config.find("put your unique phrase here") {
            Some(at) => config.replace_range(at..at + "put your unique phrase here".len(), salt),
            None => break,
        }
    }
    config
}

/// Eight random strings for WordPress's keys and salts.
fn random_salts() -> Result<Vec<String>, String> {
    use std::io::Read;
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#%^&*()-_[]{}<>~+=,.;:/?|";
    let mut bytes = vec![0u8; 8 * 64];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .map_err(|e| format!("no randomness: {e}"))?;
    Ok(bytes
        .chunks(64)
        .map(|chunk| {
            chunk
                .iter()
                .map(|b| CHARS[*b as usize % CHARS.len()] as char)
                .collect()
        })
        .collect())
}

/// Download the latest WordPress and unpack it into `path`.
async fn download_wordpress(app: &AppState, path: &Path) -> Result<(), String> {
    let http = app.runtimes.http()?;
    let bytes = http
        .get(WORDPRESS_URL)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("downloading WordPress: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("downloading WordPress: {e}"))?;
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || unpack_wordpress(&bytes, &path))
        .await
        .map_err(|e| e.to_string())?
}

/// Unpack WordPress's tarball (everything under `wordpress/`) into `dest`.
fn unpack_wordpress(tarball: &[u8], dest: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(tarball));
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let Ok(rest) = path.strip_prefix("wordpress") else {
            continue;
        };
        if rest.as_os_str().is_empty()
            || rest
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            continue;
        }
        let target = dest.join(rest);
        if let Some(dir) = target.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        entry.unpack(target).map_err(|e| e.to_string())?;
    }
    if !dest.join("wp-config-sample.php").is_file() {
        return Err("the WordPress download did not contain WordPress".to_owned());
    }
    Ok(())
}

/// Serve the new directory: it already is a site when its parent is parked;
/// otherwise link it. Then secure it if asked.
fn serve(
    app: &AppState,
    name: &str,
    parent: &Path,
    path: &Path,
    secure: bool,
) -> Result<String, String> {
    let parked = app
        .state_snapshot()
        .parked
        .iter()
        .any(|p| same_dir(p, parent));
    if !parked {
        app.update_state(|s| s.link(name.to_owned(), path.to_path_buf()))
            .map_err(|e| e.to_string())?;
    }
    if secure {
        let expires = app.tls.issue(name).map_err(|e| e.to_string())?;
        app.update_state(|s| s.secured.insert(name.to_owned(), expires))
            .map_err(|e| e.to_string())?;
    }
    app.notify(Event::SitesChanged);
    Ok(app.site_url(&app.state_snapshot(), name))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_names() {
        assert!(valid_name("blog"));
        assert!(valid_name("my-shop-2"));
        assert!(!valid_name(""));
        assert!(!valid_name("Blog"));
        assert!(!valid_name("-x"));
        assert!(!valid_name("a_b"));
        assert!(!valid_name("a.b"));
        assert!(!valid_name(&"a".repeat(64)));
        assert_eq!(database_name("my-shop"), "my_shop");
    }

    #[test]
    fn picks_packages() {
        assert_eq!(package(ProjectKind::Laravel), Some("laravel/laravel"));
        assert_eq!(
            package(ProjectKind::LaravelReact),
            Some("laravel/react-starter-kit")
        );
        assert_eq!(package(ProjectKind::Wordpress), None);
        assert!(needs_frontend(ProjectKind::LaravelVue));
        assert!(!needs_frontend(ProjectKind::Laravel));
    }

    #[test]
    fn edits_env_files() {
        let env = "APP_NAME=Laravel\nAPP_URL=http://localhost\n\nDB_CONNECTION=sqlite\n# DB_HOST=127.0.0.1\n# DB_DATABASE=laravel\nMAIL_MAILER=log";
        let db = Database {
            service: "mysql".into(),
            kind: ServiceKind::Mysql,
            port: 3307,
            binary: "/x/mysqld".into(),
            name: "blog".into(),
        };
        let out = apply_env(
            env,
            &laravel_settings("blog", "https://blog.test", Some(2525), Some(&db)),
        );
        assert!(out.starts_with("APP_NAME=blog\nAPP_URL=https://blog.test\n"));
        assert!(out.contains("\nDB_CONNECTION=mysql\nDB_HOST=127.0.0.1\nDB_DATABASE=blog\n"));
        assert!(out.contains("\nMAIL_MAILER=smtp\n"));
        assert!(out.contains("\nDB_PORT=3307\n"));
        assert!(out.contains("\nMAIL_USERNAME=blog\n"));
        assert!(out.ends_with('\n'));
        assert!(!out.contains("# DB_"));
    }

    #[test]
    fn fills_wp_config() {
        let sample = "define( 'DB_NAME', 'database_name_here' );\n\
            define( 'DB_USER', 'username_here' );\n\
            define( 'DB_PASSWORD', 'password_here' );\n\
            define( 'DB_HOST', 'localhost' );\n\
            define( 'AUTH_KEY', 'put your unique phrase here' );\n\
            define( 'SECURE_AUTH_KEY', 'put your unique phrase here' );\n";
        let config = wp_config(sample, "blog", 3306, &["s1".into(), "s2".into()]);
        assert!(config.contains("'DB_NAME', 'blog'"));
        assert!(config.contains("'DB_USER', 'root'"));
        assert!(config.contains("'DB_PASSWORD', ''"));
        assert!(config.contains("'DB_HOST', '127.0.0.1:3306'"));
        assert!(config.contains("'AUTH_KEY', 's1'"));
        assert!(config.contains("'SECURE_AUTH_KEY', 's2'"));
        let salts = random_salts().unwrap();
        assert_eq!(salts.len(), 8);
        assert!(salts.iter().all(|s| s.len() == 64 && !s.contains('\'')));
    }

    #[test]
    fn unpacks_wordpress_safely() {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, body) in [
            ("wordpress/wp-config-sample.php", "<?php"),
            ("wordpress/wp-includes/version.php", "<?php"),
            ("other/file", "x"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, path, body.as_bytes()).unwrap();
        }
        let raw = tar.into_inner().unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        let tarball = gz.finish().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("blog");
        unpack_wordpress(&tarball, &dest).unwrap();
        assert!(dest.join("wp-includes/version.php").is_file());
        assert!(!dest.join("file").exists());
        assert!(!dir.path().join("other").exists());
    }

    #[test]
    fn keeps_the_end_of_progress_lines() {
        let mut log = VecDeque::new();
        push_line(&mut log, "  10%\r  50%\r 100%  ".into());
        push_line(&mut log, "   ".into());
        assert_eq!(log, [" 100%"]);
        for i in 0..LOG_LINES + 5 {
            push_line(&mut log, i.to_string());
        }
        assert_eq!(log.len(), LOG_LINES);
    }
}
