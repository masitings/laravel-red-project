//! `red runtimes*` and `red php:install`: PHP, MySQL and PostgreSQL builds
//! that RED downloads and manages itself.

use std::io::Write;

use anyhow::{Context, bail};
use red_api::client::Method;
use red_api::{
    JobState, RuntimeInfo, RuntimeJob, RuntimeKind, RuntimeRequest, RuntimesResponse, endpoints,
};

use crate::{Ctx, print_table};

pub(crate) fn parse_kind(text: &str) -> anyhow::Result<RuntimeKind> {
    Ok(match red_core::runtimes::RuntimeKind::parse(text) {
        Some(red_core::runtimes::RuntimeKind::Php) => RuntimeKind::Php,
        Some(red_core::runtimes::RuntimeKind::Mysql) => RuntimeKind::Mysql,
        Some(red_core::runtimes::RuntimeKind::Postgres) => RuntimeKind::Postgres,
        Some(red_core::runtimes::RuntimeKind::Valkey) => RuntimeKind::Valkey,
        Some(red_core::runtimes::RuntimeKind::Node) => RuntimeKind::Node,
        Some(red_core::runtimes::RuntimeKind::Composer) => RuntimeKind::Composer,
        None => {
            bail!("unknown runtime {text:?}; use php, mysql, postgres, valkey, node or composer")
        }
    })
}

fn kind_name(kind: RuntimeKind) -> &'static str {
    match kind {
        RuntimeKind::Php => "php",
        RuntimeKind::Mysql => "mysql",
        RuntimeKind::Postgres => "postgres",
        RuntimeKind::Valkey => "valkey",
        RuntimeKind::Node => "node",
        RuntimeKind::Composer => "composer",
    }
}

pub(crate) async fn list(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let response: RuntimesResponse = ctx.client.get(endpoints::RUNTIMES).await?;
    ctx.output(&response, |r| {
        if r.installed.is_empty() {
            println!("No runtimes installed by RED yet.");
        } else {
            let rows: Vec<[String; 4]> = r
                .installed
                .iter()
                .map(|i| {
                    [
                        kind_name(i.kind).into(),
                        i.version.clone(),
                        update_text(i),
                        i.path.clone(),
                    ]
                })
                .collect();
            print_table(&["RUNTIME", "VERSION", "UPDATE", "PATH"], &rows);
            if r.installed.iter().any(|i| i.update.is_some()) {
                println!("Update with `red runtimes:update`.");
            }
        }
        if !r.available.is_empty() {
            println!(
                "\nAvailable for {} (install with `red runtimes:install <runtime> <line>`):",
                r.platform
            );
            for kind in [
                RuntimeKind::Php,
                RuntimeKind::Mysql,
                RuntimeKind::Postgres,
                RuntimeKind::Valkey,
                RuntimeKind::Node,
                RuntimeKind::Composer,
            ] {
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
    install_with(ctx, kind, version, true).await
}

/// Install a runtime on the way to running something else: progress on
/// stderr only, nothing on stdout.
pub(crate) async fn install_quietly(
    ctx: &Ctx<'_>,
    kind: RuntimeKind,
    version: &str,
) -> anyhow::Result<()> {
    install_with(ctx, kind, version, false).await
}

async fn install_with(
    ctx: &Ctx<'_>,
    kind: RuntimeKind,
    version: &str,
    report: bool,
) -> anyhow::Result<()> {
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
    if !report {
        return Ok(());
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
            RuntimeKind::Valkey => println!("Add it as a service: `red services:add valkey`."),
            RuntimeKind::Node => println!(
                "Use it in a project with `node = \"{}\"` in red.toml (or .nvmrc), then `red node`/`red npm`.",
                job.line
            ),
            RuntimeKind::Composer => println!("`red composer` uses it now."),
        }
    })
}

/// What the UPDATE column says for an installed runtime.
fn update_text(info: &RuntimeInfo) -> String {
    match &info.update {
        None => "-".into(),
        Some(version) if *version == info.version => format!("{version} (RED build)"),
        Some(version) => version.clone(),
    }
}

/// Update installed runtimes that have an update, optionally only one kind
/// or one line.
pub(crate) async fn update(
    ctx: &Ctx<'_>,
    kind: Option<RuntimeKind>,
    line: Option<&str>,
) -> anyhow::Result<()> {
    let response: RuntimesResponse = ctx.client.get(endpoints::RUNTIMES).await?;
    let selected: Vec<&RuntimeInfo> = response
        .installed
        .iter()
        .filter(|i| kind.is_none_or(|k| i.kind == k))
        .filter(|i| line.is_none_or(|l| i.line == l))
        .collect();
    if selected.is_empty() {
        match (kind, line) {
            (Some(kind), Some(line)) => bail!("{} {line} is not installed by RED", kind_name(kind)),
            (Some(kind), None) => bail!("no {} runtime is installed by RED", kind_name(kind)),
            _ => bail!("no runtimes are installed by RED"),
        }
    }
    for error in &response.catalog_errors {
        eprintln!("warning: {error}");
    }
    let pending: Vec<&RuntimeInfo> = selected
        .into_iter()
        .filter(|i| i.update.is_some())
        .collect();
    let mut updated = Vec::new();
    for info in &pending {
        install_quietly(ctx, info.kind, &info.line).await?;
        updated.push(serde_json::json!({
            "runtime": kind_name(info.kind),
            "line": info.line,
            "from": info.version,
            "to": info.update,
        }));
    }
    ctx.output(&serde_json::json!({ "updated": updated }), |_| {
        if pending.is_empty() {
            println!("Everything is up to date.");
        }
        for info in &pending {
            println!(
                "Updated {} {} → {}.",
                kind_name(info.kind),
                info.version,
                update_text(info)
            );
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
        assert!(matches!(parse_kind("redis"), Ok(RuntimeKind::Valkey)));
        assert!(parse_kind("mongo").is_err());

        let mut info = RuntimeInfo {
            kind: RuntimeKind::Php,
            line: "8.4".into(),
            version: "8.4.25".into(),
            path: "/x".into(),
            installed: 0,
            source: None,
            update: None,
        };
        assert_eq!(update_text(&info), "-");
        info.update = Some("8.4.26".into());
        assert_eq!(update_text(&info), "8.4.26");
        info.update = Some("8.4.25".into());
        assert_eq!(update_text(&info), "8.4.25 (RED build)");
    }
}
