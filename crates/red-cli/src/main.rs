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

mod inspect;
mod install;
mod project;
mod runtimes;
mod services;
mod system;
mod tools;
mod uninstall;

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
    /// Download and install a PHP version managed by RED (default: newest).
    #[command(name = "php:install")]
    PhpInstall {
        #[arg(value_name = "VERSION")]
        php: Option<String>,
    },
    /// Download and install a Node.js version managed by RED (default: newest LTS).
    #[command(name = "node:install")]
    NodeInstall {
        #[arg(value_name = "VERSION")]
        node: Option<String>,
    },
    /// Remove a PHP version installed with `red php:install`.
    #[command(name = "php:uninstall")]
    PhpUninstall {
        #[arg(value_name = "VERSION")]
        php: String,
    },
    /// List runtimes RED installed and the versions it can download.
    Runtimes,
    /// Download and install php, mysql, postgres, valkey, node or composer
    /// (default: newest version).
    #[command(name = "runtimes:install")]
    RuntimesInstall {
        #[arg(value_name = "RUNTIME")]
        kind: String,
        #[arg(value_name = "VERSION")]
        line: Option<String>,
    },
    /// Update runtimes installed by RED to the newest build of their line
    /// (all of them, one kind, or one line).
    #[command(name = "runtimes:update")]
    RuntimesUpdate {
        #[arg(value_name = "RUNTIME")]
        kind: Option<String>,
        #[arg(value_name = "VERSION")]
        line: Option<String>,
    },
    /// Remove a runtime installed by RED.
    #[command(name = "runtimes:remove")]
    RuntimesRemove {
        #[arg(value_name = "RUNTIME")]
        kind: String,
        #[arg(value_name = "VERSION")]
        line: String,
    },
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
    /// (Re)start redd, e.g. after installing a new version.
    Restart,
    /// Check that everything RED needs is in place.
    Doctor,
    /// One-time setup: helper, autostart, DNS and CA trust.
    Setup,
    /// Undo `red setup`: DNS, CA trust, autostart, the helper and the
    /// commands in ~/.local/bin. Project folders are never touched.
    Uninstall {
        /// Also delete RED's data: runtimes, databases, mail, certificates, logs.
        #[arg(long)]
        purge: bool,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
    },
    /// Point the OS at RED's DNS responder for the local TLD (uses sudo).
    #[command(name = "dns:install")]
    DnsInstall,
    /// Remove the resolver file installed by `red dns:install` (uses sudo).
    #[command(name = "dns:uninstall")]
    DnsUninstall,
    /// List database and cache services, and the servers RED found.
    Services,
    /// Add and start a service: mysql (or mariadb), postgres, redis (or valkey).
    #[command(name = "services:add")]
    ServicesAdd {
        #[arg(value_name = "KIND")]
        kind: String,
        /// Instance name (default: the kind), e.g. postgres-15.
        #[arg(long)]
        name: Option<String>,
        /// Port on 127.0.0.1 (default: 3306, 5432 or 6379).
        #[arg(long)]
        port: Option<u16>,
        /// Use the installed server with this version, e.g. 16 or 8.4.
        #[arg(long = "server-version", value_name = "VERSION")]
        server_version: Option<String>,
        /// Use this server binary.
        #[arg(long)]
        binary: Option<String>,
        /// Do not start it whenever redd starts.
        #[arg(long)]
        no_autostart: bool,
    },
    /// Start a service.
    #[command(name = "services:start")]
    ServicesStart { name: String },
    /// Stop a service.
    #[command(name = "services:stop")]
    ServicesStop { name: String },
    /// Restart a service.
    #[command(name = "services:restart")]
    ServicesRestart { name: String },
    /// Remove a service (its data is kept unless --delete-data is given).
    #[command(name = "services:remove")]
    ServicesRemove {
        name: String,
        #[arg(long)]
        delete_data: bool,
    },
    /// Print the .env settings for a service.
    #[command(name = "services:env")]
    ServicesEnv { name: String },
    /// Set up the project in the current directory from its red.toml
    /// (created on first run): link, HTTPS, PHP, services.
    Init,
    /// Run the project's PHP version.
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Php {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Run Composer with the project's PHP version.
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Composer {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Run the project's Node.js version (red.toml, .nvmrc or .node-version).
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Node {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Run npm from the project's Node.js version.
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Npm {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Run npx from the project's Node.js version.
    #[command(disable_help_flag = true, disable_version_flag = true)]
    Npx {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<std::ffi::OsString>,
    },
    /// Print shell commands that put the project's PHP and Node.js first on
    /// PATH: eval "$(red env)".
    Env,
    /// List caught mail.
    Mail {
        /// Only mail sent with this SMTP username.
        #[arg(long)]
        mailbox: Option<String>,
    },
    /// Show one caught message.
    #[command(name = "mail:show")]
    MailShow { id: String },
    /// Delete caught mail.
    #[command(name = "mail:clear")]
    MailClear {
        #[arg(long)]
        mailbox: Option<String>,
    },
    /// Show captured dump() output.
    Dumps {
        /// Keep printing new dumps as they arrive.
        #[arg(short, long)]
        follow: bool,
    },
    /// Send dump() output from sites to RED instead of the page.
    #[command(name = "dumps:on")]
    DumpsOn,
    /// Show dump() output in the page again.
    #[command(name = "dumps:off")]
    DumpsOff,
    /// Clear captured dumps.
    #[command(name = "dumps:clear")]
    DumpsClear,
    /// List logs, or show one: RED, services, PHP-FPM and site logs.
    Logs {
        /// Log id from `red logs`.
        id: Option<String>,
        /// Number of entries to show.
        #[arg(short = 'n', long, default_value_t = 50)]
        lines: usize,
        /// Keep printing new entries.
        #[arg(short, long)]
        follow: bool,
    },
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
        Command::Restart => {
            install::restart_agent(&paths.log_dir)?;
            ctx.wait_for_daemon().await?;
            let status = ctx.client.status().await?;
            println!("redd {} is running (pid {}).", status.version, status.pid);
        }
        Command::Doctor => {
            let report: DoctorReport = ctx.client.get(endpoints::DOCTOR).await?;
            ctx.output(&report, print_doctor)?;
            if report.checks.iter().any(|c| c.status == CheckStatus::Fail) {
                std::process::exit(1);
            }
        }
        Command::Uninstall { purge, yes } => uninstall::run(&ctx, &paths, *purge, *yes).await?,
        Command::Setup => {
            println!("Setting up RED. You may be asked for your password a few times.\n");
            println!("1/4  Privileged helper (ports 80 and 443)");
            install::install_helper(red_platform::user::current_uid())?;
            println!("\n2/4  Start redd at login");
            install::install_agent(&paths.log_dir)?;
            ctx.wait_for_daemon().await?;
            let php: PhpListResponse = ctx.client.get(endpoints::PHP).await?;
            if php.installs.is_empty() {
                println!("\n     No PHP found; installing RED's own build");
                runtimes::install(&ctx, red_api::RuntimeKind::Php, "").await?;
            }
            println!("\n3/4  DNS for .test");
            ctx.dns(true).await?;
            println!("\n4/4  Trust the local certificate authority");
            let status = ctx.client.status().await?;
            system::trust(Path::new(&status.paths.ca_cert))?;
            println!();
            let report: DoctorReport = ctx.client.get(endpoints::DOCTOR).await?;
            print_doctor(&report);
        }
        Command::PhpInstall { php } => {
            runtimes::install(
                &ctx,
                red_api::RuntimeKind::Php,
                php.as_deref().unwrap_or(""),
            )
            .await?
        }
        Command::PhpUninstall { php } => {
            let line = red_core::php::minor_version(php).unwrap_or_else(|| php.clone());
            runtimes::remove(&ctx, red_api::RuntimeKind::Php, &line).await?
        }
        Command::NodeInstall { node } => {
            runtimes::install(
                &ctx,
                red_api::RuntimeKind::Node,
                node.as_deref().unwrap_or(""),
            )
            .await?
        }
        Command::Runtimes => runtimes::list(&ctx).await?,
        Command::RuntimesInstall { kind, line } => {
            runtimes::install(
                &ctx,
                runtimes::parse_kind(kind)?,
                line.as_deref().unwrap_or(""),
            )
            .await?
        }
        Command::RuntimesUpdate { kind, line } => {
            let kind = kind.as_deref().map(runtimes::parse_kind).transpose()?;
            runtimes::update(&ctx, kind, line.as_deref()).await?
        }
        Command::RuntimesRemove { kind, line } => {
            runtimes::remove(&ctx, runtimes::parse_kind(kind)?, line).await?
        }
        Command::Services => services::list(&ctx).await?,
        Command::ServicesAdd {
            kind,
            name,
            port,
            server_version,
            binary,
            no_autostart,
        } => {
            services::add(
                &ctx,
                services::AddArgs {
                    kind,
                    name: name.clone(),
                    port: *port,
                    version: server_version.clone(),
                    binary: binary.clone(),
                    autostart: !no_autostart,
                },
            )
            .await?
        }
        Command::ServicesStart { name } => services::action(&ctx, name, "start").await?,
        Command::ServicesStop { name } => services::action(&ctx, name, "stop").await?,
        Command::ServicesRestart { name } => services::action(&ctx, name, "restart").await?,
        Command::ServicesRemove { name, delete_data } => {
            services::remove(&ctx, name, *delete_data).await?
        }
        Command::ServicesEnv { name } => services::env(&ctx, name).await?,
        Command::Init => project::init(&ctx).await?,
        Command::Php { args } => tools::php(&ctx, args).await?,
        Command::Composer { args } => tools::composer(&ctx, args).await?,
        Command::Node { args } => tools::node(&ctx, "node", args).await?,
        Command::Npm { args } => tools::node(&ctx, "npm", args).await?,
        Command::Npx { args } => tools::node(&ctx, "npx", args).await?,
        Command::Env => tools::env(&ctx).await?,
        Command::Mail { mailbox } => inspect::mail_list(&ctx, mailbox.as_deref()).await?,
        Command::MailShow { id } => inspect::mail_show(&ctx, id).await?,
        Command::MailClear { mailbox } => inspect::mail_clear(&ctx, mailbox.as_deref()).await?,
        Command::Dumps { follow } => inspect::dumps(&ctx, *follow).await?,
        Command::DumpsOn => inspect::dumps_set(&ctx, true).await?,
        Command::DumpsOff => inspect::dumps_set(&ctx, false).await?,
        Command::DumpsClear => inspect::dumps_clear(&ctx).await?,
        Command::Logs { id, lines, follow } => {
            inspect::logs(&ctx, id.as_deref(), *lines, *follow).await?
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
    match status.smtp_port {
        Some(port) => println!("mail     smtp://127.0.0.1:{port}"),
        None => println!("mail     not running (see the redd log)"),
    }
    match status.dump_port {
        Some(port) => println!("dumps    127.0.0.1:{port}"),
        None => println!("dumps    not running (see the redd log)"),
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
        Some(ClientError::DaemonNotRunning { .. }) => Some(
            "start it with `red restart` (or run `redd` in a terminal); see `red setup` for a first install"
                .to_owned(),
        ),
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
        assert!(Cli::try_parse_from(["red", "php:install"]).is_ok());
        assert!(Cli::try_parse_from(["red", "php:install", "8.3"]).is_ok());
        assert!(Cli::try_parse_from(["red", "runtimes:install", "postgres", "17"]).is_ok());
        assert!(Cli::try_parse_from(["red", "runtimes:update"]).is_ok());
        assert!(Cli::try_parse_from(["red", "uninstall", "--purge", "-y"]).is_ok());
        assert!(Cli::try_parse_from(["red", "runtimes:update", "php", "8.4"]).is_ok());
        assert!(Cli::try_parse_from(["red", "use", "8.4"]).is_ok());
        assert!(Cli::try_parse_from(["red", "--version"]).is_err()); // prints version
        assert!(Cli::try_parse_from(["red", "links"]).is_ok());
        assert!(
            Cli::try_parse_from(["red", "services:add", "postgres", "--server-version", "16"])
                .is_ok()
        );
        assert!(Cli::try_parse_from(["red", "services:remove", "mysql", "--delete-data"]).is_ok());
        let cli = Cli::try_parse_from(["red", "php", "-v"]).unwrap();
        assert!(matches!(&cli.command, Command::Php { args } if args == &["-v"]));
        let cli = Cli::try_parse_from(["red", "npm", "run", "dev", "--", "--host"]).unwrap();
        assert!(matches!(&cli.command, Command::Npm { args } if args.len() == 4));
        assert!(Cli::try_parse_from(["red", "logs", "daemon", "-n", "10", "-f"]).is_ok());
        let cli = Cli::try_parse_from(["red", "composer", "--version"]).unwrap();
        assert!(matches!(&cli.command, Command::Composer { args } if args == &["--version"]));
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
