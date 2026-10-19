//! Installing the privileged helper and starting redd at login.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use red_platform::services;

use crate::system::{root, root_command, run};

/// A binary installed next to the running `red`, e.g. `redd` or `red-helper`.
pub fn sibling_binary(name: &str) -> anyhow::Result<std::path::PathBuf> {
    let exe = std::env::current_exe().context("cannot locate the red binary")?;
    // `red` may be a symlink (e.g. ~/.local/bin/red → RED.app); the other
    // binaries sit next to the real file.
    let exe = exe.canonicalize().unwrap_or(exe);
    let path = exe.with_file_name(name);
    if !path.is_file() {
        bail!(
            "{} not found next to {}; build or install all RED binaries together",
            path.display(),
            exe.display()
        );
    }
    Ok(path)
}

/// Write `contents` to a root-owned file through `sudo install` (or
/// `install` when already root).
fn sudo_write(dest: &Path, contents: &str, mode: &str) -> anyhow::Result<()> {
    let tmp = std::env::temp_dir().join(format!("red-{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents).context("could not write a temporary file")?;
    let result = root(&[
        "install",
        "-m",
        mode,
        &tmp.display().to_string(),
        &dest.display().to_string(),
    ]);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// Install and start the privileged helper (asks for your password).
pub fn install_helper(uid: u32) -> anyhow::Result<()> {
    let source = sibling_binary("red-helper")?;
    let binary = services::helper_binary();
    let definition = services::helper_definition();
    let socket = red_helper_socket();
    if red_platform::user::is_root() {
        println!("Installing red-helper:");
    } else {
        println!("Installing red-helper (sudo will ask for your password):");
    }
    println!("  {} → {}", source.display(), binary.display());
    println!("  {}", definition.display());
    if let Some(dir) = binary.parent() {
        root(&["mkdir", "-p", &dir.display().to_string()])?;
    }
    root(&[
        "install",
        "-m",
        "0755",
        &source.display().to_string(),
        &binary.display().to_string(),
    ])?;
    if cfg!(target_os = "macos") {
        sudo_write(
            &definition,
            &services::helper_plist(&binary, &socket, uid),
            "0644",
        )?;
        let target = format!("system/{}", services::HELPER_LABEL);
        let _ = root_command("launchctl")
            .args(["bootout", &target])
            .stderr(Stdio::null())
            .status();
        root(&[
            "launchctl",
            "bootstrap",
            "system",
            &definition.display().to_string(),
        ])?;
    } else {
        sudo_write(
            &definition,
            &services::helper_unit(&binary, &socket, uid),
            "0644",
        )?;
        root(&["systemctl", "daemon-reload"])?;
        root(&["systemctl", "enable", "--now", "red-helper.service"])?;
        root(&["systemctl", "restart", "red-helper.service"])?;
    }
    println!("red-helper is running. Restart redd to move to ports 80 and 443.");
    Ok(())
}

/// Stop and remove the privileged helper.
pub fn uninstall_helper() -> anyhow::Result<()> {
    let binary = services::helper_binary();
    let definition = services::helper_definition();
    println!("Removing red-helper (sudo will ask for your password).");
    if cfg!(target_os = "macos") {
        let target = format!("system/{}", services::HELPER_LABEL);
        let _ = root_command("launchctl")
            .args(["bootout", &target])
            .status();
    } else {
        let _ = root_command("systemctl")
            .args(["disable", "--now", "red-helper.service"])
            .status();
    }
    root(&[
        "rm",
        "-f",
        &definition.display().to_string(),
        &binary.display().to_string(),
    ])?;
    println!("Removed. redd falls back to ports 8180 and 8443 after a restart.");
    Ok(())
}

fn red_helper_socket() -> std::path::PathBuf {
    if cfg!(target_os = "macos") {
        "/var/run/red-helper.sock".into()
    } else {
        "/run/red-helper.sock".into()
    }
}

fn home() -> anyhow::Result<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .context("HOME is not set")
}

/// Start redd at login and (re)start it now.
pub fn install_agent(log_dir: &Path) -> anyhow::Result<()> {
    let redd = sibling_binary("redd")?;
    let definition = services::agent_definition(&home()?);
    if let Some(dir) = definition.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::create_dir_all(log_dir)?;
    if cfg!(target_os = "macos") {
        let log = log_dir.join("redd.log");
        std::fs::write(&definition, services::agent_plist(&redd, &log))?;
        let uid = red_platform::user::current_uid();
        let domain = format!("gui/{uid}");
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("{domain}/{}", services::AGENT_LABEL)])
            .stderr(Stdio::null())
            .status();
        // `bootout` finishes asynchronously; bootstrapping right away can
        // fail with "Input/output error" until the old job is gone.
        let definition_arg = definition.display().to_string();
        let mut attempt = 0;
        loop {
            attempt += 1;
            let ok = Command::new("launchctl")
                .args(["bootstrap", &domain, &definition_arg])
                .stderr(if attempt < 5 {
                    Stdio::null()
                } else {
                    Stdio::inherit()
                })
                .status()
                .is_ok_and(|s| s.success());
            if ok {
                break;
            }
            if attempt >= 5 {
                bail!("launchctl could not load {definition_arg}");
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        println!("redd starts at login now (log: {}).", log.display());
    } else {
        std::fs::write(&definition, services::agent_unit(&redd))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run("systemctl", &["--user", "enable", "redd.service"])?;
        run("systemctl", &["--user", "restart", "redd.service"])?;
        println!("redd starts at login now (journalctl --user -u redd for logs).");
    }
    Ok(())
}

/// Restart redd through the login agent, installing the agent if needed.
pub fn restart_agent(log_dir: &Path) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        let uid = red_platform::user::current_uid();
        let job = format!("gui/{uid}/{}", services::AGENT_LABEL);
        let restarted = Command::new("launchctl")
            .args(["kickstart", "-k", &job])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if restarted {
            println!("Restarted redd.");
            return Ok(());
        }
    } else {
        let restarted = Command::new("systemctl")
            .args(["--user", "restart", "redd.service"])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if restarted {
            println!("Restarted redd.");
            return Ok(());
        }
    }
    install_agent(log_dir)
}

/// Stop starting redd at login, and stop it.
pub fn uninstall_agent() -> anyhow::Result<()> {
    let definition = services::agent_definition(&home()?);
    if cfg!(target_os = "macos") {
        let uid = red_platform::user::current_uid();
        let _ = Command::new("launchctl")
            .args(["bootout", &format!("gui/{uid}/{}", services::AGENT_LABEL)])
            .status();
    } else {
        let _ = Command::new("systemctl")
            .args(["--user", "disable", "--now", "redd.service"])
            .status();
    }
    match std::fs::remove_file(&definition) {
        Ok(()) => println!("redd no longer starts at login."),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("redd was not set up to start at login.");
        }
        Err(err) => return Err(err.into()),
    }
    Ok(())
}
