use std::process::ExitCode;

use clap::{Parser, Subcommand};
use red_api::StatusResponse;
use red_api::client::{Client, ClientError};
use red_platform::Paths;

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

async fn run(cli: &Cli) -> anyhow::Result<()> {
    let paths = Paths::discover()?;
    let client = Client::new(paths.socket());

    match cli.command {
        Command::Status => {
            let status = client.status().await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            } else {
                print_status(&status);
            }
        }
    }
    Ok(())
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
    println!("tld      .{}", status.tld);
    println!("config   {}", status.paths.config_dir);
    println!("data     {}", status.paths.data_dir);
    println!("socket   {}", status.paths.socket);
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
    }
}
