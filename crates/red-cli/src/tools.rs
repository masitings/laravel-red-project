//! `red php`, `red composer`, `red node`, `red npm`, `red npx` and
//! `red env`: run the PHP and Node.js versions a project asks for.
//!
//! PHP: the site's version as redd reports it (isolation, `red.toml`,
//! default), or `php` in `red.toml` when redd is not running. Node.js:
//! `node` in `red.toml`, else `.nvmrc` / `.node-version`, else the newest
//! installation found.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, bail};
use red_api::{PhpListResponse, SiteInfo, endpoints};
use red_core::project::{ProjectConfig, node_version_file, node_version_matches};

use crate::{Ctx, cwd, site_containing};

/// A PHP CLI binary and the version it is.
#[derive(Debug)]
pub(crate) struct Php {
    pub binary: PathBuf,
    pub version: Option<String>,
}

/// The PHP CLI to use in the current directory.
pub(crate) async fn resolve_php(ctx: &Ctx<'_>) -> anyhow::Result<Php> {
    let dir = cwd()?;
    let project = ProjectConfig::find(&dir)?.map(|(_, c)| c);
    let mut wanted = project
        .and_then(|c| c.php)
        .and_then(|v| red_core::php::minor_version(&v));
    let mut installs: Vec<(String, PathBuf)> = Vec::new();

    // redd knows isolation and the default version, and where PHP lives.
    if let Ok(list) = ctx.client.get::<PhpListResponse>(endpoints::PHP).await {
        if let Ok(sites) = ctx.client.get::<Vec<SiteInfo>>(endpoints::SITES).await
            && let Some(site) = site_containing(&sites, &dir)
            && site.php.is_some()
        {
            wanted = site.php.clone();
        }
        if wanted.is_none() {
            wanted = list.default.clone();
        }
        for install in list.installs {
            if let Some(cli) = red_platform::binaries::php_cli_for_fpm(Path::new(&install.fpm_path))
            {
                installs.push((install.version, cli));
            }
        }
    } else {
        let data = red_platform::Paths::discover()?.data_dir;
        for fpm in red_platform::php::fpm_candidates(&data) {
            if let (Some(cli), Some(version)) = (
                red_platform::binaries::php_cli_for_fpm(&fpm),
                php_version(&fpm),
            ) && !installs.iter().any(|(v, _)| *v == version)
            {
                installs.push((version, cli));
            }
        }
    }

    if let Some(version) = &wanted {
        if let Some((_, cli)) = installs.iter().find(|(v, _)| v == version) {
            return Ok(Php {
                binary: cli.clone(),
                version: Some(version.clone()),
            });
        }
        let installed: Vec<&str> = installs.iter().map(|(v, _)| v.as_str()).collect();
        bail!(
            "PHP {version} is not installed (installed: {})",
            if installed.is_empty() {
                "none".to_owned()
            } else {
                installed.join(", ")
            }
        );
    }
    if let Some((version, cli)) = installs.first() {
        return Ok(Php {
            binary: cli.clone(),
            version: Some(version.clone()),
        });
    }
    let binary = red_platform::binaries::in_path("php")
        .into_iter()
        .next()
        .context("no PHP found; install one, e.g. `brew install php`")?;
    Ok(Php {
        binary,
        version: None,
    })
}

fn php_version(fpm: &Path) -> Option<String> {
    let output = Command::new(fpm).arg("-v").output().ok()?;
    let (version, _) =
        red_core::php::parse_version_output(&String::from_utf8_lossy(&output.stdout))?;
    Some(version)
}

/// The Node.js version the current project asks for, if any.
fn wanted_node() -> anyhow::Result<Option<String>> {
    let dir = cwd()?;
    Ok(match ProjectConfig::find(&dir)? {
        Some((_, config)) if config.node.is_some() => config.node,
        Some((root, _)) => node_version_file(&root),
        None => dir.ancestors().find_map(node_version_file),
    })
}

/// The installed Node.js matching `wanted` (or the newest), or why none does.
fn find_node(wanted: Option<&str>) -> Result<PathBuf, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let data = red_platform::Paths::discover()
        .map(|p| p.data_dir)
        .unwrap_or_default();
    let mut found: Vec<(String, PathBuf)> = red_platform::binaries::node_candidates(&home, &data)
        .into_iter()
        .filter_map(|node| {
            let output = Command::new(&node).arg("--version").output().ok()?;
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!version.is_empty()).then_some((version, node))
        })
        .collect();
    found.sort_by(|(a, _), (b, _)| compare_versions(b, a));
    match wanted {
        Some(wanted) => found
            .iter()
            .find(|(v, _)| node_version_matches(v, wanted))
            .map(|(_, node)| node.clone())
            .ok_or_else(|| {
                let installed: Vec<&str> = found.iter().map(|(v, _)| v.as_str()).collect();
                format!(
                    "Node.js {wanted} is not installed (found: {})",
                    if installed.is_empty() {
                        "none".to_owned()
                    } else {
                        installed.join(", ")
                    }
                )
            }),
        None => found
            .into_iter()
            .next()
            .map(|(_, node)| node)
            .ok_or_else(|| "no Node.js found".to_owned()),
    }
}

/// The Node.js binary to use in the current directory. When it is missing
/// and redd runs, RED downloads it.
pub(crate) async fn resolve_node(ctx: &Ctx<'_>) -> anyhow::Result<PathBuf> {
    let wanted = wanted_node()?;
    match find_node(wanted.as_deref()) {
        Ok(node) => Ok(node),
        Err(missing) => {
            if ctx.client.status().await.is_err() {
                bail!("{missing}; start redd so RED can install it, or use nvm/fnm/Homebrew");
            }
            eprintln!("{missing}; installing it with RED.");
            crate::runtimes::install(
                ctx,
                red_api::RuntimeKind::Node,
                wanted.as_deref().unwrap_or(""),
            )
            .await?;
            find_node(wanted.as_deref()).map_err(anyhow::Error::msg)
        }
    }
}

/// Composer: RED's own copy, else one on `PATH`, else RED downloads it.
async fn resolve_composer(ctx: &Ctx<'_>) -> anyhow::Result<PathBuf> {
    let data = red_platform::Paths::discover()?.data_dir;
    if let Some(composer) = red_platform::binaries::red_composer(&data) {
        return Ok(composer);
    }
    if let Some(composer) = red_platform::binaries::in_path("composer")
        .into_iter()
        .chain(red_platform::binaries::in_path("composer.phar"))
        .next()
    {
        return Ok(composer);
    }
    ctx.client
        .status()
        .await
        .context("Composer is not installed; start redd so RED can install it")?;
    eprintln!("Composer is not installed; installing it with RED.");
    crate::runtimes::install_quietly(ctx, red_api::RuntimeKind::Composer, "").await?;
    red_platform::binaries::red_composer(&data).context("Composer did not install")
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parts(a).cmp(&parts(b))
}

/// Replace this process with `program args`, with `extra_path` first on
/// `PATH` so scripts started by it find the same PHP or Node.js.
fn exec(program: &Path, args: &[OsString], extra_path: &[&Path]) -> anyhow::Result<()> {
    use std::os::unix::process::CommandExt;
    let mut dirs: Vec<PathBuf> = extra_path.iter().map(|p| p.to_path_buf()).collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let err = Command::new(program)
        .args(args)
        .env("PATH", std::env::join_paths(dirs)?)
        .exec();
    Err(anyhow::Error::new(err).context(format!("cannot run {}", program.display())))
}

fn parent(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new("/"))
}

/// `-c <php.ini>` for PHP builds installed by RED, which have no ini file
/// of their own.
fn ini_args(php: &Path) -> Vec<OsString> {
    match red_platform::binaries::php_ini_for(php) {
        Some(ini) => vec!["-c".into(), ini.into_os_string()],
        None => Vec::new(),
    }
}

pub(crate) async fn php(ctx: &Ctx<'_>, args: &[OsString]) -> anyhow::Result<()> {
    let php = resolve_php(ctx).await?;
    let mut full = ini_args(&php.binary);
    full.extend_from_slice(args);
    exec(&php.binary, &full, &[parent(&php.binary)])
}

pub(crate) async fn composer(ctx: &Ctx<'_>, args: &[OsString]) -> anyhow::Result<()> {
    let php = resolve_php(ctx).await?;
    let composer = resolve_composer(ctx).await?;
    let mut full = ini_args(&php.binary);
    full.push(composer.into_os_string());
    full.extend_from_slice(args);
    exec(&php.binary, &full, &[parent(&php.binary)])
}

/// `node`, or `npm`/`npx` from the same installation.
pub(crate) async fn node(ctx: &Ctx<'_>, tool: &str, args: &[OsString]) -> anyhow::Result<()> {
    let node = resolve_node(ctx).await?;
    let bin = parent(&node);
    let program = if tool == "node" {
        node.clone()
    } else {
        let sibling = bin.join(tool);
        if !sibling.is_file() {
            bail!("{tool} was not found next to {}", node.display());
        }
        sibling
    };
    exec(&program, args, &[bin])
}

/// Shell commands that put the project's PHP and Node.js first on `PATH`.
pub(crate) async fn env(ctx: &Ctx<'_>) -> anyhow::Result<()> {
    let mut dirs = Vec::new();
    let mut notes = Vec::new();
    match resolve_php(ctx).await {
        Ok(php) => {
            notes.push(format!(
                "# PHP {}: {}",
                php.version.as_deref().unwrap_or("?"),
                php.binary.display()
            ));
            dirs.push(parent(&php.binary).to_path_buf());
        }
        Err(err) => notes.push(format!("# PHP: {err}")),
    }
    match wanted_node()
        .map_err(|e| e.to_string())
        .and_then(|w| find_node(w.as_deref()))
    {
        Ok(node) => {
            notes.push(format!("# Node.js: {}", node.display()));
            dirs.push(parent(&node).to_path_buf());
        }
        Err(err) => notes.push(format!("# Node.js: {err}")),
    }
    if let Ok(paths) = red_platform::Paths::discover()
        && let Some(composer) = red_platform::binaries::red_composer(&paths.data_dir)
    {
        notes.push(format!("# Composer: {}", composer.display()));
        dirs.push(parent(&composer).to_path_buf());
    }
    for note in notes {
        println!("{note}");
    }
    if !dirs.is_empty() {
        let joined: Vec<String> = dirs
            .iter()
            .map(|d| shell_quote(&d.display().to_string()))
            .collect();
        println!("export PATH={}:\"$PATH\"", joined.join(":"));
    }
    Ok(())
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_node_versions_numerically() {
        let mut versions = vec!["v9.0.0", "v22.1.0", "v18.20.0"];
        versions.sort_by(|a, b| compare_versions(b, a));
        assert_eq!(versions, ["v22.1.0", "v18.20.0", "v9.0.0"]);
    }

    #[test]
    fn quotes_for_the_shell() {
        assert_eq!(shell_quote("/a b/it's"), "'/a b/it'\\''s'");
    }
}
