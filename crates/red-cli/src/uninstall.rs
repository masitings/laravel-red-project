//! `red uninstall`: undo everything `red setup` (and the app) changed on
//! this machine, optionally with RED's data. Project folders are never
//! touched.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::bail;
use red_platform::Paths;
use red_platform::services::{self, APPIMAGE_MARKER};

use crate::{Ctx, install, system};

const BINARIES: [&str; 3] = ["red", "redd", "red-helper"];

pub(crate) async fn run(
    ctx: &Ctx<'_>,
    paths: &Paths,
    purge: bool,
    yes: bool,
) -> anyhow::Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    let targets = purge_targets(paths, &home);

    println!("This removes RED from this machine:");
    println!("  - DNS for the local TLD (the resolver file)");
    println!("  - trust in RED's local certificate authority");
    println!("  - starting redd at login (stops redd and its services)");
    println!("  - the privileged helper");
    println!("  - the red, redd and red-helper commands in ~/.local/bin");
    if purge {
        println!("  - ALL of RED's data: runtimes, databases, mail, dumps, certificates, logs:");
        for dir in &targets {
            println!("      {}", dir.display());
        }
    } else {
        println!("RED's data (runtimes, databases, mail) is kept; add --purge to delete it.");
    }
    println!("Your project folders are not touched.");
    if !yes && !confirm()? {
        println!("Nothing changed.");
        return Ok(());
    }

    let tld = red_core::config::Config::load(&paths.config_file())
        .map(|c| c.tld)
        .unwrap_or_else(|_| red_core::config::DEFAULT_TLD.to_owned());
    let ca = paths.data_dir.join("certs").join("ca.pem");
    // A redd started by hand or by the app is not under the login agent.
    let daemon_pid = ctx.client.status().await.ok().map(|s| s.pid);
    let mut problems = Vec::new();

    step(&mut problems, "DNS", async {
        // Through the helper while redd still runs; with sudo otherwise.
        match ctx.dns(false).await {
            Ok(()) => Ok(()),
            Err(_) => system::uninstall_resolver(&tld),
        }
    })
    .await;
    step(&mut problems, "CA trust", async {
        if ca.is_file() {
            system::untrust(&ca)
        } else {
            println!("No local CA was created; nothing to untrust.");
            Ok(())
        }
    })
    .await;
    step(&mut problems, "Start at login", async {
        install::uninstall_agent()?;
        stop_daemon(paths, daemon_pid);
        Ok(())
    })
    .await;
    step(&mut problems, "Privileged helper", async {
        if services::helper_definition().exists() || services::helper_binary().exists() {
            install::uninstall_helper()
        } else {
            println!("The helper is not installed.");
            Ok(())
        }
    })
    .await;
    if purge {
        step(&mut problems, "Data", async {
            wait_for_daemon_exit(paths);
            for dir in &targets {
                std::fs::remove_dir_all(dir)
                    .map_err(|e| anyhow::anyhow!("cannot delete {}: {e}", dir.display()))?;
                println!("Deleted {}.", dir.display());
            }
            // ~/.red holds nothing but bin/; drop it once empty.
            let _ = std::fs::remove_dir(home.join(".red"));
            Ok(())
        })
        .await;
    }
    let bin_dir = home.join(".local/bin");
    step(&mut problems, "Commands", async {
        remove_commands(&bin_dir)
    })
    .await;

    println!();
    if problems.is_empty() {
        println!("RED is uninstalled.");
    } else {
        println!("RED is uninstalled, except for:");
        for (label, err) in &problems {
            println!("  - {label}: {err:#}");
        }
    }
    let exe = std::env::current_exe().ok();
    if let Some(hint) = leftover_hint(exe.as_deref(), &bin_dir) {
        println!("{hint}");
    }
    if !purge {
        println!("RED's data is still in {}.", paths.data_dir.display());
    }
    Ok(())
}

async fn step(
    problems: &mut Vec<(&'static str, anyhow::Error)>,
    label: &'static str,
    action: impl std::future::Future<Output = anyhow::Result<()>>,
) {
    println!("\n{label}");
    if let Err(err) = action.await {
        eprintln!("  {err:#}");
        problems.push((label, err));
    }
}

fn confirm() -> anyhow::Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("not asking without a terminal; pass --yes to uninstall");
    }
    print!("Continue? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// Stop redd when removing the login agent did not (it was started by hand
/// or by the app). It shuts its services down on the way out.
fn stop_daemon(paths: &Paths, pid: Option<u32>) {
    let socket = paths.socket();
    for _ in 0..10 {
        if !socket.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    if let Some(pid) = pid
        && red_platform::process::interrupt(pid)
    {
        wait_for_daemon_exit(paths);
        println!("Stopped redd (pid {pid}).");
    }
}

/// Give redd a moment to stop (the login agent was just removed) so its
/// files are not deleted under it.
fn wait_for_daemon_exit(paths: &Paths) {
    let socket = paths.socket();
    for _ in 0..50 {
        if !socket.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

/// RED's own directories, outermost first, without nested duplicates. A
/// directory that is the home directory, an ancestor of it, or too short a
/// path is never included, whatever `RED_HOME` says.
pub fn purge_targets(paths: &Paths, home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        &paths.config_dir,
        &paths.data_dir,
        &paths.run_dir,
        &paths.log_dir,
        &paths.bin_dir,
    ]
    .into_iter()
    .filter(|dir| dir.components().count() >= 3)
    .filter(|dir| !home.starts_with(dir))
    .cloned()
    .collect();
    dirs.sort();
    dirs.dedup();
    let mut outermost: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        if !outermost.iter().any(|kept| dir.starts_with(kept)) {
            outermost.push(dir);
        }
    }
    outermost.retain(|dir| dir.exists());
    outermost
}

/// Remove RED's commands from `bin_dir`: symlinks into the app, copies the
/// AppImage made, or binaries `just install` put there. Other programs that
/// happen to be called `red` are left alone.
fn remove_commands(bin_dir: &Path) -> anyhow::Result<()> {
    let mut removed = Vec::new();
    for name in BINARIES {
        let path = bin_dir.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        let ours = meta.file_type().is_symlink() || is_red_binary(&path, name);
        if ours {
            std::fs::remove_file(&path)?;
            removed.push(path.display().to_string());
        }
    }
    let _ = std::fs::remove_file(bin_dir.join(APPIMAGE_MARKER));
    if removed.is_empty() {
        println!("No RED commands in {}.", bin_dir.display());
    } else {
        println!("Removed {}.", removed.join(", "));
    }
    Ok(())
}

/// `true` when `path --version` says it is RED's `name` ("red 0.3.0").
fn is_red_binary(path: &Path, name: &str) -> bool {
    Command::new(path)
        .arg("--version")
        .output()
        .ok()
        .is_some_and(|out| String::from_utf8_lossy(&out.stdout).starts_with(&format!("{name} ")))
}

/// What the user still has to remove themselves, judged by where this `red`
/// runs from.
fn leftover_hint(exe: Option<&Path>, bin_dir: &Path) -> Option<String> {
    let exe = exe?;
    let real = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let text = real.display().to_string();
    if text.contains(".app/Contents/MacOS/") {
        Some("Move RED.app to the Trash to finish (or `brew uninstall --cask red`).".into())
    } else if text.starts_with("/opt/homebrew/")
        || text.contains("/Cellar/")
        || text.contains("linuxbrew")
    {
        Some("Finish with `brew uninstall red`.".into())
    } else if text.starts_with("/usr/bin/") {
        Some(
            "Finish by removing the package: `sudo apt remove red` (or your package manager)."
                .into(),
        )
    } else if real.parent() == Some(bin_dir) || exe.parent() == Some(bin_dir) {
        None
    } else {
        Some(format!(
            "The command-line tools in {} were left in place; delete them to finish.",
            real.parent().unwrap_or(Path::new("/")).display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purges_only_reds_own_directories() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join("red-home");
        let paths = Paths::under(&root);
        for dir in [&paths.config_dir, &paths.data_dir, &paths.log_dir] {
            std::fs::create_dir_all(dir).unwrap();
        }
        let targets = purge_targets(&paths, home.path());
        assert_eq!(targets.len(), 3, "{targets:?}");
        assert!(targets.iter().all(|t| t.starts_with(&root)));

        // macOS layout: data lives inside the config directory.
        let app = home.path().join("Library/Application Support/RED");
        let nested = Paths {
            config_dir: app.clone(),
            data_dir: app.join("data"),
            run_dir: app.join("run"),
            log_dir: home.path().join("Library/Logs/RED"),
            bin_dir: home.path().join(".red/bin"),
        };
        std::fs::create_dir_all(&nested.data_dir).unwrap();
        std::fs::create_dir_all(&nested.log_dir).unwrap();
        let targets = purge_targets(&nested, home.path());
        assert_eq!(targets, vec![app, nested.log_dir.clone()]);

        // RED_HOME pointing at the home directory (or above) is refused.
        let dangerous = Paths {
            config_dir: home.path().to_path_buf(),
            data_dir: PathBuf::from("/"),
            run_dir: PathBuf::from("/tmp"),
            log_dir: home.path().parent().unwrap().to_path_buf(),
            bin_dir: home.path().to_path_buf(),
        };
        assert!(purge_targets(&dangerous, home.path()).is_empty());
    }

    #[test]
    fn removes_symlinks_and_markers_but_not_strangers() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path();
        std::os::unix::fs::symlink("/Applications/RED.app/Contents/MacOS/red", bin.join("red"))
            .unwrap();
        // Another program called redd: not RED's, so it stays.
        std::fs::write(bin.join("redd"), "#!/bin/sh\necho something else\n").unwrap();
        std::fs::set_permissions(
            bin.join("redd"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        std::fs::write(bin.join(APPIMAGE_MARKER), "").unwrap();
        remove_commands(bin).unwrap();
        assert!(std::fs::symlink_metadata(bin.join("red")).is_err());
        assert!(bin.join("redd").exists());
        assert!(!bin.join(APPIMAGE_MARKER).exists());
    }

    #[test]
    fn tells_what_is_left() {
        let bin = Path::new("/home/me/.local/bin");
        assert!(
            leftover_hint(
                Some(Path::new("/Applications/RED.app/Contents/MacOS/red")),
                bin
            )
            .unwrap()
            .contains("Trash")
        );
        assert!(
            leftover_hint(Some(Path::new("/opt/homebrew/bin/red")), bin)
                .unwrap()
                .contains("brew uninstall")
        );
        assert!(leftover_hint(Some(Path::new("/home/me/.local/bin/red")), bin).is_none());
        assert!(
            leftover_hint(Some(Path::new("/srv/tools/red")), bin)
                .unwrap()
                .contains("/srv/tools")
        );
    }
}
