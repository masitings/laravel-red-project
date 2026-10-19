//! `red db*`: databases in MySQL, MariaDB and PostgreSQL services.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, bail};
use red_api::client::Method;
use red_api::{
    BackupInfo, DatabaseRequest, DatabasesResponse, RestoreRequest, ServiceKind, ServicesResponse,
    endpoints,
};

use crate::{Ctx, print_table};

/// The service to work on: the one named, else the only database service.
async fn service(ctx: &Ctx<'_>, name: Option<&str>) -> anyhow::Result<String> {
    if let Some(name) = name {
        return Ok(name.to_owned());
    }
    let services: ServicesResponse = ctx.client.get(endpoints::SERVICES).await?;
    let databases: Vec<String> = services
        .services
        .iter()
        .filter(|s| matches!(s.kind, ServiceKind::Mysql | ServiceKind::Postgres))
        .map(|s| s.name.clone())
        .collect();
    match databases.as_slice() {
        [] => bail!(
            "no MySQL, MariaDB or PostgreSQL service yet; add one with `red services:add mysql`"
        ),
        [one] => Ok(one.clone()),
        many => bail!(
            "several database services ({}); pick one with --service",
            many.join(", ")
        ),
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        _ => format!("{:.1} GB", bytes as f64 / 1_073_741_824.0),
    }
}

pub(crate) async fn list(ctx: &Ctx<'_>, name: Option<&str>) -> anyhow::Result<()> {
    let service = service(ctx, name).await?;
    let response: DatabasesResponse = ctx.client.get(&endpoints::databases(&service)).await?;
    ctx.output(&response, print_list)
}

fn print_list(r: &DatabasesResponse) {
    if r.databases.is_empty() {
        println!(
            "{} has no databases yet. Create one with `red db:create <name>`.",
            r.service
        );
    } else {
        let rows: Vec<[String; 2]> = r
            .databases
            .iter()
            .map(|d| {
                [
                    d.name.clone(),
                    d.size.map(human_size).unwrap_or_else(|| "-".into()),
                ]
            })
            .collect();
        print_table(&["DATABASE", "SIZE"], &rows);
    }
    if !r.backups.is_empty() {
        println!("\nBackups:");
        for b in r.backups.iter().take(10) {
            println!(
                "  {}  {}  {}",
                red_core::time::datetime_from_unix(b.created),
                human_size(b.size),
                b.file
            );
        }
    }
}

pub(crate) async fn create(ctx: &Ctx<'_>, db: &str, name: Option<&str>) -> anyhow::Result<()> {
    let service = service(ctx, name).await?;
    let response: DatabasesResponse = ctx
        .send(
            Method::POST,
            &endpoints::databases(&service),
            Some(&DatabaseRequest {
                name: db.to_owned(),
            }),
        )
        .await?;
    ctx.output(&response, |_| println!("Created {db} in {service}."))
}

pub(crate) async fn drop(
    ctx: &Ctx<'_>,
    db: &str,
    name: Option<&str>,
    yes: bool,
) -> anyhow::Result<()> {
    let service = service(ctx, name).await?;
    if !yes {
        print!("Drop {db} in {service}, with all its data? Type the name to confirm: ");
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim() != db {
            bail!("not dropped");
        }
    }
    let response: DatabasesResponse = ctx
        .send(
            Method::DELETE,
            &endpoints::database(&service, db),
            None::<&()>,
        )
        .await?;
    ctx.output(&response, |_| println!("Dropped {db}."))
}

pub(crate) async fn backup(ctx: &Ctx<'_>, db: &str, name: Option<&str>) -> anyhow::Result<()> {
    let service = service(ctx, name).await?;
    let info: BackupInfo = ctx
        .send(
            Method::POST,
            &endpoints::database_action(&service, db, "backup"),
            None::<&()>,
        )
        .await?;
    ctx.output(&info, |b| {
        println!(
            "Backed up {} ({}) to {}",
            b.database,
            human_size(b.size),
            b.file
        );
        println!("Restore with `red db:restore {} {}`.", b.database, b.file);
    })
}

pub(crate) async fn restore(
    ctx: &Ctx<'_>,
    db: &str,
    file: &Path,
    name: Option<&str>,
) -> anyhow::Result<()> {
    let service = service(ctx, name).await?;
    let file = std::path::absolute(file).context("bad path")?;
    let response: DatabasesResponse = ctx
        .send(
            Method::POST,
            &endpoints::database_action(&service, db, "restore"),
            Some(&RestoreRequest {
                file: file.display().to_string(),
                create: true,
            }),
        )
        .await?;
    ctx.output(&response, |_| {
        println!("Restored {} into {db} ({service}).", file.display());
    })
}
