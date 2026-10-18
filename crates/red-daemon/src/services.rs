//! Running database and cache services (MySQL/MariaDB, PostgreSQL,
//! Redis/Valkey) as supervised child processes.
//!
//! Each instance gets `<data>/services/<name>/data`, listens on 127.0.0.1
//! only, and logs to `<logs>/services/<name>.log`. A service that crashes is
//! restarted with a growing delay; one that keeps crashing is marked failed.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, RwLock};
use std::time::{Duration, Instant};

use red_api::{Event, ServiceStatus};
use red_core::services::{ServerVersion, parse_server_version};
use red_core::{ServiceConfig, ServiceKind};
use red_platform::Paths;
use red_platform::binaries::{self, Server};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// How long a server may take to accept connections after starting.
const START_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a server may take to stop before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(20);
/// How long initialising a data directory may take.
const INIT_TIMEOUT: Duration = Duration::from_secs(180);
/// Crashes in a row before a service is marked failed.
const MAX_ATTEMPTS: u32 = 4;
/// A service that ran this long before crashing starts counting afresh.
const STABLE_AFTER: Duration = Duration::from_secs(30);

/// A server binary found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInstall {
    pub binary: PathBuf,
    pub server: ServerVersion,
}

/// What a running (or stopped) instance looks like right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runtime {
    pub status: ServiceStatus,
    pub pid: Option<u32>,
    pub error: Option<String>,
    pub flavor: Option<String>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            status: ServiceStatus::Stopped,
            pid: None,
            error: None,
            flavor: None,
        }
    }
}

#[derive(Debug)]
struct Handle {
    cancel: CancellationToken,
    task: JoinHandle<()>,
}

type Runtimes = Arc<StdMutex<HashMap<String, Runtime>>>;

/// Owns every service process RED starts.
#[derive(Debug)]
pub struct ServiceManager {
    paths: Paths,
    events: tokio::sync::broadcast::Sender<Event>,
    installs: RwLock<Vec<ServerInstall>>,
    handles: Mutex<HashMap<String, Handle>>,
    runtimes: Runtimes,
}

impl ServiceManager {
    pub fn new(paths: Paths, events: tokio::sync::broadcast::Sender<Event>) -> Self {
        Self {
            paths,
            events,
            installs: RwLock::new(Vec::new()),
            handles: Mutex::new(HashMap::new()),
            runtimes: Arc::default(),
        }
    }

    /// Look for server binaries again.
    pub async fn discover(&self) -> Vec<ServerInstall> {
        let mut found = Vec::new();
        for kind in ServiceKind::ALL {
            for binary in binaries::server_candidates(server(kind)) {
                if let Some(server) = probe(kind, &binary).await {
                    found.push(ServerInstall { binary, server });
                }
            }
        }
        *self.installs.write().expect("installs lock poisoned") = found.clone();
        found
    }

    /// Installs found by the last [`discover`](Self::discover).
    pub fn installs(&self) -> Vec<ServerInstall> {
        self.installs
            .read()
            .expect("installs lock poisoned")
            .clone()
    }

    /// Current runtime state of `name`.
    pub fn runtime(&self, name: &str) -> Runtime {
        self.runtimes
            .lock()
            .expect("runtimes lock poisoned")
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// Start `name` unless it is already running or starting.
    pub async fn start(&self, name: &str, config: &ServiceConfig) {
        let mut handles = self.handles.lock().await;
        if let Some(handle) = handles.get(name) {
            if !handle.task.is_finished() {
                return;
            }
            handles.remove(name);
        }
        let cancel = CancellationToken::new();
        let supervisor = Supervisor {
            name: name.to_owned(),
            config: config.clone(),
            paths: self.paths.clone(),
            runtimes: self.runtimes.clone(),
            events: self.events.clone(),
            cancel: cancel.clone(),
        };
        supervisor.set(Runtime {
            status: ServiceStatus::Starting,
            ..Runtime::default()
        });
        let task = tokio::spawn(supervisor.run());
        handles.insert(name.to_owned(), Handle { cancel, task });
    }

    /// Stop `name` and wait until it exited.
    pub async fn stop(&self, name: &str) {
        let handle = self.handles.lock().await.remove(name);
        if let Some(handle) = handle {
            handle.cancel.cancel();
            let _ = handle.task.await;
        }
        let mut runtimes = self.runtimes.lock().expect("runtimes lock poisoned");
        let runtime = runtimes.entry(name.to_owned()).or_default();
        runtime.status = ServiceStatus::Stopped;
        runtime.pid = None;
        drop(runtimes);
        let _ = self.events.send(Event::ServicesChanged);
    }

    /// Forget a removed service (after [`stop`](Self::stop)).
    pub fn forget(&self, name: &str) {
        self.runtimes
            .lock()
            .expect("runtimes lock poisoned")
            .remove(name);
    }

    /// Stop every service. Called when the daemon shuts down.
    pub async fn shutdown(&self) {
        let handles: Vec<Handle> = self.handles.lock().await.drain().map(|(_, h)| h).collect();
        for handle in &handles {
            handle.cancel.cancel();
        }
        for handle in handles {
            let _ = handle.task.await;
        }
    }
}

fn server(kind: ServiceKind) -> Server {
    match kind {
        ServiceKind::Mysql => Server::Mysql,
        ServiceKind::Postgres => Server::Postgres,
        ServiceKind::Redis => Server::Redis,
    }
}

/// Run `<binary> --version` and parse what it says.
pub async fn probe(kind: ServiceKind, binary: &Path) -> Option<ServerVersion> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(binary)
            .arg("--version")
            .stdin(Stdio::null())
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    parse_server_version(kind, &text)
}

/// One program invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// How to initialise and run one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Creates the data directory on first start.
    pub init: Option<Step>,
    pub run: Step,
    pub data_dir: PathBuf,
    /// PostgreSQL shuts down fast on SIGINT; everything else uses SIGTERM.
    pub stop_with_interrupt: bool,
}

/// Work out the commands for `name`.
pub fn plan(
    paths: &Paths,
    name: &str,
    config: &ServiceConfig,
    flavor: &str,
    as_root: bool,
) -> Result<Plan, String> {
    let dir = paths.service_dir(name);
    let data = dir.join("data");
    let port = config.port.to_string();
    let binary = config.binary.clone();
    let real = binary.canonicalize().unwrap_or_else(|_| binary.clone());
    let base_dir = real
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/usr"));
    let os = |s: &str| OsString::from(s);
    let flag = |name: &str, value: &Path| {
        let mut arg = OsString::from(format!("--{name}="));
        arg.push(value);
        arg
    };

    let plan = match config.kind {
        ServiceKind::Mysql => {
            let socket = paths.run_dir.join(format!("{name}.sock"));
            let mut run = vec![
                os("--no-defaults"),
                flag("datadir", &data),
                flag("basedir", &base_dir),
                os(&format!("--port={port}")),
                os("--bind-address=127.0.0.1"),
                flag("socket", &socket),
                flag("pid-file", &dir.join("server.pid")),
                os("--skip-name-resolve"),
            ];
            if flavor == "mysql" {
                run.push(os("--mysqlx=OFF"));
            }
            if as_root {
                run.push(os("--user=root"));
            }
            let init = if flavor == "mariadb" {
                let program =
                    binaries::companion(&real, &["mariadb-install-db", "mysql_install_db"])
                        .ok_or("mariadb-install-db was not found next to the MariaDB server")?;
                let mut args = vec![
                    os("--no-defaults"),
                    flag("datadir", &data),
                    flag("basedir", &base_dir),
                    os("--auth-root-authentication-method=normal"),
                    os("--skip-test-db"),
                ];
                if as_root {
                    args.push(os("--user=root"));
                }
                Step { program, args }
            } else {
                let mut args = vec![
                    os("--no-defaults"),
                    os("--initialize-insecure"),
                    flag("datadir", &data),
                    flag("basedir", &base_dir),
                ];
                if as_root {
                    args.push(os("--user=root"));
                }
                Step {
                    program: binary.clone(),
                    args,
                }
            };
            Plan {
                init: Some(init),
                run: Step {
                    program: binary,
                    args: run,
                },
                data_dir: data,
                stop_with_interrupt: false,
            }
        }
        ServiceKind::Postgres => {
            if as_root {
                return Err("PostgreSQL refuses to run as root; run redd as your own user".into());
            }
            let initdb = binaries::companion(&real, &["initdb"])
                .ok_or("initdb was not found next to the PostgreSQL server")?;
            Plan {
                init: Some(Step {
                    program: initdb,
                    args: vec![
                        os("-D"),
                        data.clone().into(),
                        os("-U"),
                        os("root"),
                        os("--auth=trust"),
                        os("--encoding=UTF8"),
                        os("--locale=C"),
                    ],
                }),
                run: Step {
                    program: binary,
                    args: vec![
                        os("-D"),
                        data.clone().into(),
                        os("-p"),
                        os(&port),
                        os("-c"),
                        os("listen_addresses=127.0.0.1"),
                        // TCP only: the macOS data directory has a space in
                        // it and socket paths are limited to ~104 bytes.
                        os("-c"),
                        os("unix_socket_directories="),
                    ],
                },
                data_dir: data,
                stop_with_interrupt: true,
            }
        }
        ServiceKind::Redis => Plan {
            init: None,
            run: Step {
                program: binary,
                args: vec![
                    os("--port"),
                    os(&port),
                    os("--bind"),
                    os("127.0.0.1"),
                    os("--dir"),
                    data.clone().into(),
                    os("--daemonize"),
                    os("no"),
                    os("--logfile"),
                    os(""),
                ],
            },
            data_dir: data,
            stop_with_interrupt: false,
        },
    };
    Ok(plan)
}

/// `true` when something already listens on 127.0.0.1:`port`.
pub fn port_in_use(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok()
        || std::net::TcpListener::bind(addr).is_err()
}

async fn accepting(port: u16) -> bool {
    tokio::time::timeout(
        Duration::from_millis(500),
        tokio::net::TcpStream::connect(("127.0.0.1", port)),
    )
    .await
    .is_ok_and(|r| r.is_ok())
}

/// The last `lines` lines of a log file, for error messages.
pub fn log_tail(path: &Path, lines: usize) -> String {
    let text = std::fs::read(path).unwrap_or_default();
    let start = text.len().saturating_sub(16 * 1024);
    let text = String::from_utf8_lossy(&text[start..]);
    let all: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

/// Supervises one instance until cancelled or it gives up.
struct Supervisor {
    name: String,
    config: ServiceConfig,
    paths: Paths,
    runtimes: Runtimes,
    events: tokio::sync::broadcast::Sender<Event>,
    cancel: CancellationToken,
}

enum Outcome {
    Cancelled,
    Crashed(String),
}

impl Supervisor {
    fn set(&self, runtime: Runtime) {
        let changed = {
            let mut runtimes = self.runtimes.lock().expect("runtimes lock poisoned");
            let entry = runtimes.entry(self.name.clone()).or_default();
            let flavor = runtime.flavor.clone().or_else(|| entry.flavor.clone());
            let next = Runtime { flavor, ..runtime };
            let changed = *entry != next;
            *entry = next;
            changed
        };
        if changed {
            let _ = self.events.send(Event::ServicesChanged);
        }
    }

    fn fail(&self, flavor: Option<String>, error: String) {
        tracing::warn!(service = %self.name, %error, "service failed");
        self.set(Runtime {
            status: ServiceStatus::Failed,
            pid: None,
            error: Some(error),
            flavor,
        });
    }

    fn log(&self) -> PathBuf {
        self.paths.service_log(&self.name)
    }

    async fn run(self) {
        let Some(version) = probe(self.config.kind, &self.config.binary).await else {
            return self.fail(
                None,
                format!(
                    "{} is not a usable {} server",
                    self.config.binary.display(),
                    self.config.kind
                ),
            );
        };
        let flavor = Some(version.flavor.clone());
        let plan = match plan(
            &self.paths,
            &self.name,
            &self.config,
            &version.flavor,
            red_platform::user::is_root(),
        ) {
            Ok(plan) => plan,
            Err(error) => return self.fail(flavor, error),
        };
        for dir in [
            self.paths.service_dir(&self.name),
            self.log()
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
            self.paths.run_dir.clone(),
        ] {
            if let Err(err) = std::fs::create_dir_all(&dir) {
                return self.fail(flavor, format!("cannot create {}: {err}", dir.display()));
            }
        }
        if let Err(error) = self.initialise(&plan).await {
            return self.fail(flavor, error);
        }
        if plan.init.is_none()
            && let Err(err) = std::fs::create_dir_all(&plan.data_dir)
        {
            return self.fail(
                flavor,
                format!("cannot create {}: {err}", plan.data_dir.display()),
            );
        }

        let mut attempts = 0;
        loop {
            if port_in_use(self.config.port) {
                return self.fail(
                    flavor,
                    format!(
                        "port {} is already in use; stop the other server or pick another port",
                        self.config.port
                    ),
                );
            }
            let started = Instant::now();
            match self.run_once(&plan, &flavor).await {
                Outcome::Cancelled => {
                    self.set(Runtime {
                        flavor: flavor.clone(),
                        ..Runtime::default()
                    });
                    return;
                }
                Outcome::Crashed(reason) => {
                    if started.elapsed() > STABLE_AFTER {
                        attempts = 0;
                    }
                    attempts += 1;
                    let tail = log_tail(&self.log(), 5);
                    let error = if tail.is_empty() {
                        reason
                    } else {
                        format!("{reason}\n{tail}")
                    };
                    if attempts >= MAX_ATTEMPTS {
                        return self.fail(flavor, error);
                    }
                    tracing::warn!(service = %self.name, attempts, %error, "service crashed; restarting");
                    self.set(Runtime {
                        status: ServiceStatus::Starting,
                        pid: None,
                        error: Some(error),
                        flavor: flavor.clone(),
                    });
                    let delay = Duration::from_secs(1 << attempts.min(4));
                    tokio::select! {
                        () = self.cancel.cancelled() => {
                            self.set(Runtime { flavor: flavor.clone(), ..Runtime::default() });
                            return;
                        }
                        () = tokio::time::sleep(delay) => {}
                    }
                }
            }
        }
    }

    /// Create the data directory on first start.
    async fn initialise(&self, plan: &Plan) -> Result<(), String> {
        let Some(init) = &plan.init else {
            return Ok(());
        };
        let initialised = std::fs::read_dir(&plan.data_dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if initialised {
            return Ok(());
        }
        tracing::info!(service = %self.name, "initialising data directory");
        let (stdout, stderr) = self.log_files()?;
        let mut child = Command::new(&init.program)
            .args(&init.args)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("cannot run {}: {e}", init.program.display()))?;
        let status = tokio::select! {
            () = self.cancel.cancelled() => {
                let _ = child.kill().await;
                let _ = std::fs::remove_dir_all(&plan.data_dir);
                return Err("cancelled while initialising".into());
            }
            status = tokio::time::timeout(INIT_TIMEOUT, child.wait()) => status,
        };
        match status {
            Ok(Ok(status)) if status.success() => Ok(()),
            other => {
                let _ = child.kill().await;
                let _ = std::fs::remove_dir_all(&plan.data_dir);
                let reason = match other {
                    Ok(Ok(status)) => format!("exited with {status}"),
                    Ok(Err(err)) => err.to_string(),
                    Err(_) => "timed out".into(),
                };
                Err(format!(
                    "initialising the data directory failed ({reason}):\n{}",
                    log_tail(&self.log(), 8)
                ))
            }
        }
    }

    fn log_files(&self) -> Result<(Stdio, Stdio), String> {
        let open = || {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log())
                .map_err(|e| format!("cannot open {}: {e}", self.log().display()))
        };
        let file = open()?;
        let clone = file.try_clone().map_err(|e| e.to_string())?;
        Ok((Stdio::from(file), Stdio::from(clone)))
    }

    async fn run_once(&self, plan: &Plan, flavor: &Option<String>) -> Outcome {
        let (stdout, stderr) = match self.log_files() {
            Ok(files) => files,
            Err(error) => return Outcome::Crashed(error),
        };
        let mut child = match Command::new(&plan.run.program)
            .args(&plan.run.args)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(err) => {
                return Outcome::Crashed(format!(
                    "cannot run {}: {err}",
                    plan.run.program.display()
                ));
            }
        };
        let pid = child.id();
        self.set(Runtime {
            status: ServiceStatus::Starting,
            pid,
            error: None,
            flavor: flavor.clone(),
        });

        // Wait until it accepts connections.
        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            tokio::select! {
                () = self.cancel.cancelled() => {
                    stop(&mut child, plan.stop_with_interrupt).await;
                    return Outcome::Cancelled;
                }
                status = child.wait() => {
                    return Outcome::Crashed(match status {
                        Ok(status) => format!("exited with {status} while starting"),
                        Err(err) => err.to_string(),
                    });
                }
                () = tokio::time::sleep(Duration::from_millis(100)) => {}
            }
            if accepting(self.config.port).await {
                break;
            }
            if tokio::time::Instant::now() > deadline {
                stop(&mut child, plan.stop_with_interrupt).await;
                return Outcome::Crashed(format!(
                    "did not accept connections on port {} within {}s",
                    self.config.port,
                    START_TIMEOUT.as_secs()
                ));
            }
        }
        tracing::info!(service = %self.name, ?pid, port = self.config.port, "service running");
        self.set(Runtime {
            status: ServiceStatus::Running,
            pid,
            error: None,
            flavor: flavor.clone(),
        });

        tokio::select! {
            () = self.cancel.cancelled() => {
                stop(&mut child, plan.stop_with_interrupt).await;
                Outcome::Cancelled
            }
            status = child.wait() => Outcome::Crashed(match status {
                Ok(status) => format!("exited with {status}"),
                Err(err) => err.to_string(),
            }),
        }
    }
}

async fn stop(child: &mut Child, interrupt: bool) {
    if let Some(pid) = child.id() {
        if interrupt {
            red_platform::process::interrupt(pid);
        } else {
            red_platform::process::terminate(pid);
        }
    }
    if tokio::time::timeout(STOP_TIMEOUT, child.wait())
        .await
        .is_err()
    {
        let _ = child.kill().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(kind: ServiceKind, binary: &str, port: u16) -> ServiceConfig {
        ServiceConfig {
            kind,
            binary: binary.into(),
            version: None,
            port,
            autostart: true,
        }
    }

    #[test]
    fn redis_plan_binds_loopback() {
        let paths = Paths::under("/red");
        let plan = plan(
            &paths,
            "redis",
            &config(ServiceKind::Redis, "/usr/bin/redis-server", 6380),
            "redis",
            false,
        )
        .unwrap();
        assert!(plan.init.is_none());
        let args: Vec<_> = plan.run.args.iter().map(|a| a.to_string_lossy()).collect();
        assert!(args.windows(2).any(|w| w[0] == "--port" && w[1] == "6380"));
        assert!(
            args.windows(2)
                .any(|w| w[0] == "--bind" && w[1] == "127.0.0.1")
        );
        assert_eq!(
            plan.data_dir,
            PathBuf::from("/red/data/services/redis/data")
        );
    }

    #[test]
    fn mysql_plan_depends_on_flavor() {
        let paths = Paths::under("/red");
        let cfg = config(ServiceKind::Mysql, "/nonexistent/bin/mysqld", 3307);
        let mysql = plan(&paths, "mysql", &cfg, "mysql", false).unwrap();
        let args: Vec<_> = mysql.run.args.iter().map(|a| a.to_string_lossy()).collect();
        assert_eq!(args[0], "--no-defaults");
        assert!(args.contains(&"--mysqlx=OFF".into()));
        assert!(args.contains(&"--port=3307".into()));
        let init = mysql.init.unwrap();
        assert!(init.args.iter().any(|a| a == "--initialize-insecure"));

        // MariaDB initialises with its install script instead (found next
        // to the server or on PATH).
        match plan(&paths, "mysql", &cfg, "mariadb", false) {
            Ok(plan) => {
                let init = plan.init.unwrap();
                let name = init
                    .program
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                assert!(name.contains("install"), "{name}");
                assert!(!plan.run.args.iter().any(|a| a == "--mysqlx=OFF"));
            }
            Err(err) => assert!(err.contains("mariadb-install-db"), "{err}"),
        }
    }

    #[test]
    fn postgres_refuses_root() {
        let paths = Paths::under("/red");
        let cfg = config(
            ServiceKind::Postgres,
            "/usr/lib/postgresql/16/bin/postgres",
            5432,
        );
        assert!(plan(&paths, "postgres", &cfg, "postgresql", true).is_err());
    }

    #[test]
    fn detects_used_ports() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_in_use(port));
        drop(listener);
    }

    #[test]
    fn tails_logs() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("x.log");
        std::fs::write(&log, "a\nb\n\nc\nd\n").unwrap();
        assert_eq!(log_tail(&log, 2), "c\nd");
        assert_eq!(log_tail(&dir.path().join("missing"), 2), "");
    }
}
