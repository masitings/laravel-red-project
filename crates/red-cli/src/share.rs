//! `red share`, `red unshare`, `red shares`, `red share:login`: put a site on
//! the internet through Cloudflare Tunnel.

use anyhow::bail;
use red_api::client::Method;
use red_api::{ShareInfo, ShareLoginResponse, ShareRequest, ShareState, SharesResponse, endpoints};

use crate::{Ctx, print_table};

pub(crate) async fn share(
    ctx: &Ctx<'_>,
    site: &str,
    hostname: Option<String>,
) -> anyhow::Result<()> {
    let info: ShareInfo = ctx
        .send(
            Method::POST,
            &endpoints::share(site),
            Some(&ShareRequest { hostname }),
        )
        .await?;
    let mut last_step = String::new();
    for _ in 0..600 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let response: SharesResponse = ctx.client.get(endpoints::SHARES).await?;
        let Some(current) = response.shares.into_iter().find(|s| s.site == info.site) else {
            bail!("the share of {site} stopped");
        };
        if !ctx.cli.json && current.step != last_step && current.state == ShareState::Starting {
            eprintln!("{}…", current.step);
            last_step = current.step.clone();
        }
        match current.state {
            ShareState::Starting => {}
            ShareState::Running => {
                return ctx.output(&current, |s| {
                    println!(
                        "{} is on the internet at {}",
                        s.site,
                        s.url.as_deref().unwrap_or("?")
                    );
                    println!(
                        "Anyone with the link can open it. Stop with `red unshare {}`.",
                        s.site
                    );
                });
            }
            ShareState::Failed => bail!(
                "sharing {site} failed: {}",
                current.error.as_deref().unwrap_or("unknown error")
            ),
        }
    }
    bail!("sharing {site} took too long; see `red shares`")
}

pub(crate) async fn unshare(ctx: &Ctx<'_>, site: &str) -> anyhow::Result<()> {
    let _: serde_json::Value = ctx
        .send(Method::DELETE, &endpoints::share(site), None::<&()>)
        .await?;
    ctx.output(&serde_json::json!({ "unshared": site }), |_| {
        println!("{site} is no longer shared.");
    })
}

pub(crate) async fn list(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let response: SharesResponse = ctx.client.get(endpoints::SHARES).await?;
    ctx.output(&response, |r| {
        if r.shares.is_empty() {
            println!("No shared sites. Share one with `red share <site>`.");
        } else {
            let rows: Vec<[String; 3]> = r
                .shares
                .iter()
                .map(|s| {
                    let state = match s.state {
                        ShareState::Starting => s.step.clone(),
                        ShareState::Running => "shared".into(),
                        ShareState::Failed => format!("failed: {}", s.error.as_deref().unwrap_or("?")),
                    };
                    [s.site.clone(), s.url.clone().unwrap_or_else(|| "-".into()), state]
                })
                .collect();
            print_table(&["SITE", "URL", "STATE"], &rows);
        }
        if !r.logged_in {
            println!("\nFor a hostname on your own domain: `red share:login`, then `red share <site> --hostname app.example.com`.");
        }
    })
}

pub(crate) async fn login(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let response: ShareLoginResponse = ctx
        .send(Method::POST, endpoints::SHARE_LOGIN, None::<&()>)
        .await?;
    let Some(url) = response.url else {
        println!("cloudflared is already logged in to Cloudflare.");
        return Ok(());
    };
    println!("Open this page, log in to Cloudflare and pick the domain to use:\n\n  {url}\n");
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(opener)
        .arg(&url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    for _ in 0..300 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let shares: SharesResponse = ctx.client.get(endpoints::SHARES).await?;
        if shares.logged_in {
            println!(
                "Logged in. Share with your own hostname: `red share <site> --hostname app.example.com`."
            );
            return Ok(());
        }
    }
    bail!("not logged in after 5 minutes; run `red share:login` again")
}
