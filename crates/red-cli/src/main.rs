use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use red_api::client::{Client, ClientError, Method};
use red_api::{
    CheckStatus, DoctorReport, PathRequest, PhpListResponse, PhpVersionRequest, ProxyInfo,
    ProxyRequest, SiteInfo, SiteKind, StatusResponse, endpoints,
};
use red_platform::Paths;
use serde::Serialize;

mod install;
mod system;

/// red: local PHP development environment.
#[derive(Debug, Parser)]
#[command(version, propagate_version = true)]
struct Cli {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show whether redd is running and where RED keeps its files.
    Status,
    /// Serve every sub-directory of PATH (default: current directory) as a site.
    Park { path: Option<PathBuf> },
    /// Stop serving the sub-directories of PATH (default: current directory).
    Forget { path: Option<PathBuf> },
    /// List parked directories.
    Paths,
    /// Serve a directory under NAME (default: the directory's name).
    Link {
        name: Option<String>,
        /// Directory to link (default: current directory).
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Remove a link (default: the site in the current directory).
    Unlink { name: Option<String> },
    /// List all sites.
    #[command(visible_alias = "links")]
    Sites,
    /// Show the site and driver serving the current directory.
    Which,
    /// List installed PHP versions.
    #[command(name = "php:list")]
    PhpList,
    /// Set the PHP version used by sites that are not isolated.
    Use {
        #[arg(value_name = "VERSION")]
        php: String,
    },
    /// Pin a site to a PHP version (default site: current directory).
    Isolate {
        #[arg(value_name = "VERSION")]
        php: String,
        #[arg(long)]
        site: Option<String>,
    },
    /// Remove a site's PHP pin (default site: current directory).
    Unisolate {
        #[arg(long)]
        site: Option<String>,
    },
    /// Forward NAME.<tld> to a local dev server (port, host:port or http:// URL).
    Proxy { name: String, target: String },
    /// Remove a proxy.
    Unproxy { name: String },
    /// List proxies.
    Proxies,
    /// Print daemon events as they happen (for debugging).
    Events,
    /// Serve a site or proxy over HTTPS (default: the site in the current directory).
    Secure { name: Option<String> },
    /// Serve a site over plain HTTP again.
    Unsecure { name: Option<String> },
    /// List sites served over HTTPS and when their certificates expire.
    Secured,
    /// Trust RED's local certificate authority (asks for your password).
    Trust,
    /// Remove RED's local certificate authority from the trust store.
    Untrust,
    /// Install the privileged helper that opens ports 80/443 (uses sudo).
    #[command(name = "helper:install")]
    HelperInstall,
    /// Remove the privileged helper (uses sudo).
    #[command(name = "helper:uninstall")]
    HelperUninstall,
    /// Start redd at login, and (re)start it now.
    #[command(name = "service:install")]
    ServiceInstall,
    /// Stop starting redd at login, and stop it.
    #[command(name = "service:uninstall")]
    ServiceUninstall,
    /// Check that everything RED needs is in place.
    Doctor,
    /// One-time setup: helper, autostart, DNS and CA trust.
    Setup,
    /// Point the OS at RED's DNS responder for the local TLD (uses sudo).
    #[command(name = "dns:install")]
    DnsInstall,
    /// Remove the resolver file installed by `red dns:install` (uses sudo).
    #[command(name = "dns:uninstall")]
    DnsUninstall,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report(&cli, &err);
            ExitCode::FAILURE
        }
    }
}

struct Ctx<'a> {
    cli: &'a Cli,
    client: Client,
}

impl Ctx<'_> {
    async fn send<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, ClientError> {
        self.client.call(method, path, body).await
    }

    /// Print `value` as JSON when `--json` is set; otherwise run `text`.
    fn output<T: Serialize>(&self, value: &T, text: impl FnOnce(&T)) -> anyhow::Result<()> {
        if self.cli.json {
            println!("{}", serde_json::to_string_pretty(value)?);
        } else {
            text(value);
        }
        Ok(())
    }

    /// The site whose directory is (or contains) the current directory.
    async fn current_site(&self) -> anyhow::Result<SiteInfo> {
        let cwd = cwd()?;
        let sites: Vec<SiteInfo> = self.client.get(endpoints::SITES).await?;
        site_containing(&sites, &cwd).cloned().with_context(|| {
            format!(
                "{} is not inside a site; pass a site name or run `red link` here",
                cwd.display()
            )
        })
    }

    /// Install or remove the resolver file: through the helper when it is
    /// installed, otherwise with sudo.
    async fn dns(&self, install: bool) -> anyhow::Result<()> {
        let method = if install {
            Method::POST
        } else {
            Method::DELETE
        };
        match self
            .send::<(), serde_json::Value>(method, endpoints::DNS, None)
            .await
        {
            Ok(_) => {
                println!(
                    "{} through red-helper.",
                    if install {
                        "DNS installed"
                    } else {
                        "DNS removed"
                    }
                );
                Ok(())
            }
            Err(ClientError::Api { status: 503, .. }) => {
                let status = self.client.status().await?;
                if install {
                    let port = status
                        .dns_port
                        .context("redd's DNS responder is not running; check the redd log")?;
                    system::install_resolver(&status.tld, port)
                } else {
                    system::uninstall_resolver(&status.tld)
                }
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Wait until a (re)started redd answers.
    async fn wait_for_daemon(&self) -> anyhow::Result<()> {
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if self.client.status().await.is_ok() {
                return Ok(());
            }
        }
        bail!("redd did not come up; check its log with `red doctor` or the log directory")
    }

    /// Secure or unsecure a site, or a proxy when no site has that name.
    /// Returns the name, the new URL and the JSON to print with `--json`.
    async fn set_secure(
        &self,
        name: &Option<String>,
        on: bool,
    ) -> anyhow::Result<(String, String, serde_json::Value)> {
        let name = self.site_name(name).await?;
        let method = if on { Method::PUT } else { Method::DELETE };
        match self
            .send::<(), SiteInfo>(method.clone(), &endpoints::site_secure(&name), None)
            .await
        {
            Ok(site) => Ok((
                site.name.clone(),
                site.url.clone(),
                serde_json::to_value(site)?,
            )),
            Err(ClientError::Api { status: 404, .. }) => {
                let proxy: ProxyInfo = self
                    .send(method, &endpoints::proxy_secure(&name), None::<&()>)
                    .await?;
                Ok((
                    proxy.name.clone(),
                    proxy.url.clone(),
                    serde_json::to_value(proxy)?,
                ))
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn site_name(&self, name: &Option<String>) -> anyhow::Result<String> {
        match name {
            Some(name) => Ok(name.clone()),
            None => Ok(self.current_site().await?.name),
        }
    }
}

async fn run(cli: &Cli) -> anyhow::Result<()> {
    let paths = Paths::discover()?;
    let ctx = Ctx {
        cli,
        client: Client::new(paths.socket()),
    };

    match &cli.command {
        Command::Status => {
            let status = ctx.client.status().await?;
            ctx.output(&status, print_status)?;
        }
        Command::Park { path } => {
            let dir = absolute_dir(path.as_deref())?;
            let parked: Vec<String> = ctx
                .send(
                    Method::POST,
                    endpoints::PATHS,
                    Some(&path_request(&dir, None)),
                )
                .await?;
            ctx.output(&parked, |_| {
                println!(
                    "Parked {}. Its sub-directories are now sites.",
                    dir.display()
                );
            })?;
        }
        Command::Forget { path } => {
            let dir = absolute_dir(path.as_deref())?;
            let parked: Vec<String> = ctx
                .send(
                    Method::DELETE,
                    endpoints::PATHS,
                    Some(&path_request(&dir, None)),
                )
                .await?;
            ctx.output(&parked, |_| println!("Forgot {}.", dir.display()))?;
        }
        Command::Paths => {
            let parked: Vec<String> = ctx.client.get(endpoints::PATHS).await?;
            ctx.output(&parked, |parked| {
                if parked.is_empty() {
                    println!("No parked directories. Run `red park` in a directory of projects.");
                }
                for path in parked {
                    println!("{path}");
                }
            })?;
        }
        Command::Link { name, path } => {
            let dir = absolute_dir(path.as_deref())?;
            let site: SiteInfo = ctx
                .send(
                    Method::POST,
                    endpoints::LINKS,
                    Some(&path_request(&dir, name.clone())),
                )
                .await?;
            ctx.output(&site, |site| {
                println!("Linked {} → {}", site.url, site.path);
            })?;
        }
        Command::Unlink { name } => {
            let name = ctx.site_name(name).await?;
            let _: serde_json::Value = ctx
                .send(Method::DELETE, &endpoints::link(&name), None::<&()>)
                .await?;
            ctx.output(&serde_json::json!({ "unlinked": name }), |_| {
                println!("Unlinked {name}.");
            })?;
        }
        Command::Sites => {
            let sites: Vec<SiteInfo> = ctx.client.get(endpoints::SITES).await?;
            ctx.output(&sites, |s| print_sites(s))?;
        }
        Command::Which => {
            let site = ctx.current_site().await?;
            ctx.output(&site, |site| {
                println!(
                    "{} is served by the {} driver at {}",
                    site.path, site.driver, site.url
                );
            })?;
        }
        Command::PhpList => {
            let list: PhpListResponse = ctx.client.get(endpoints::PHP).await?;
            ctx.output(&list, print_php)?;
        }
        Command::Use { php } => {
            let list: PhpListResponse = ctx
                .send(
                    Method::PUT,
                    endpoints::PHP_DEFAULT,
                    Some(&PhpVersionRequest {
                        version: php.clone(),
                    }),
                )
                .await?;
            ctx.output(&list, |list| {
                println!(
                    "Sites that are not isolated now use PHP {}.",
                    list.default.as_deref().unwrap_or("?")
                );
            })?;
        }
        Command::Isolate { php, site } => {
            let name = ctx.site_name(site).await?;
            let site: SiteInfo = ctx
                .send(
                    Method::PUT,
                    &endpoints::site_php(&name),
                    Some(&PhpVersionRequest {
                        version: php.clone(),
                    }),
                )
                .await?;
            ctx.output(&site, |site| {
                println!(
                    "{} now uses PHP {}.",
                    site.name,
                    site.php.as_deref().unwrap_or("?")
                );
            })?;
        }
        Command::Secure { name } => {
            let (name, url, value) = ctx.set_secure(name, true).await?;
            ctx.output(&value, |_| println!("{name} is now served at {url}"))?;
        }
        Command::Unsecure { name } => {
            let (name, url, value) = ctx.set_secure(name, false).await?;
            ctx.output(&value, |_| println!("{name} is served at {url} again"))?;
        }
        Command::Proxy { name, target } => {
            let proxy: ProxyInfo = ctx
                .send(
                    Method::POST,
                    endpoints::PROXIES,
                    Some(&ProxyRequest {
                        name: name.clone(),
                        target: target.clone(),
                    }),
                )
                .await?;
            ctx.output(&proxy, |p| println!("{} → {}", p.url, p.target))?;
        }
        Command::Unproxy { name } => {
            let _: serde_json::Value = ctx
                .send(Method::DELETE, &endpoints::proxy(name), None::<&()>)
                .await?;
            ctx.output(&serde_json::json!({ "removed": name }), |_| {
                println!("Removed proxy {name}.");
            })?;
        }
        Command::Proxies => {
            let proxies: Vec<ProxyInfo> = ctx.client.get(endpoints::PROXIES).await?;
            ctx.output(&proxies, |proxies| {
                if proxies.is_empty() {
                    println!("No proxies. Add one with `red proxy <name> <port>`.");
                    return;
                }
                let rows: Vec<[String; 2]> = proxies
                    .iter()
                    .map(|p| [p.url.clone(), p.target.clone()])
                    .collect();
                print_table(&["URL", "TARGET"], &rows);
            })?;
        }
        Command::Events => {
            let json = cli.json;
            ctx.client
                .events(|event| {
                    if json {
                        println!("{}", serde_json::to_string(&event).unwrap_or_default());
                    } else {
                        println!("{event:?}");
                    }
                    true
                })
                .await?;
        }
        Command::Secured => {
            let sites: Vec<SiteInfo> = ctx.client.get(endpoints::SITES).await?;
            let secured: Vec<SiteInfo> = sites.into_iter().filter(|s| s.secure).collect();
            ctx.output(&secured, |sites| {
                if sites.is_empty() {
                    println!("No secured sites. Run `red secure` in a site's directory.");
                    return;
                }
                let rows: Vec<[String; 2]> = sites
                    .iter()
                    .map(|s| {
                        let expires = s
                            .cert_expires
                            .map(red_core_date)
                            .unwrap_or_else(|| "-".into());
                        [s.url.clone(), expires]
                    })
                    .collect();
                print_table(&["URL", "VALID UNTIL"], &rows);
            })?;
        }
        Command::Trust => {
            let status = ctx.client.status().await?;
            system::trust(Path::new(&status.paths.ca_cert))?;
        }
        Command::Untrust => {
            let status = ctx.client.status().await?;
            system::untrust(Path::new(&status.paths.ca_cert))?;
        }
        Command::DnsInstall => ctx.dns(true).await?,
        Command::DnsUninstall => ctx.dns(false).await?,
        Command::HelperInstall => {
            install::install_helper(red_platform::user::current_uid())?;
        }
        Command::HelperUninstall => install::uninstall_helper()?,
        Command::ServiceInstall => {
            install::install_agent(&paths.log_dir)?;
            ctx.wait_for_daemon().await?;
        }
        Command::ServiceUninstall => install::uninstall_agent()?,
        Command::Doctor => {
            let report: DoctorReport = ctx.client.get(endpoints::DOCTOR).await?;
            ctx.output(&report, print_doctor)?;
            if report.checks.iter().any(|c| c.status == CheckStatus::Fail) {
                std::process::exit(1);
            }
        }
        Command::Setup => {
            println!("Setting up RED. You may be asked for your password a few times.\n");
            println!("1/4  Privileged helper (ports 80 and 443)");
            install::install_helper(red_platform::user::current_uid())?;
            println!("\n2/4  Start redd at login");
            install::install_agent(&paths.log_dir)?;
            ctx.wait_for_daemon().await?;
            println!("\n3/4  DNS for .test");
            ctx.dns(true).await?;
            println!("\n4/4  Trust the local certificate authority");
            let status = ctx.client.status().await?;
            system::trust(Path::new(&status.paths.ca_cert))?;
            println!();
            let report: DoctorReport = ctx.client.get(endpoints::DOCTOR).await?;
            print_doctor(&report);
        }
        Command::Unisolate { site } => {
            let name = ctx.site_name(site).await?;
            let site: SiteInfo = ctx
                .send(Method::DELETE, &endpoints::site_php(&name), None::<&()>)
                .await?;
            ctx.output(&site, |site| {
                println!(
                    "{} follows the default PHP again ({}).",
                    site.name,
                    site.php.as_deref().unwrap_or("none installed")
                );
            })?;
        }
    }
    Ok(())
}

fn path_request(dir: &Path, name: Option<String>) -> PathRequest {
    PathRequest {
        path: dir.display().to_string(),
        name,
    }
}

fn cwd() -> anyhow::Result<PathBuf> {
    std::env::current_dir().context("cannot read the current directory")
}

/// `path` (or the current directory) as an absolute, canonical directory.
fn absolute_dir(path: Option<&Path>) -> anyhow::Result<PathBuf> {
    let path = match path {
        Some(path) => path.to_owned(),
        None => cwd()?,
    };
    let dir = path
        .canonicalize()
        .with_context(|| format!("{} does not exist", path.display()))?;
    if !dir.is_dir() {
        bail!("{} is not a directory", dir.display());
    }
    Ok(dir)
}

/// The site whose path is `dir` or an ancestor of it (deepest match wins).
fn site_containing<'a>(sites: &'a [SiteInfo], dir: &Path) -> Option<&'a SiteInfo> {
    sites
        .iter()
        .filter(|site| {
            let path = Path::new(&site.path);
            let path = path.canonicalize().unwrap_or_else(|_| path.to_owned());
            dir.starts_with(path)
        })
        .max_by_key(|site| site.path.len())
}

fn print_status(status: &StatusResponse) {
    println!(
        "redd     running (pid {}, up {})",
        status.pid,
        uptime(status.uptime_secs)
    );
    println!(
        "version  {} (API v{}, {})",
        status.version, status.api_version, status.os
    );
    match status.http_port {
        Some(port) => println!("http     http://*.{}:{port}", status.tld),
        None => println!("http     not running (see the redd log)"),
    }
    match status.https_port {
        Some(port) => println!("https    https://*.{}:{port}", status.tld),
        None => println!("https    not running (see the redd log)"),
    }
    match status.dns_port {
        Some(port) => println!("dns      .{} on 127.0.0.1:{port}", status.tld),
        None => println!("dns      not running (see the redd log)"),
    }
    println!("config   {}", status.paths.config_dir);
    println!("data     {}", status.paths.data_dir);
    println!("socket   {}", status.paths.socket);
    println!("ca       {}", status.paths.ca_cert);
}

fn print_doctor(report: &DoctorReport) {
    for check in &report.checks {
        let mark = match check.status {
            CheckStatus::Ok => " ok ",
            CheckStatus::Warn => "warn",
            CheckStatus::Fail => "FAIL",
        };
        println!("[{mark}] {:<18} {}", check.label, check.detail);
        if check.status != CheckStatus::Ok
            && let Some(fix) = &check.fix
        {
            println!("       {:<18} fix: {fix}", "");
        }
    }
}

fn red_core_date(unix: u64) -> String {
    red_core::time::date_from_unix(unix)
}

fn print_sites(sites: &[SiteInfo]) {
    if sites.is_empty() {
        println!(
            "No sites yet. Run `red park` in a directory of projects, or `red link` in one project."
        );
        return;
    }
    let rows: Vec<[String; 5]> = sites
        .iter()
        .map(|s| {
            let php = match (&s.php, s.isolated) {
                (Some(v), true) => format!("{v} (isolated)"),
                (Some(v), false) => v.clone(),
                (None, _) => "-".into(),
            };
            let kind = match s.kind {
                SiteKind::Parked => "parked",
                SiteKind::Linked => "linked",
            };
            [
                s.url.clone(),
                s.driver.clone(),
                php,
                kind.into(),
                s.path.clone(),
            ]
        })
        .collect();
    print_table(&["URL", "DRIVER", "PHP", "KIND", "PATH"], &rows);
}

fn print_php(list: &PhpListResponse) {
    if list.installs.is_empty() {
        println!("No PHP found. Install one, for example `brew install php`, then run this again.");
        return;
    }
    let rows: Vec<[String; 4]> = list
        .installs
        .iter()
        .map(|i| {
            let mut marks = Vec::new();
            if list.default.as_deref() == Some(i.version.as_str()) {
                marks.push("default");
            }
            if i.running {
                marks.push("running");
            }
            [
                i.version.clone(),
                i.full_version.clone(),
                marks.join(", "),
                i.fpm_path.clone(),
            ]
        })
        .collect();
    print_table(&["VERSION", "FULL", "", "PHP-FPM"], &rows);
}

fn print_table<const N: usize>(header: &[&str; N], rows: &[[String; N]]) {
    let mut widths = header.map(str::len);
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: Vec<&str>| {
        let parts: Vec<String> = cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:<width$}"))
            .collect();
        println!("{}", parts.join("  ").trim_end());
    };
    line(header.to_vec());
    for row in rows {
        line(row.iter().map(String::as_str).collect());
    }
}

fn uptime(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, secs % 3600 / 60),
    }
}

fn report(cli: &Cli, err: &anyhow::Error) {
    let hint = match err.downcast_ref::<ClientError>() {
        Some(ClientError::DaemonNotRunning { .. }) => Some("start it with `redd`".to_owned()),
        Some(ClientError::Api { error, .. }) => error.hint.clone(),
        _ => None,
    };
    if cli.json {
        let body = serde_json::json!({ "error": err.to_string(), "hint": hint });
        eprintln!("{body}");
    } else {
        eprintln!("error: {err}");
        if let Some(hint) = hint {
            eprintln!("hint:  {hint}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_is_human_readable() {
        assert_eq!(uptime(5), "5s");
        assert_eq!(uptime(125), "2m 5s");
        assert_eq!(uptime(7260), "2h 1m");
    }

    #[test]
    fn cli_parses() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
        let cli = Cli::try_parse_from(["red", "status", "--json"]).unwrap();
        assert!(cli.json);
        let cli = Cli::try_parse_from(["red", "isolate", "8.3", "--site", "blog"]).unwrap();
        assert!(matches!(cli.command, Command::Isolate { .. }));
        assert!(Cli::try_parse_from(["red", "php:list"]).is_ok());
        assert!(Cli::try_parse_from(["red", "use", "8.4"]).is_ok());
        assert!(Cli::try_parse_from(["red", "--version"]).is_err()); // prints version
        assert!(Cli::try_parse_from(["red", "links"]).is_ok());
    }

    #[test]
    fn finds_the_deepest_site_containing_a_directory() {
        let site = |name: &str, path: &str| SiteInfo {
            name: name.into(),
            path: path.into(),
            url: String::new(),
            kind: SiteKind::Linked,
            driver: "php".into(),
            php: None,
            isolated: false,
            secure: false,
            cert_expires: None,
        };
        let sites = vec![
            site("mono", "/nonexistent/mono"),
            site("api", "/nonexistent/mono/api"),
        ];
        let found = site_containing(&sites, Path::new("/nonexistent/mono/api/src"));
        assert_eq!(found.map(|s| s.name.as_str()), Some("api"));
        assert!(site_containing(&sites, Path::new("/elsewhere")).is_none());
    }
}
