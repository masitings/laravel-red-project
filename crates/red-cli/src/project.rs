//! `red init`: create or apply a project's `red.toml`.

use std::path::Path;

use anyhow::Context;
use red_api::client::Method;
use red_api::{
    PathRequest, ServiceInfo, ServiceRequest, ServiceStatus, ServicesResponse, SiteInfo,
    StatusResponse, endpoints,
};
use red_core::project::{FILE_NAME, ProjectConfig, node_version_file};

use crate::services::{kind_name, parse_kind};
use crate::{Ctx, absolute_dir};

pub(crate) async fn init(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let dir = absolute_dir(None)?;
    let status = ctx.client.status().await?;
    let sites: Vec<SiteInfo> = ctx.client.get(endpoints::SITES).await?;
    let current = sites.iter().find(|s| same_dir(Path::new(&s.path), &dir));

    let config = match ProjectConfig::load(&dir)? {
        Some(config) => {
            println!("Applying {FILE_NAME}.");
            config
        }
        None => {
            let config = ProjectConfig {
                name: Some(match current {
                    Some(site) => site.name.clone(),
                    None => default_name(&dir)?,
                }),
                php: match current.and_then(|s| s.php.clone()) {
                    Some(php) => Some(php),
                    None => ctx
                        .client
                        .get::<red_api::PhpListResponse>(endpoints::PHP)
                        .await
                        .ok()
                        .and_then(|list| list.default),
                },
                secure: Some(current.is_some_and(|s| s.secure)),
                node: node_version_file(&dir),
                services: Vec::new(),
            };
            std::fs::write(dir.join(FILE_NAME), config.render())
                .with_context(|| format!("cannot write {FILE_NAME}"))?;
            println!("Created {FILE_NAME}. Commit it so others get the same setup.");
            config
        }
    };

    // 1. Serve the directory under the configured name.
    let name = match &config.name {
        Some(name) => name.clone(),
        None => default_name(&dir)?,
    };
    let mut site = match current {
        Some(site) if site.name == name => site.clone(),
        _ => {
            let site: SiteInfo = ctx
                .send(
                    Method::POST,
                    endpoints::LINKS,
                    Some(&PathRequest {
                        path: dir.display().to_string(),
                        name: Some(name.clone()),
                    }),
                )
                .await?;
            println!("  linked   {}", site.url);
            site
        }
    };

    // 2. HTTPS.
    if let Some(secure) = config.secure
        && secure != site.secure
    {
        let method = if secure { Method::PUT } else { Method::DELETE };
        site = ctx
            .send(method, &endpoints::site_secure(&site.name), None::<&()>)
            .await?;
        println!(
            "  {}  {}",
            if secure { "secured " } else { "unsecured" },
            site.url
        );
    }

    // 3. PHP: redd reads `php` from red.toml itself; only an explicit
    //    `red isolate` overrides it.
    if let Some(wanted) = config.php.as_deref().and_then(red_core::php::minor_version) {
        if site.isolated && site.php.as_deref() != Some(wanted.as_str()) {
            println!(
                "  note     {} is isolated to PHP {}; run `red unisolate` to follow {FILE_NAME} ({wanted})",
                site.name,
                site.php.as_deref().unwrap_or("?")
            );
        } else if site.php.as_deref() != Some(wanted.as_str()) {
            println!("  warning  PHP {wanted} is not installed; see `red php:list`");
        }
    }

    // 4. Services.
    let mut used: Vec<ServiceInfo> = Vec::new();
    if !config.services.is_empty() {
        let list: ServicesResponse = ctx.client.get(endpoints::SERVICES).await?;
        for wanted in &config.services {
            let service = ensure_service(ctx, &list, wanted).await?;
            println!(
                "  service  {} on 127.0.0.1:{} ({})",
                service.name,
                service.port,
                match service.status {
                    ServiceStatus::Running => "running",
                    ServiceStatus::Starting => "starting",
                    ServiceStatus::Stopped => "stopped",
                    ServiceStatus::Failed => "FAILED, see `red services`",
                }
            );
            used.push(service);
        }
    }

    println!("\n{} is ready at {}", site.name, site.url);
    print_env(&site, &status, &used);
    Ok(())
}

/// Find a service by instance name or kind, starting or adding it.
async fn ensure_service(
    ctx: &Ctx<'_>,
    list: &ServicesResponse,
    wanted: &str,
) -> anyhow::Result<ServiceInfo> {
    let existing = list.services.iter().find(|s| s.name == wanted).or_else(|| {
        let kind = parse_kind(wanted).ok()?;
        list.services.iter().find(|s| s.kind == kind)
    });
    let service = match existing {
        Some(service) if service.status == ServiceStatus::Running => return Ok(service.clone()),
        Some(service) => {
            ctx.send(
                Method::POST,
                &endpoints::service_action(&service.name, "start"),
                None::<&()>,
            )
            .await?
        }
        None => {
            let kind = parse_kind(wanted)
                .with_context(|| format!("{FILE_NAME} lists an unknown service {wanted:?}"))?;
            ctx.send(
                Method::POST,
                endpoints::SERVICES,
                Some(&ServiceRequest {
                    kind,
                    name: None,
                    port: None,
                    version: None,
                    binary: None,
                    autostart: None,
                }),
            )
            .await
            .with_context(|| format!("cannot add the {} service", kind_name(kind)))?
        }
    };
    wait(ctx, service).await
}

async fn wait(ctx: &Ctx<'_>, mut service: ServiceInfo) -> anyhow::Result<ServiceInfo> {
    for _ in 0..600 {
        if service.status != ServiceStatus::Starting {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        service = ctx.client.get(&endpoints::service(&service.name)).await?;
    }
    Ok(service)
}

fn print_env(site: &SiteInfo, status: &StatusResponse, services: &[ServiceInfo]) {
    println!("\nFor your .env:");
    println!("  APP_URL={}", site.url);
    let mut db_done = false;
    for service in services {
        let is_db = service.env.iter().any(|v| v.name == "DB_CONNECTION");
        if is_db && db_done {
            continue;
        }
        db_done |= is_db;
        for var in &service.env {
            println!("  {}={}", var.name, var.value);
        }
    }
    if let Some(port) = status.smtp_port {
        println!("  MAIL_MAILER=smtp");
        println!("  MAIL_HOST=127.0.0.1");
        println!("  MAIL_PORT={port}");
        println!("  MAIL_USERNAME={}", site.name);
    }
}

fn default_name(dir: &Path) -> anyhow::Result<String> {
    let base = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    red_core::sites::normalize_name(&base).with_context(|| {
        format!("{base:?} is not usable as a site name; set `name` in {FILE_NAME}")
    })
}

fn same_dir(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    canonical(a) == canonical(b)
}
