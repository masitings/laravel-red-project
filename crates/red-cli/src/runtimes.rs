//! `red runtimes*` and `red php:install`: PHP, MySQL and PostgreSQL builds
//! that RED downloads and manages itself.

use std::io::Write;

use anyhow::{Context, bail};
use red_api::client::Method;
use red_api::{JobState, RuntimeJob, RuntimeKind, RuntimeRequest, RuntimesResponse, endpoints};

use crate::{Ctx, print_table};

pub(crate) fn parse_kind(text: &str) -> anyhow::Result<RuntimeKind> {
    Ok(match red_core::runtimes::RuntimeKind::parse(text) {
        Some(red_core::runtimes::RuntimeKind::Php) => RuntimeKind::Php,
        Some(red_core::runtimes::RuntimeKind::Mysql) => RuntimeKind::Mysql,
        Some(red_core::runtimes::RuntimeKind::Postgres) => RuntimeKind::Postgres,
        None => bail!("unknown runtime {text:?}; use php, mysql or postgres"),
    })
}

fn kind_name(kind: RuntimeKind) -> &'static str {
    match kind {
        RuntimeKind::Php => "php",
        RuntimeKind::Mysql => "mysql",
        RuntimeKind::Postgres => "postgres",
    }
}

pub(crate) async fn list(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let response: RuntimesResponse = ctx.client.get(endpoints::RUNTIMES).await?;
    ctx.output(&response, |r| {
        if r.installed.is_empty() {
            println!("No runtimes installed by RED yet.");
        } else {
            let rows: Vec<[String; 3]> = r
                .installed
                .iter()
                .map(|i| [kind_name(i.kind).into(), i.version.clone(), i.path.clone()])
                .collect();
            print_table(&["RUNTIME", "VERSION", "PATH"], &rows);
        }
        if !r.available.is_empty() {
            println!(
                "\nAvailable for {} (install with `red runtimes:install <runtime> <line>`):",
                r.platform
            );
            for kind in [RuntimeKind::Php, RuntimeKind::Mysql, RuntimeKind::Postgres] {
                let versions: Vec<String> =
                    r.available
                        .iter()
                        .filter(|a| a.kind == kind)
                        .map(|a| {
                            let installed = r.installed.iter().any(|i| {
                                i.kind == kind && i.line == a.line && i.version == a.version
                            });
                            format!("{}{}", a.version, if installed { " ✓" } else { "" })
                        })
                        .collect();
                if !versions.is_empty() {
                    println!("  {:<9} {}", kind_name(kind), versions.join(", "));
                }
            }
        }
        for job in r
            .jobs
            .iter()
            .filter(|j| matches!(j.state, JobState::Downloading | JobState::Extracting))
        {
            println!(
                "\nInstalling {} {}: {}",
                kind_name(job.kind),
                job.version,
                progress_text(job)
            );
        }
        for error in &r.catalog_errors {
            eprintln!("warning: {error}");
        }
    })
}

/// Install a runtime and wait for it, showing progress.
pub(crate) async fn install(ctx: &Ctx<'_>, kind: RuntimeKind, version: &str) -> anyhow::Result<()> {
    let mut job: RuntimeJob = ctx
        .send(
            Method::POST,
            endpoints::RUNTIMES,
            Some(&RuntimeRequest {
                kind,
                version: version.to_owned(),
            }),
        )
        .await?;
    let interactive = !ctx.cli.json;
    if interactive {
        eprintln!("Installing {} {}…", kind_name(kind), job.version);
    }
    loop {
        if interactive {
            eprint!("\r\x1b[2K  {}", progress_text(&job));
            let _ = std::io::stderr().flush();
        }
        match job.state {
            JobState::Done => break,
            JobState::Failed => {
                if interactive {
                    eprintln!();
                }
                bail!(
                    "installing {} {} failed: {}",
                    kind_name(kind),
                    job.version,
                    job.error.as_deref().unwrap_or("unknown error")
                );
            }
            JobState::Downloading | JobState::Extracting => {}
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let response: RuntimesResponse = ctx.client.get(endpoints::RUNTIMES).await?;
        job = response
            .jobs
            .into_iter()
            .find(|j| j.id == job.id)
            .context("the install disappeared; is redd still running?")?;
    }
    if interactive {
        eprintln!();
    }
    ctx.output(&job, |job| {
        println!("Installed {} {}.", kind_name(job.kind), job.version);
        match job.kind {
            RuntimeKind::Php => println!(
                "Sites can use it now: `red use {}` or `red isolate {}`.",
                job.line, job.line
            ),
            RuntimeKind::Mysql => println!("Add it as a service: `red services:add mysql`."),
            RuntimeKind::Postgres => println!("Add it as a service: `red services:add postgres`."),
        }
    })
}

pub(crate) async fn remove(ctx: &Ctx<'_>, kind: RuntimeKind, line: &str) -> anyhow::Result<()> {
    let _: serde_json::Value = ctx
        .send(
            Method::DELETE,
            &endpoints::runtime(kind_name(kind), line),
            None::<&()>,
        )
        .await?;
    ctx.output(
        &serde_json::json!({ "removed": format!("{} {line}", kind_name(kind)) }),
        |_| {
            println!("Removed {} {line}.", kind_name(kind));
        },
    )
}

fn progress_text(job: &RuntimeJob) -> String {
    let mb = |b: u64| b as f64 / 1_048_576.0;
    match job.state {
        JobState::Downloading => match job.total {
            Some(total) if total > 0 => format!(
                "downloading {:.0}% ({:.1} of {:.1} MB)",
                job.received as f64 * 100.0 / total as f64,
                mb(job.received),
                mb(total)
            ),
            _ => format!("downloading {:.1} MB", mb(job.received)),
        },
        JobState::Extracting => "unpacking…".into(),
        JobState::Done => "done".into(),
        JobState::Failed => "failed".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_progress() {
        let mut job = RuntimeJob {
            id: 1,
            kind: RuntimeKind::Php,
            line: "8.4".into(),
            version: "8.4.23".into(),
            state: JobState::Downloading,
            received: 1_048_576,
            total: Some(4_194_304),
            error: None,
        };
        assert_eq!(progress_text(&job), "downloading 25% (1.0 of 4.0 MB)");
        job.total = None;
        assert_eq!(progress_text(&job), "downloading 1.0 MB");
        assert!(parse_kind("postgresql").is_ok());
        assert!(parse_kind("redis").is_err());
    }
}
