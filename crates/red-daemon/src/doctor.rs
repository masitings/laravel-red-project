//! `GET /v1/doctor`: everything that has to be right for sites to work,
//! each with a one-line fix.

use std::net::ToSocketAddrs;

use red_api::{Check, CheckStatus, DoctorReport};

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
    if let Some(services) = services(app) {
        checks.push(services);
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
            Some("brew install php"),
        );
    }
    let versions: Vec<String> = installs.iter().map(|i| i.full_version.clone()).collect();
    check("php", "PHP", CheckStatus::Ok, versions.join(", "), None)
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
        return check(
            "resolver",
            "Resolver",
            CheckStatus::Warn,
            "not checked on this OS",
            None,
        );
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
        return check(
            "ca",
            "Local CA",
            CheckStatus::Warn,
            format!("trust is not checked on this OS; CA at {}", ca.display()),
            Some("red trust"),
        );
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
