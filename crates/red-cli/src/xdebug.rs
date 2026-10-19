//! `red xdebug`: when sites and `red php` run with Xdebug.

use red_api::client::Method;
use red_api::{PhpListResponse, XdebugMode, XdebugRequest, endpoints};

use crate::{Ctx, print_table};

pub(crate) async fn status(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let list: PhpListResponse = ctx.client.get(endpoints::PHP).await?;
    ctx.output(&list, print_status)
}

pub(crate) async fn set(ctx: &Ctx<'_>, mode: XdebugMode) -> anyhow::Result<()> {
    let list: PhpListResponse = ctx
        .send(
            Method::PUT,
            endpoints::XDEBUG,
            Some(&XdebugRequest { mode }),
        )
        .await?;
    ctx.output(&list, print_status)
}

fn mode_text(mode: XdebugMode) -> &'static str {
    match mode {
        XdebugMode::Off => "off",
        XdebugMode::Auto => {
            "auto: requests with the XDEBUG_SESSION cookie (or XDEBUG_SESSION_START / \
             XDEBUG_TRIGGER in the URL), and `red php` with XDEBUG_TRIGGER=1"
        }
        XdebugMode::On => "on: every request and every `red php` run",
    }
}

fn print_status(list: &PhpListResponse) {
    println!("Xdebug is {}.", mode_text(list.xdebug));
    if list.xdebug != XdebugMode::Off {
        println!("Your IDE listens on port {} (127.0.0.1).", list.xdebug_port);
    }
    let rows: Vec<[String; 2]> = list
        .installs
        .iter()
        .map(|i| {
            let state = if i.xdebug_loaded {
                "loaded by its php.ini (always on)".to_owned()
            } else if let Some(path) = &i.xdebug {
                format!("available ({path})")
            } else {
                "not available".to_owned()
            };
            [format!("PHP {}", i.version), state]
        })
        .collect();
    if !rows.is_empty() {
        println!();
        print_table(&["VERSION", "XDEBUG"], &rows);
    }
    if list
        .installs
        .iter()
        .any(|i| i.xdebug.is_none() && !i.xdebug_loaded)
    {
        println!(
            "\nRED's macOS PHP builds include Xdebug (`red runtimes:update php`); elsewhere, \
             install your distribution's php-xdebug."
        );
    }
}
