//! `red services*`: database and cache services.

use red_api::client::Method;
use red_api::{
    ServiceInfo, ServiceKind, ServiceRequest, ServiceStatus, ServicesResponse, endpoints,
};

use crate::{Ctx, print_table};

pub(crate) fn parse_kind(text: &str) -> anyhow::Result<ServiceKind> {
    Ok(match red_core::ServiceKind::parse(text) {
        Some(red_core::ServiceKind::Mysql) => ServiceKind::Mysql,
        Some(red_core::ServiceKind::Postgres) => ServiceKind::Postgres,
        Some(red_core::ServiceKind::Redis) => ServiceKind::Redis,
        None => {
            anyhow::bail!("unknown service {text:?}; use mysql, mariadb, postgres, redis or valkey")
        }
    })
}

pub(crate) fn kind_name(kind: ServiceKind) -> &'static str {
    match kind {
        ServiceKind::Mysql => "mysql",
        ServiceKind::Postgres => "postgres",
        ServiceKind::Redis => "redis",
    }
}

fn status_name(status: ServiceStatus) -> &'static str {
    match status {
        ServiceStatus::Stopped => "stopped",
        ServiceStatus::Starting => "starting",
        ServiceStatus::Running => "running",
        ServiceStatus::Failed => "FAILED",
    }
}

pub(crate) async fn list(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let response: ServicesResponse = ctx.client.get(endpoints::SERVICES).await?;
    ctx.output(&response, |r| {
        if r.services.is_empty() {
            println!(
                "No services yet. Add one with `red services:add mysql` (or postgres, redis)."
            );
        } else {
            let rows: Vec<[String; 5]> = r
                .services
                .iter()
                .map(|s| {
                    let server = match (&s.flavor, &s.version) {
                        (Some(f), Some(v)) => format!("{f} {v}"),
                        (None, Some(v)) => format!("{} {v}", kind_name(s.kind)),
                        _ => kind_name(s.kind).to_owned(),
                    };
                    let auto = if s.autostart { "yes" } else { "no" };
                    [
                        s.name.clone(),
                        server,
                        s.port.to_string(),
                        status_name(s.status).to_owned(),
                        auto.to_owned(),
                    ]
                })
                .collect();
            print_table(&["NAME", "SERVER", "PORT", "STATUS", "AUTOSTART"], &rows);
            for s in r
                .services
                .iter()
                .filter(|s| s.status == ServiceStatus::Failed)
            {
                if let Some(error) = &s.error {
                    println!("\n{} failed: {error}\nlog: {}", s.name, s.log_file);
                }
            }
        }
        if !r.available.is_empty() {
            println!("\nServers found on this machine:");
            for i in &r.available {
                println!("  {:<10} {:<10} {}", i.flavor, i.full_version, i.binary);
            }
        }
    })
}

pub(crate) struct AddArgs<'a> {
    pub kind: &'a str,
    pub name: Option<String>,
    pub port: Option<u16>,
    pub version: Option<String>,
    pub binary: Option<String>,
    pub autostart: bool,
}

pub(crate) async fn add(ctx: &Ctx<'_>, args: AddArgs<'_>) -> anyhow::Result<()> {
    // `mariadb` and `valkey` pick that flavor (and name the service after it).
    let flavor = match args.kind.trim().to_ascii_lowercase().as_str() {
        f @ ("mariadb" | "valkey") => Some(f.to_owned()),
        _ => None,
    };
    let request = ServiceRequest {
        kind: parse_kind(args.kind)?,
        name: args.name.or_else(|| flavor.clone()),
        port: args.port,
        version: args.version.or(flavor),
        binary: args.binary,
        autostart: Some(args.autostart),
    };
    let service: ServiceInfo = ctx
        .send(Method::POST, endpoints::SERVICES, Some(&request))
        .await?;
    let service = wait_until_settled(ctx, service).await?;
    ctx.output(&service, print_started)
}

pub(crate) async fn action(ctx: &Ctx<'_>, name: &str, action: &str) -> anyhow::Result<()> {
    let service: ServiceInfo = ctx
        .send(
            Method::POST,
            &endpoints::service_action(name, action),
            None::<&()>,
        )
        .await?;
    let service = if action == "stop" {
        service
    } else {
        wait_until_settled(ctx, service).await?
    };
    ctx.output(&service, |s| {
        if action == "stop" {
            println!("Stopped {}.", s.name);
        } else {
            print_started(s);
        }
    })
}

pub(crate) async fn remove(ctx: &Ctx<'_>, name: &str, delete_data: bool) -> anyhow::Result<()> {
    let path = if delete_data {
        format!("{}?delete_data=true", endpoints::service(name))
    } else {
        endpoints::service(name)
    };
    let _: serde_json::Value = ctx.send(Method::DELETE, &path, None::<&()>).await?;
    ctx.output(&serde_json::json!({ "removed": name }), |_| {
        if delete_data {
            println!("Removed {name} and deleted its data.");
        } else {
            println!("Removed {name}. Its data was kept; add it again to reuse it.");
        }
    })
}

pub(crate) async fn env(ctx: &Ctx<'_>, name: &str) -> anyhow::Result<()> {
    let service: ServiceInfo = ctx.client.get(&endpoints::service(name)).await?;
    ctx.output(&service.env, |env| {
        for var in env {
            println!("{}={}", var.name, var.value);
        }
    })
}

/// Poll until a service is running or failed (databases can take a while
/// to initialise on first start).
async fn wait_until_settled(
    ctx: &Ctx<'_>,
    mut service: ServiceInfo,
) -> anyhow::Result<ServiceInfo> {
    let mut announced = false;
    for _ in 0..1200 {
        if service.status != ServiceStatus::Starting {
            break;
        }
        if !announced && !ctx.cli.json {
            eprintln!("Starting {}…", service.name);
            announced = true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        service = ctx.client.get(&endpoints::service(&service.name)).await?;
    }
    if service.status == ServiceStatus::Failed {
        anyhow::bail!(
            "{} failed to start: {}\nlog: {}",
            service.name,
            service.error.as_deref().unwrap_or("unknown error"),
            service.log_file
        );
    }
    Ok(service)
}

fn print_started(s: &ServiceInfo) {
    let server = s.flavor.as_deref().unwrap_or(kind_name(s.kind));
    println!(
        "{} ({server} {}) is {} on 127.0.0.1:{}",
        s.name,
        s.version.as_deref().unwrap_or(""),
        status_name(s.status),
        s.port
    );
    println!("\nFor your .env:");
    for var in &s.env {
        println!("  {}={}", var.name, var.value);
    }
}

pub(crate) struct SetArgs {
    pub port: Option<u16>,
    pub version: Option<String>,
    pub binary: Option<String>,
    pub autostart: Option<bool>,
}

/// `red services:set`: change a service's port, server version or autostart.
pub(crate) async fn set(ctx: &Ctx<'_>, name: &str, args: SetArgs) -> anyhow::Result<()> {
    if args.port.is_none()
        && args.version.is_none()
        && args.binary.is_none()
        && args.autostart.is_none()
    {
        anyhow::bail!("nothing to change; pass --port, --server-version, --binary or --autostart");
    }
    let port_changed = args.port.is_some();
    let update = red_api::ServiceUpdate {
        port: args.port,
        version: args.version,
        binary: args.binary,
        autostart: args.autostart,
    };
    let service: ServiceInfo = ctx
        .send(Method::PUT, &endpoints::service(name), Some(&update))
        .await?;
    let service = if service.status == red_api::ServiceStatus::Stopped {
        service
    } else {
        wait_until_settled(ctx, service).await?
    };
    ctx.output(&service, |s| {
        println!(
            "{}: {} {} on port {}{}.",
            s.name,
            s.flavor.as_deref().unwrap_or(kind_name(s.kind)),
            s.version.as_deref().unwrap_or("?"),
            s.port,
            if s.autostart {
                ", starts with redd"
            } else {
                ""
            }
        );
        if port_changed {
            println!(
                "Update the port in your projects' .env (see `red services:env {}`).",
                s.name
            );
        }
    })
}
