//! `red new`: create a Laravel, WordPress or empty project and serve it.

use std::path::PathBuf;

use anyhow::bail;
use red_api::client::Method;
use red_api::{ProjectJob, ProjectKind, ProjectRequest, ProjectState, endpoints};

use crate::Ctx;

/// What to create, as `red new --kind` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum Kind {
    /// Laravel (laravel/laravel).
    Laravel,
    /// Laravel with the React starter kit.
    React,
    /// Laravel with the Vue starter kit.
    Vue,
    /// Laravel with the Livewire starter kit.
    Livewire,
    /// The latest WordPress, on a MySQL service.
    Wordpress,
    /// An empty site with public/index.php.
    Blank,
}

impl From<Kind> for ProjectKind {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Laravel => Self::Laravel,
            Kind::React => Self::LaravelReact,
            Kind::Vue => Self::LaravelVue,
            Kind::Livewire => Self::LaravelLivewire,
            Kind::Wordpress => Self::Wordpress,
            Kind::Blank => Self::Blank,
        }
    }
}

pub(crate) async fn run(
    ctx: &Ctx<'_>,
    name: &str,
    kind: Kind,
    dir: Option<PathBuf>,
    secure: bool,
    database: Option<String>,
) -> anyhow::Result<()> {
    // Default to the current directory, like `composer create-project`.
    let directory = match dir {
        Some(dir) => crate::absolute_dir(Some(&dir))?,
        None => crate::absolute_dir(None)?,
    };
    let request = ProjectRequest {
        name: name.to_owned(),
        kind: kind.into(),
        directory: Some(directory.display().to_string()),
        secure,
        database,
    };
    let job: ProjectJob = ctx
        .send(Method::POST, endpoints::PROJECTS, Some(&request))
        .await?;
    if !ctx.cli.json {
        println!("Creating {} in {}", job.name, job.path);
    }
    let mut last: Option<String> = None;
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let jobs: Vec<ProjectJob> = ctx.client.get(endpoints::PROJECTS).await?;
        let Some(current) = jobs.into_iter().find(|j| j.id == job.id) else {
            bail!("redd forgot about the project; was it restarted?");
        };
        if !ctx.cli.json {
            // The log is a window of the last lines; go on after the last
            // line printed (all of it when that scrolled out).
            let start = last
                .as_ref()
                .and_then(|line| current.log.iter().rposition(|l| l == line))
                .map_or(0, |i| i + 1);
            for line in &current.log[start..] {
                println!("{line}");
            }
            if let Some(line) = current.log.last() {
                last = Some(line.clone());
            }
        }
        match current.state {
            ProjectState::Running => {}
            ProjectState::Done => {
                return ctx.output(&current, |job| {
                    println!(
                        "\n{} is ready at {}",
                        job.name,
                        job.url.as_deref().unwrap_or("?")
                    );
                });
            }
            ProjectState::Failed => {
                if ctx.cli.json {
                    println!("{}", serde_json::to_string_pretty(&current)?);
                }
                bail!(
                    "creating {} failed: {}",
                    current.name,
                    current.error.as_deref().unwrap_or("unknown error")
                );
            }
        }
    }
}
