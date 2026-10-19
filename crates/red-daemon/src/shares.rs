//! Sharing sites on the internet through Cloudflare Tunnel.
//!
//! `cloudflared` connects out to Cloudflare (no open ports, no sudo) and
//! forwards requests for the public hostname to RED's HTTP port. RED then
//! routes that hostname to the shared site and tells PHP the request came
//! over HTTPS, so the app builds its URLs with the public address.
//!
//! - Without an account: a one-off `https://<words>.trycloudflare.com`.
//! - With `cloudflared tunnel login` done once: a stable hostname on your
//!   own domain (a named tunnel `red-<site>` plus a DNS route).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use red_api::{Event, RuntimeKind as ApiKind, ShareInfo, ShareState};
use red_core::runtimes::RuntimeKind;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;

use crate::AppState;

/// Shared sites by site name.
#[derive(Debug, Default)]
pub struct Shares {
    entries: Mutex<BTreeMap<String, Entry>>,
}

#[derive(Debug)]
struct Entry {
    info: ShareInfo,
    stop: CancellationToken,
}

impl Shares {
    pub fn list(&self) -> Vec<ShareInfo> {
        self.entries
            .lock()
            .expect("shares poisoned")
            .values()
            .map(|e| e.info.clone())
            .collect()
    }

    /// The shared site a request for `host` is for.
    pub fn site_for_host(&self, host: &str) -> Option<String> {
        let host = host
            .rsplit_once(':')
            .map_or(
                host,
                |(h, p)| if p.parse::<u16>().is_ok() { h } else { host },
            )
            .trim_end_matches('.')
            .to_ascii_lowercase();
        self.entries
            .lock()
            .expect("shares poisoned")
            .values()
            .find(|e| {
                e.info.state == ShareState::Running
                    && public_host(&e.info).is_some_and(|h| h == host)
            })
            .map(|e| e.info.site.clone())
    }

    fn update(&self, site: &str, change: impl FnOnce(&mut ShareInfo)) {
        if let Some(entry) = self.entries.lock().expect("shares poisoned").get_mut(site) {
            change(&mut entry.info);
        }
    }

    /// Stop sharing `site`; `false` when it was not shared.
    pub fn stop(&self, site: &str) -> bool {
        match self.entries.lock().expect("shares poisoned").remove(site) {
            Some(entry) => {
                entry.stop.cancel();
                true
            }
            None => false,
        }
    }

    /// Stop every share (redd is shutting down).
    pub fn shutdown(&self) {
        let entries = std::mem::take(&mut *self.entries.lock().expect("shares poisoned"));
        for entry in entries.into_values() {
            entry.stop.cancel();
        }
    }
}

/// The hostname requests arrive with.
fn public_host(info: &ShareInfo) -> Option<String> {
    info.hostname
        .clone()
        .or_else(|| {
            info.url
                .as_deref()
                .and_then(|u| u.split_once("://"))
                .map(|(_, rest)| rest.trim_end_matches('/').to_owned())
        })
        .map(|h| h.to_ascii_lowercase())
}

/// `true` for something that can be a DNS hostname on the user's domain.
pub fn valid_hostname(host: &str) -> bool {
    host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// The `trycloudflare.com` URL in a line of cloudflared's output.
pub fn quick_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '|')
        .unwrap_or(rest.len());
    let url = &rest[..end];
    url.ends_with(".trycloudflare.com")
        .then(|| url.to_owned())
        .filter(|u| u != "https://api.trycloudflare.com")
}

/// The login URL in a line of `cloudflared tunnel login`'s output.
pub fn login_url(line: &str) -> Option<String> {
    let start = line.find("https://dash.cloudflare.com/")?;
    let url = line[start..].split_whitespace().next()?;
    Some(url.to_owned())
}

/// cloudflared's account certificate (written by `cloudflared tunnel login`).
pub fn cert_path() -> Option<PathBuf> {
    std::env::var_os("TUNNEL_ORIGIN_CERT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cloudflared/cert.pem"))
        })
}

pub fn logged_in() -> bool {
    cert_path().is_some_and(|p| p.is_file())
}

/// cloudflared: RED's copy, else one on `PATH`.
pub fn cloudflared(app: &AppState) -> Option<PathBuf> {
    red_platform::binaries::red_runtime(&app.paths.data_dir, "cloudflared", "bin/cloudflared")
        .or_else(|| {
            red_platform::binaries::in_path("cloudflared")
                .into_iter()
                .next()
        })
}

/// Start sharing `site`; returns at once, the share reports its progress.
pub fn start(app: Arc<AppState>, site: String, hostname: Option<String>) -> ShareInfo {
    app.shares.stop(&site);
    let info = ShareInfo {
        site: site.clone(),
        url: hostname.as_ref().map(|h| format!("https://{h}")),
        hostname: hostname.clone(),
        state: ShareState::Starting,
        step: "Starting".to_owned(),
        error: None,
        started: red_core::time::unix_now(),
    };
    let stop = CancellationToken::new();
    app.shares.entries.lock().expect("shares poisoned").insert(
        site.clone(),
        Entry {
            info: info.clone(),
            stop: stop.clone(),
        },
    );
    app.notify(Event::SharesChanged);
    tokio::spawn(async move {
        if let Err(err) = run(&app, &site, hostname.as_deref(), &stop).await
            && !stop.is_cancelled()
        {
            tracing::warn!(%site, %err, "share failed");
            app.shares.update(&site, |i| {
                i.state = ShareState::Failed;
                i.error = Some(err);
            });
            app.notify(Event::SharesChanged);
        }
    });
    info
}

fn step(app: &AppState, site: &str, text: &str) {
    app.shares.update(site, |i| i.step = text.to_owned());
    app.notify(Event::SharesChanged);
}

async fn run(
    app: &Arc<AppState>,
    site: &str,
    hostname: Option<&str>,
    stop: &CancellationToken,
) -> Result<(), String> {
    let binary = match cloudflared(app) {
        Some(binary) => binary,
        None => {
            step(app, site, "Downloading cloudflared");
            install_cloudflared(app).await?;
            cloudflared(app).ok_or("cloudflared did not install")?
        }
    };
    let port = app
        .http_port
        .get()
        .copied()
        .ok_or("RED's web server is not running")?;
    let origin = format!("http://127.0.0.1:{port}");
    let mut command = tokio::process::Command::new(&binary);
    command.arg("tunnel").arg("--no-autoupdate");
    match hostname {
        None => {
            command.args(["--url", &origin]);
        }
        Some(host) => {
            if !logged_in() {
                return Err(
                    "cloudflared is not logged in to Cloudflare; log in first (`red share:login`)"
                        .into(),
                );
            }
            let tunnel = format!("red-{site}");
            step(app, site, "Setting up the tunnel and DNS");
            prepare_named(&binary, &tunnel, host).await?;
            command.args(["run", "--url", &origin, &tunnel]);
        }
    }
    step(app, site, "Connecting to Cloudflare");
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", binary.display()))?;
    let stderr = child.stderr.take().expect("piped");
    let mut lines = BufReader::new(stderr).lines();
    let mut recent: Vec<String> = Vec::new();
    loop {
        tokio::select! {
            () = stop.cancelled() => {
                let _ = child.kill().await;
                return Ok(());
            }
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    tracing::debug!(%site, cloudflared = %line);
                    if let Some(url) = quick_url(&line) {
                        app.shares.update(site, |i| i.url = Some(url));
                    }
                    if line.contains("Registered tunnel connection") {
                        app.shares.update(site, |i| {
                            if i.state != ShareState::Running {
                                i.state = ShareState::Running;
                                i.step = "Shared".to_owned();
                            }
                        });
                        app.notify(Event::SharesChanged);
                        tracing::info!(%site, "site shared");
                    }
                    recent.push(line);
                    if recent.len() > 8 {
                        recent.remove(0);
                    }
                }
                _ => break,
            },
        }
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    if stop.is_cancelled() {
        return Ok(());
    }
    Err(format!(
        "cloudflared stopped ({status}): {}",
        last_error(&recent).unwrap_or("see the redd log")
    ))
}

/// The most telling line cloudflared printed before it stopped.
fn last_error(lines: &[String]) -> Option<&str> {
    lines
        .iter()
        .rev()
        .find(|l| l.contains(" ERR ") || l.contains("error"))
        .or(lines.last())
        .map(String::as_str)
}

/// Create the named tunnel if needed and point `host` at it.
async fn prepare_named(binary: &Path, tunnel: &str, host: &str) -> Result<(), String> {
    let create = tokio::process::Command::new(binary)
        .args(["tunnel", "create", tunnel])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&create.stdout),
        String::from_utf8_lossy(&create.stderr)
    );
    if !create.status.success() && !text.contains("already exists") {
        return Err(format!("cloudflared tunnel create failed: {}", text.trim()));
    }
    let route = tokio::process::Command::new(binary)
        .args(["tunnel", "route", "dns", "--overwrite-dns", tunnel, host])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !route.status.success() {
        return Err(format!(
            "pointing {host} at the tunnel failed: {}",
            String::from_utf8_lossy(&route.stderr).trim()
        ));
    }
    Ok(())
}

/// Download cloudflared with RED's runtime installer and wait for it.
async fn install_cloudflared(app: &Arc<AppState>) -> Result<(), String> {
    let job = crate::runtimes::install(app.clone(), RuntimeKind::Cloudflared, "").await?;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let Some(current) = app.runtimes.jobs().into_iter().find(|j| j.id == job.id) else {
            return Err("the cloudflared download disappeared".into());
        };
        debug_assert_eq!(current.kind, ApiKind::Cloudflared);
        match current.state {
            red_api::JobState::Done => return Ok(()),
            red_api::JobState::Failed => {
                return Err(current
                    .error
                    .unwrap_or_else(|| "downloading cloudflared failed".into()));
            }
            _ => {}
        }
    }
}

/// Run `cloudflared tunnel login` in the background and return the URL to
/// open; cloudflared saves the certificate once the user authorized it.
pub async fn login(app: &AppState) -> Result<Option<String>, String> {
    if logged_in() {
        return Ok(None);
    }
    let binary = cloudflared(app).ok_or("cloudflared is not installed yet; share a site once, or install it with `red runtimes:install cloudflared`")?;
    let mut child = tokio::process::Command::new(&binary)
        .args(["tunnel", "login"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run {}: {e}", binary.display()))?;
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = Arc::new(Mutex::new(Some(tx)));
    for stream in [
        Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Send + Unpin>,
        Box::new(stderr),
    ] {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if let Some(url) = login_url(&line)
                    && let Some(tx) = tx.lock().expect("login poisoned").take()
                {
                    let _ = tx.send(url);
                }
            }
        });
    }
    // cloudflared waits (up to its own timeout) for the authorization.
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    match tokio::time::timeout(std::time::Duration::from_secs(20), rx).await {
        Ok(Ok(url)) => Ok(Some(url)),
        _ => Err("cloudflared did not print a login URL".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(url: Option<&str>, hostname: Option<&str>, state: ShareState) -> ShareInfo {
        ShareInfo {
            site: "blog".into(),
            url: url.map(str::to_owned),
            hostname: hostname.map(str::to_owned),
            state,
            step: String::new(),
            error: None,
            started: 0,
        }
    }

    #[test]
    fn reads_cloudflared_output() {
        assert_eq!(
            quick_url(
                "2026-10-19T10:00:00Z INF |  https://calm-river-blue-sky.trycloudflare.com                    |"
            ),
            Some("https://calm-river-blue-sky.trycloudflare.com".into())
        );
        assert_eq!(
            quick_url("INF Requesting new quick Tunnel on trycloudflare.com..."),
            None
        );
        assert_eq!(quick_url("POST https://api.trycloudflare.com/tunnel"), None);
        assert_eq!(
            login_url("https://dash.cloudflare.com/argotunnel?aud=&callback=https%3A%2F%2Fx"),
            Some("https://dash.cloudflare.com/argotunnel?aud=&callback=https%3A%2F%2Fx".into())
        );
        assert_eq!(login_url("Leave cloudflared running"), None);
    }

    #[test]
    fn routes_public_hosts_to_running_shares() {
        let shares = Shares::default();
        for (site, info) in [
            (
                "blog",
                info(
                    Some("https://calm-river.trycloudflare.com"),
                    None,
                    ShareState::Running,
                ),
            ),
            (
                "shop",
                info(
                    Some("https://shop.example.com"),
                    Some("shop.example.com"),
                    ShareState::Running,
                ),
            ),
            (
                "wip",
                info(
                    Some("https://wip.trycloudflare.com"),
                    None,
                    ShareState::Starting,
                ),
            ),
        ] {
            shares.entries.lock().unwrap().insert(
                site.into(),
                Entry {
                    info: ShareInfo {
                        site: site.into(),
                        ..info
                    },
                    stop: CancellationToken::new(),
                },
            );
        }
        assert_eq!(
            shares.site_for_host("calm-river.trycloudflare.com"),
            Some("blog".into())
        );
        assert_eq!(
            shares.site_for_host("Shop.Example.com:443"),
            Some("shop".into())
        );
        assert_eq!(shares.site_for_host("wip.trycloudflare.com"), None);
        assert_eq!(shares.site_for_host("blog.test"), None);
        assert!(shares.stop("blog"));
        assert_eq!(shares.site_for_host("calm-river.trycloudflare.com"), None);
        assert!(!shares.stop("blog"));
    }

    #[test]
    fn checks_hostnames() {
        assert!(valid_hostname("app.example.com"));
        assert!(valid_hostname("dev-1.my-site.io"));
        assert!(!valid_hostname("localhost"));
        assert!(!valid_hostname("App.example.com"));
        assert!(!valid_hostname("-a.example.com"));
        assert!(!valid_hostname("a..example.com"));
        assert!(!valid_hostname("a_b.example.com"));
    }

    #[test]
    fn picks_the_telling_error_line() {
        let lines = vec![
            "INF starting".to_owned(),
            "2026 ERR failed to dial".to_owned(),
            "INF bye".to_owned(),
        ];
        assert_eq!(last_error(&lines), Some("2026 ERR failed to dial"));
        assert_eq!(last_error(&["INF x".to_owned()]), Some("INF x"));
        assert_eq!(last_error(&[]), None);
    }
}
