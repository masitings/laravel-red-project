//! `GET /v1/doctor`: everything that has to be right for sites to work,
//! each with a one-line fix.

use std::net::ToSocketAddrs;

use red_api::{Check, CheckStatus, DoctorReport, FixAction};

use crate::AppState;

fn check(
    id: &str,
    label: &str,
    status: CheckStatus,
    detail: impl Into<String>,
    fix: Option<&str>,
) -> Check {
    Check {
        id: id.to_owned(),
        label: label.to_owned(),
        status,
        detail: detail.into(),
        fix: fix.map(str::to_owned),
        action: None,
    }
}

/// The fix RED can apply itself for a failing check, if any.
pub fn action_for(check: &Check) -> Option<FixAction> {
    if check.status == CheckStatus::Ok {
        return None;
    }
    match check.id.as_str() {
        "php" => Some(FixAction::InstallPhp),
        // Installed but redd started before it: restarting through the
        // login agent moves redd onto ports 80 and 443.
        "helper" if check.detail.starts_with("installed") => Some(FixAction::StartAtLogin),
        "helper" => Some(FixAction::InstallHelper),
        "autostart" => Some(FixAction::StartAtLogin),
        "resolver" => Some(FixAction::InstallDns),
        "ca" => Some(FixAction::TrustCa),
        "services" => Some(FixAction::RestartServices),
        _ => None,
    }
}

/// Run every check.
pub async fn run(app: &AppState) -> DoctorReport {
    let mut checks = vec![php(app), web_ports(app), dns_responder(app)];
    checks.push(resolver_file(app));
    checks.push(resolution(app).await);
    checks.push(ca_trust(app).await);
    checks.push(helper(app).await);
    checks.push(autostart());
    checks.push(xdebug(app));
    if let Some(services) = services(app) {
        checks.push(services);
    }
    for check in &mut checks {
        check.action = action_for(check);
    }
    DoctorReport { checks }
}

/// Services that failed to start. Omitted when no service is configured.
fn services(app: &AppState) -> Option<Check> {
    let state = app.state_snapshot();
    if state.services.is_empty() {
        return None;
    }
    let failed: Vec<String> = state
        .services
        .keys()
        .filter(|name| app.services.runtime(name).status == red_api::ServiceStatus::Failed)
        .cloned()
        .collect();
    Some(if failed.is_empty() {
        check(
            "services",
            "Services",
            CheckStatus::Ok,
            format!("{} configured, none failed", state.services.len()),
            None,
        )
    } else {
        check(
            "services",
            "Services",
            CheckStatus::Warn,
            format!("failed: {}", failed.join(", ")),
            Some("see `red services`, then `red services:restart <name>`"),
        )
    })
}

fn php(app: &AppState) -> Check {
    let installs = app.php.installs();
    if installs.is_empty() {
        return check(
            "php",
            "PHP",
            CheckStatus::Fail,
            "no php-fpm found",
            Some("red php:install (or brew install php)"),
        );
    }
    let versions: Vec<String> = installs.iter().map(|i| i.full_version.clone()).collect();
    // Laravel's default database is SQLite; a PHP without pdo_sqlite breaks
    // a fresh app in a confusing way.
    let without_sqlite: Vec<String> = installs
        .iter()
        .filter(|i| {
            std::process::Command::new(&i.fpm)
                .arg("-m")
                .output()
                .is_ok_and(|o| {
                    o.status.success()
                        && !String::from_utf8_lossy(&o.stdout)
                            .lines()
                            .any(|l| l.trim() == "pdo_sqlite")
                })
        })
        .map(|i| i.version.clone())
        .collect();
    if without_sqlite.is_empty() {
        check("php", "PHP", CheckStatus::Ok, versions.join(", "), None)
    } else {
        check(
            "php",
            "PHP",
            CheckStatus::Warn,
            format!(
                "{}; without pdo_sqlite (Laravel's default database): {}",
                versions.join(", "),
                without_sqlite.join(", ")
            ),
            Some(
                "red runtimes:update php (or red php:install <version>) for RED's own builds, which have it",
            ),
        )
    }
}

fn web_ports(app: &AppState) -> Check {
    let notes = app.web_notes.lock().expect("web notes poisoned").join("; ");
    let with_notes = |text: String| {
        if notes.is_empty() {
            text
        } else {
            format!("{text} ({notes})")
        }
    };
    match (app.http_port.get(), app.https_port.get()) {
        (Some(80), Some(443)) => check(
            "ports",
            "Web ports",
            CheckStatus::Ok,
            "serving on 80 and 443",
            None,
        ),
        (Some(http), Some(https)) => check(
            "ports",
            "Web ports",
            CheckStatus::Warn,
            with_notes(format!("serving on {http} and {https}; URLs need the port")),
            Some(if notes.contains("another program") {
                "stop the other web server, then `red restart`"
            } else {
                "red helper:install"
            }),
        ),
        _ => check(
            "ports",
            "Web ports",
            CheckStatus::Fail,
            with_notes("the web server is not running".into()),
            Some("see ~/Library/Logs/RED/redd.log, then `red restart`"),
        ),
    }
}

fn dns_responder(app: &AppState) -> Check {
    match app.dns_port.get() {
        Some(port) => check(
            "dns",
            "DNS responder",
            CheckStatus::Ok,
            format!(".{} on 127.0.0.1:{port}", app.config.tld),
            None,
        ),
        None => check(
            "dns",
            "DNS responder",
            CheckStatus::Fail,
            "not running; another program may use its port",
            Some("set dns_port in config.toml"),
        ),
    }
}

fn resolver_file(app: &AppState) -> Check {
    let tld = &app.config.tld;
    if !cfg!(target_os = "macos") {
        let path = red_helper::resolver::path_for(&red_helper::resolver::default_dir(), tld);
        let (status, detail, fix) = linux_resolver(&LinuxResolver {
            path: &path.display().to_string(),
            drop_in: std::fs::read_to_string(&path).ok().as_deref(),
            dns_port: app.dns_port.get().copied(),
            resolved_running: red_platform::trust::systemd_resolved_running(),
            resolv_conf: std::fs::read_to_string("/etc/resolv.conf").ok().as_deref(),
            systemd_version: systemd_version(),
            tld,
        });
        return check("resolver", "Resolver", status, detail, fix);
    }
    let path = format!("/etc/resolver/{tld}");
    match (std::fs::read_to_string(&path), app.dns_port.get()) {
        (Ok(text), Some(port)) if text.contains(&format!("port {port}")) => check(
            "resolver",
            "Resolver",
            CheckStatus::Ok,
            format!("{path} points at RED"),
            None,
        ),
        (Ok(text), _) => {
            let ours = text.contains("Added by RED Project");
            let points_at = text
                .lines()
                .filter(|l| l.starts_with("nameserver") || l.starts_with("port"))
                .collect::<Vec<_>>()
                .join(", ");
            let detail = if ours {
                format!("{path} points at an old DNS port ({points_at})")
            } else {
                format!(
                    "{path} belongs to another tool ({points_at}); only one tool can own .{tld}"
                )
            };
            check(
                "resolver",
                "Resolver",
                CheckStatus::Fail,
                detail,
                Some(if ours {
                    "red dns:install"
                } else {
                    "quit the other tool (e.g. Herd or Valet), then `red dns:install`"
                }),
            )
        }
        (Err(_), _) => check(
            "resolver",
            "Resolver",
            CheckStatus::Fail,
            format!("{path} is missing"),
            Some("red dns:install"),
        ),
    }
}

fn xdebug(app: &AppState) -> Check {
    let mode = app.state_snapshot().xdebug;
    let with: Vec<String> = app
        .php
        .installs()
        .into_iter()
        .filter(|i| i.xdebug.is_some() || i.xdebug_loaded)
        .map(|i| {
            if i.xdebug_loaded {
                format!("{} (always on through its php.ini)", i.version)
            } else {
                i.version
            }
        })
        .collect();
    let (status, detail, fix) = xdebug_check(mode, &with, app.config.xdebug_port);
    check("xdebug", "Xdebug", status, detail, fix)
}

fn xdebug_check(
    mode: red_core::xdebug::XdebugMode,
    with: &[String],
    port: u16,
) -> (CheckStatus, String, Option<&'static str>) {
    use red_core::xdebug::XdebugMode;
    if mode == XdebugMode::Off {
        return (
            CheckStatus::Ok,
            "off; `red xdebug:auto` debugs requests that carry XDEBUG_SESSION".into(),
            None,
        );
    }
    if with.is_empty() {
        return (
            CheckStatus::Warn,
            "no installed PHP has Xdebug".into(),
            Some(if cfg!(target_os = "macos") {
                "red runtimes:update php (RED's macOS builds include Xdebug)"
            } else {
                "install your distribution's php-xdebug package"
            }),
        );
    }
    let how = if mode == XdebugMode::On {
        "on for every request"
    } else {
        "on demand (XDEBUG_SESSION cookie or XDEBUG_TRIGGER)"
    };
    (
        CheckStatus::Ok,
        format!(
            "{how} for PHP {}; IDE listens on port {port}",
            with.join(", ")
        ),
        None,
    )
}

/// What the Linux resolver check looks at.
struct LinuxResolver<'a> {
    path: &'a str,
    drop_in: Option<&'a str>,
    dns_port: Option<u16>,
    resolved_running: bool,
    resolv_conf: Option<&'a str>,
    systemd_version: Option<u32>,
    tld: &'a str,
}

/// systemd-resolved accepts `DNS=127.0.0.1:<port>` from this version on.
const RESOLVED_PORT_SYNTAX: u32 = 246;

fn linux_resolver(r: &LinuxResolver<'_>) -> (CheckStatus, String, Option<&'static str>) {
    if !r.resolved_running {
        let port = r.dns_port.map_or("<port>".to_owned(), |p| p.to_string());
        return (
            CheckStatus::Warn,
            format!(
                "systemd-resolved is not running; send .{} to 127.0.0.1#{port} in your DNS setup (dnsmasq: server=/{}/127.0.0.1#{port})",
                r.tld, r.tld
            ),
            None,
        );
    }
    if let Some(version) = r.systemd_version
        && version < RESOLVED_PORT_SYNTAX
    {
        return (
            CheckStatus::Fail,
            format!(
                "systemd {version} only sends DNS to port 53 (other ports need {RESOLVED_PORT_SYNTAX}+); use dnsmasq instead: server=/{}/127.0.0.1#{}",
                r.tld,
                r.dns_port.map_or("<port>".to_owned(), |p| p.to_string())
            ),
            None,
        );
    }
    let stub = r.resolv_conf.is_none_or(|text| {
        text.lines()
            .any(|l| l.trim_start().starts_with("nameserver 127.0.0.53"))
    });
    match (r.drop_in, r.dns_port) {
        (Some(text), Some(port)) if text.contains(&format!("DNS=127.0.0.1:{port}")) => {
            if stub {
                (CheckStatus::Ok, format!("{} points at RED", r.path), None)
            } else {
                (
                    CheckStatus::Warn,
                    format!(
                        "{} points at RED, but /etc/resolv.conf does not use systemd-resolved (127.0.0.53), so some programs bypass it",
                        r.path
                    ),
                    Some("sudo ln -sf /run/systemd/resolve/stub-resolv.conf /etc/resolv.conf"),
                )
            }
        }
        (Some(text), _) if text.starts_with(red_helper::resolver::MARKER) => (
            CheckStatus::Fail,
            format!("{} points at an old DNS port", r.path),
            Some("red dns:install"),
        ),
        (Some(_), _) => (
            CheckStatus::Fail,
            format!("{} belongs to another tool", r.path),
            Some("remove it, then `red dns:install`"),
        ),
        (None, _) => (
            CheckStatus::Fail,
            format!("{} is missing", r.path),
            Some("red dns:install"),
        ),
    }
}

/// Major version of systemd, from `systemctl --version` ("systemd 255 (...)").
fn systemd_version() -> Option<u32> {
    let output = std::process::Command::new("systemctl")
        .arg("--version")
        .output()
        .ok()?;
    parse_systemd_version(&String::from_utf8_lossy(&output.stdout))
}

fn parse_systemd_version(text: &str) -> Option<u32> {
    text.lines()
        .next()?
        .strip_prefix("systemd ")?
        .split(|c: char| !c.is_ascii_digit())
        .next()?
        .parse()
        .ok()
}

async fn resolution(app: &AppState) -> Check {
    let host = format!("red-doctor.{}:80", app.config.tld);
    let lookup = host.clone();
    let result = tokio::task::spawn_blocking(move || {
        lookup
            .to_socket_addrs()
            .map(|addrs| addrs.map(|a| a.ip()).collect::<Vec<_>>())
    })
    .await;
    match result {
        Ok(Ok(ips)) if !ips.is_empty() && ips.iter().all(|ip| ip.is_loopback()) => check(
            "resolution",
            "Name resolution",
            CheckStatus::Ok,
            format!("*.{} resolves to this machine", app.config.tld),
            None,
        ),
        Ok(Ok(ips)) if !ips.is_empty() => check(
            "resolution",
            "Name resolution",
            CheckStatus::Fail,
            format!("*.{} resolves elsewhere: {ips:?}", app.config.tld),
            Some("red dns:install"),
        ),
        _ => check(
            "resolution",
            "Name resolution",
            CheckStatus::Fail,
            format!("*.{} does not resolve", app.config.tld),
            Some("red dns:install"),
        ),
    }
}

async fn ca_trust(app: &AppState) -> Check {
    let ca = app.tls.ca_cert_path();
    if !cfg!(target_os = "macos") {
        let pem = std::fs::read_to_string(&ca).unwrap_or_default();
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let (status, detail, fix) = tokio::task::spawn_blocking(move || {
            let system = system_trusts(&pem);
            let browsers = home.map(|home| browser_trust(&home)).unwrap_or_default();
            linux_ca(system, &browsers)
        })
        .await
        .unwrap_or((CheckStatus::Warn, "could not check".into(), None));
        return check("ca", "Local CA", status, detail, fix);
    }
    let status = tokio::process::Command::new("security")
        .args(["verify-cert", "-c"])
        .arg(&ca)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    match status {
        Ok(status) if status.success() => {
            check("ca", "Local CA", CheckStatus::Ok, "trusted by macOS", None)
        }
        _ => check(
            "ca",
            "Local CA",
            CheckStatus::Fail,
            "not trusted; browsers will warn on HTTPS sites",
            Some("red trust"),
        ),
    }
}

/// Whether the system trust store (what curl, PHP and Rust programs read)
/// contains the CA in `pem`; `None` when the store could not be read.
fn system_trusts(pem: &str) -> Option<bool> {
    use base64::Engine;
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body.trim())
        .ok()?;
    let loaded = rustls_native_certs::load_native_certs();
    if loaded.certs.is_empty() {
        return None;
    }
    Some(
        loaded
            .certs
            .iter()
            .any(|cert| cert.as_ref() == der.as_slice()),
    )
}

/// Trust in each browser NSS database that exists: `Some(trusted)`, or
/// `None` when `certutil` is not installed to look.
fn browser_trust(home: &std::path::Path) -> Vec<(String, Option<bool>)> {
    let certutil = !red_platform::binaries::in_path("certutil").is_empty();
    red_platform::trust::nss_databases(home)
        .into_iter()
        .filter(|db| db.join("cert9.db").is_file())
        .map(|db| {
            let trusted = certutil.then(|| {
                std::process::Command::new("certutil")
                    .arg("-d")
                    .arg(format!("sql:{}", db.display()))
                    .args(["-L", "-n", red_platform::trust::NSS_NICKNAME])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok_and(|s| s.success())
            });
            (db.display().to_string(), trusted)
        })
        .collect()
}

fn linux_ca(
    system: Option<bool>,
    browsers: &[(String, Option<bool>)],
) -> (CheckStatus, String, Option<&'static str>) {
    match system {
        Some(false) => {
            return (
                CheckStatus::Fail,
                "not trusted by the system; HTTPS sites fail in curl, PHP and browsers".into(),
                Some("red trust"),
            );
        }
        None => {
            return (
                CheckStatus::Warn,
                "could not read the system trust store".into(),
                Some("red trust"),
            );
        }
        Some(true) => {}
    }
    if browsers.iter().any(|(_, trusted)| trusted.is_none()) {
        return (
            CheckStatus::Warn,
            "trusted by the system; browsers not checked (certutil is not installed)".into(),
            Some("install libnss3-tools (or nss-tools), then `red trust`"),
        );
    }
    let missing: Vec<&str> = browsers
        .iter()
        .filter(|(_, trusted)| *trusted == Some(false))
        .map(|(db, _)| db.as_str())
        .collect();
    if missing.is_empty() {
        (
            CheckStatus::Ok,
            "trusted by the system and browsers".into(),
            None,
        )
    } else {
        (
            CheckStatus::Warn,
            format!(
                "trusted by the system, not by browsers using {}",
                missing.join(", ")
            ),
            Some("red trust"),
        )
    }
}

async fn helper(app: &AppState) -> Check {
    if app.via_helper.load(std::sync::atomic::Ordering::Relaxed) {
        return check(
            "helper",
            "Privileged helper",
            CheckStatus::Ok,
            "provides ports 80 and 443",
            None,
        );
    }
    match red_helper::client::call(&app.helper_socket, &red_helper::protocol::Request::Status).await
    {
        Ok(_) => check(
            "helper",
            "Privileged helper",
            CheckStatus::Warn,
            "installed, but redd started before it",
            Some("red service:install (restarts redd)"),
        ),
        Err(_) => check(
            "helper",
            "Privileged helper",
            CheckStatus::Warn,
            "not installed",
            Some("red helper:install"),
        ),
    }
}

fn autostart() -> Check {
    let Some(home) = std::env::var_os("HOME") else {
        return check(
            "autostart",
            "Start at login",
            CheckStatus::Warn,
            "HOME is not set",
            None,
        );
    };
    let definition = red_platform::services::agent_definition(std::path::Path::new(&home));
    if definition.exists() {
        check(
            "autostart",
            "Start at login",
            CheckStatus::Ok,
            "redd starts at login",
            None,
        )
    } else {
        check(
            "autostart",
            "Start at login",
            CheckStatus::Warn,
            "redd must be started by hand",
            Some("red service:install"),
        )
    }
}

#[cfg(test)]
mod action_tests {
    use super::*;

    #[test]
    fn picks_fixes() {
        let make = |id: &str, status, detail: &str| check(id, id, status, detail, None);
        assert_eq!(action_for(&make("php", CheckStatus::Ok, "8.4")), None);
        assert_eq!(
            action_for(&make("php", CheckStatus::Fail, "no php-fpm found")),
            Some(FixAction::InstallPhp)
        );
        assert_eq!(
            action_for(&make("helper", CheckStatus::Warn, "not installed")),
            Some(FixAction::InstallHelper)
        );
        assert_eq!(
            action_for(&make(
                "helper",
                CheckStatus::Warn,
                "installed, but redd started before it"
            )),
            Some(FixAction::StartAtLogin)
        );
        assert_eq!(
            action_for(&make("ca", CheckStatus::Fail, "x")),
            Some(FixAction::TrustCa)
        );
        assert_eq!(action_for(&make("dns", CheckStatus::Fail, "x")), None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver<'a>(drop_in: Option<&'a str>, resolv_conf: Option<&'a str>) -> LinuxResolver<'a> {
        LinuxResolver {
            path: "/etc/systemd/resolved.conf.d/red-test.conf",
            drop_in,
            dns_port: Some(53530),
            resolved_running: true,
            resolv_conf,
            systemd_version: Some(255),
            tld: "test",
        }
    }

    /// The systemd-resolved drop-in RED writes on Linux (built here rather
    /// than with `resolver::contents`, which follows the OS running the test).
    fn drop_in(port: u16) -> String {
        format!(
            "{} Remove with `red dns:uninstall`.\n[Resolve]\nDNS=127.0.0.1:{port}\nDomains=~test\n",
            red_helper::resolver::MARKER
        )
    }

    #[test]
    fn checks_the_linux_resolver() {
        let ours = drop_in(53530);
        let stub = "nameserver 127.0.0.53\noptions edns0\n";
        assert_eq!(
            linux_resolver(&resolver(Some(&ours), Some(stub))).0,
            CheckStatus::Ok
        );
        // resolv.conf bypasses systemd-resolved.
        let (status, _, fix) = linux_resolver(&resolver(Some(&ours), Some("nameserver 1.1.1.1\n")));
        assert_eq!(status, CheckStatus::Warn);
        assert!(fix.unwrap().contains("stub-resolv.conf"));
        // Old port.
        let old = drop_in(5353);
        assert_eq!(
            linux_resolver(&resolver(Some(&old), Some(stub))).0,
            CheckStatus::Fail
        );
        // Missing.
        let (status, detail, _) = linux_resolver(&resolver(None, Some(stub)));
        assert_eq!(status, CheckStatus::Fail);
        assert!(detail.contains("missing"));
        // Too old for DNS=address:port.
        let mut old_systemd = resolver(Some(&ours), Some(stub));
        old_systemd.systemd_version = Some(245);
        assert_eq!(linux_resolver(&old_systemd).0, CheckStatus::Fail);
        // No systemd-resolved: only advice.
        let mut none = resolver(None, None);
        none.resolved_running = false;
        let (status, detail, _) = linux_resolver(&none);
        assert_eq!(status, CheckStatus::Warn);
        assert!(detail.contains("server=/test/127.0.0.1#53530"));
    }

    #[test]
    fn reports_xdebug() {
        use red_core::xdebug::XdebugMode;
        assert_eq!(xdebug_check(XdebugMode::Off, &[], 9003).0, CheckStatus::Ok);
        assert_eq!(
            xdebug_check(XdebugMode::Auto, &[], 9003).0,
            CheckStatus::Warn
        );
        let (status, detail, _) = xdebug_check(XdebugMode::Auto, &["8.4".into()], 9003);
        assert_eq!(status, CheckStatus::Ok);
        assert!(detail.contains("on demand") && detail.contains("8.4") && detail.contains("9003"));
        assert!(
            xdebug_check(XdebugMode::On, &["8.4".into()], 9003)
                .1
                .contains("every request")
        );
    }

    #[test]
    fn parses_systemd_versions() {
        assert_eq!(
            parse_systemd_version("systemd 255 (255.4-1ubuntu8)\n+PAM"),
            Some(255)
        );
        assert_eq!(parse_systemd_version("systemd 249\n"), Some(249));
        assert_eq!(parse_systemd_version("nope"), None);
    }

    #[test]
    fn checks_linux_ca_trust() {
        assert_eq!(linux_ca(Some(false), &[]).0, CheckStatus::Fail);
        assert_eq!(linux_ca(None, &[]).0, CheckStatus::Warn);
        assert_eq!(linux_ca(Some(true), &[]).0, CheckStatus::Ok);
        let chrome = "/home/me/.pki/nssdb".to_owned();
        assert_eq!(
            linux_ca(Some(true), &[(chrome.clone(), Some(true))]).0,
            CheckStatus::Ok
        );
        let (status, detail, _) = linux_ca(Some(true), &[(chrome.clone(), Some(false))]);
        assert_eq!(status, CheckStatus::Warn);
        assert!(detail.contains(".pki/nssdb"));
        let (_, _, fix) = linux_ca(Some(true), &[(chrome, None)]);
        assert!(fix.unwrap().contains("libnss3-tools"));
    }

    #[test]
    fn finds_a_ca_in_the_system_store() {
        // A PEM that is surely not in the store, and garbage.
        let pem = "-----BEGIN CERTIFICATE-----\nAAAA\n-----END CERTIFICATE-----\n";
        assert_ne!(system_trusts(pem), Some(true));
        assert_eq!(system_trusts("not base64 !!"), None);
    }
}
