//! Discovering PHP installations and supervising one PHP-FPM pool per version.
//!
//! Pools start on demand, the first time a site needs that version, and run
//! as the current user. Each pool gets a generated config under
//! `<data>/php/<version>/` and listens on `<run>/php<version>.sock`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::RwLock;
use std::time::Duration;

use red_platform::Paths;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

/// How long a new pool may take to create its socket.
const START_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a pool may take to exit after SIGTERM before it is killed.
const STOP_TIMEOUT: Duration = Duration::from_secs(3);

/// A PHP version found on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpInstall {
    /// Minor version, e.g. `8.3`.
    pub version: String,
    /// Full version, e.g. `8.3.6`.
    pub full_version: String,
    /// The `php-fpm` binary.
    pub fpm: PathBuf,
}

/// Errors from starting or reaching a PHP-FPM pool.
#[derive(Debug, thiserror::Error)]
pub enum FpmError {
    #[error("PHP {0} is not installed")]
    NotInstalled(String),
    #[error("could not start PHP-FPM {version}: {reason}")]
    Start { version: String, reason: String },
}

#[derive(Debug)]
struct Pool {
    child: Child,
    socket: PathBuf,
}

/// Owns every PHP-FPM process RED starts.
#[derive(Debug)]
pub struct PhpManager {
    paths: Paths,
    events: tokio::sync::broadcast::Sender<red_api::Event>,
    installs: RwLock<Vec<PhpInstall>>,
    pools: Mutex<HashMap<String, Pool>>,
}

impl PhpManager {
    pub fn new(paths: Paths, events: tokio::sync::broadcast::Sender<red_api::Event>) -> Self {
        Self {
            paths,
            events,
            installs: RwLock::new(Vec::new()),
            pools: Mutex::new(HashMap::new()),
        }
    }

    /// Look for `php-fpm` binaries again and remember what was found.
    /// The first binary found for a version wins (see
    /// [`red_platform::php::fpm_candidates`] for the search order).
    pub async fn discover(&self) -> Vec<PhpInstall> {
        let mut installs: Vec<PhpInstall> = Vec::new();
        for fpm in red_platform::php::fpm_candidates(&self.paths.data_dir) {
            let Some((version, full_version)) = probe(&fpm).await else {
                tracing::debug!(fpm = %fpm.display(), "not a usable php-fpm");
                continue;
            };
            if installs.iter().any(|i| i.version == version) {
                continue;
            }
            installs.push(PhpInstall {
                version,
                full_version,
                fpm,
            });
        }
        installs.sort_by(|a, b| red_core::php::compare(&b.version, &a.version));
        let changed = {
            let mut current = self.installs.write().expect("installs lock poisoned");
            let changed = *current != installs;
            *current = installs.clone();
            changed
        };
        if changed {
            self.notify();
        }
        installs
    }

    /// Installations found by the last [`discover`](Self::discover), newest first.
    pub fn installs(&self) -> Vec<PhpInstall> {
        self.installs
            .read()
            .expect("installs lock poisoned")
            .clone()
    }

    /// The installation for `version`, if any.
    pub fn find(&self, version: &str) -> Option<PhpInstall> {
        self.installs().into_iter().find(|i| i.version == version)
    }

    /// Newest installed minor version.
    pub fn newest(&self) -> Option<String> {
        self.installs().first().map(|i| i.version.clone())
    }

    /// Versions whose pool is currently running.
    pub async fn running(&self) -> Vec<String> {
        let mut pools = self.pools.lock().await;
        pools.retain(|_, pool| matches!(pool.child.try_wait(), Ok(None)));
        let mut versions: Vec<String> = pools.keys().cloned().collect();
        versions.sort();
        versions
    }

    /// The FastCGI socket for `version`, starting its pool if needed.
    pub async fn socket(&self, version: &str) -> Result<PathBuf, FpmError> {
        let mut pools = self.pools.lock().await;
        if let Some(pool) = pools.get_mut(version) {
            if matches!(pool.child.try_wait(), Ok(None)) && pool.socket.exists() {
                return Ok(pool.socket.clone());
            }
            tracing::warn!(version, "PHP-FPM pool exited; starting it again");
            pools.remove(version);
        }
        let install = self
            .find(version)
            .ok_or_else(|| FpmError::NotInstalled(version.to_owned()))?;
        let pool = self.spawn(&install).await?;
        let socket = pool.socket.clone();
        pools.insert(version.to_owned(), pool);
        self.notify();
        Ok(socket)
    }

    /// Stop the pool for `version`; the next request starts a fresh one.
    pub async fn restart(&self, version: &str) {
        let pool = self.pools.lock().await.remove(version);
        if let Some(pool) = pool {
            tracing::info!(version, "restarting PHP-FPM pool");
            stop(pool).await;
            self.notify();
        }
    }

    /// Stop every pool. Called when the daemon shuts down.
    pub async fn shutdown(&self) {
        let pools: Vec<Pool> = self.pools.lock().await.drain().map(|(_, p)| p).collect();
        for pool in pools {
            stop(pool).await;
        }
    }

    fn notify(&self) {
        let _ = self.events.send(red_api::Event::PhpChanged);
    }

    async fn spawn(&self, install: &PhpInstall) -> Result<Pool, FpmError> {
        let version = &install.version;
        let fail = |reason: String| FpmError::Start {
            version: version.clone(),
            reason,
        };
        let dir = self.paths.php_dir(version);
        let socket = self.paths.php_socket(version);
        let log = self.paths.log_dir.join(format!("php{version}-fpm.log"));
        for d in [&dir, &self.paths.log_dir, &self.paths.run_dir] {
            std::fs::create_dir_all(d).map_err(|e| fail(format!("create {}: {e}", d.display())))?;
        }
        let conf = dir.join("php-fpm.conf");
        std::fs::write(&conf, pool_config(&dir, &socket, &log))
            .map_err(|e| fail(format!("write {}: {e}", conf.display())))?;
        let _ = std::fs::remove_file(&socket);

        let mut command = Command::new(&install.fpm);
        command.arg("--nodaemonize").arg("--fpm-config").arg(&conf);
        if let Some(ini) = red_platform::binaries::php_ini_for(&install.fpm) {
            command.arg("-c").arg(ini);
        }
        if red_platform::user::is_root() {
            command.arg("--allow-to-run-as-root");
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| fail(format!("run {}: {e}", install.fpm.display())))?;

        let deadline = tokio::time::Instant::now() + START_TIMEOUT;
        loop {
            if socket.exists() {
                tracing::info!(version, pid = child.id(), "PHP-FPM pool started");
                return Ok(Pool { child, socket });
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(fail(format!(
                    "php-fpm exited with {status}; see {}",
                    log.display()
                )));
            }
            if tokio::time::Instant::now() > deadline {
                let _ = child.start_kill();
                return Err(fail(format!(
                    "no socket after {}s; see {}",
                    START_TIMEOUT.as_secs(),
                    log.display()
                )));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
}

/// Run `php-fpm -v` and parse its version.
async fn probe(fpm: &Path) -> Option<(String, String)> {
    let output = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(fpm).arg("-v").stdin(Stdio::null()).output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    red_core::php::parse_version_output(&String::from_utf8_lossy(&output.stdout))
}

async fn stop(mut pool: Pool) {
    if let Some(pid) = pool.child.id() {
        red_platform::process::terminate(pid);
    }
    if tokio::time::timeout(STOP_TIMEOUT, pool.child.wait())
        .await
        .is_err()
    {
        let _ = pool.child.kill().await;
    }
    let _ = std::fs::remove_file(&pool.socket);
}

fn pool_config(dir: &Path, socket: &Path, log: &Path) -> String {
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into());
    format!(
        "; Generated by RED. Changes are overwritten when the pool starts.\n\
         [global]\n\
         pid = {pid}\n\
         error_log = {log}\n\
         daemonize = no\n\
         \n\
         [red]\n\
         listen = {socket}\n\
         listen.mode = 0600\n\
         pm = ondemand\n\
         pm.max_children = 16\n\
         pm.process_idle_timeout = 60s\n\
         pm.max_requests = 500\n\
         catch_workers_output = yes\n\
         decorate_workers_output = no\n\
         clear_env = yes\n\
         env[PATH] = {path}\n\
         php_admin_flag[log_errors] = on\n\
         php_admin_value[error_log] = {log}\n",
        pid = dir.join("php-fpm.pid").display(),
        log = log.display(),
        socket = socket.display(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_config_points_at_socket_and_log() {
        let conf = pool_config(
            Path::new("/d"),
            Path::new("/run/php8.3.sock"),
            Path::new("/logs/php8.3-fpm.log"),
        );
        assert!(conf.contains("listen = /run/php8.3.sock"));
        assert!(conf.contains("listen.mode = 0600"));
        assert!(conf.contains("error_log = /logs/php8.3-fpm.log"));
        assert!(conf.contains("daemonize = no"));
        assert!(conf.contains("clear_env = yes"));
    }
}
