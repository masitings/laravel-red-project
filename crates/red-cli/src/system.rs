//! One-time system integration: trusting the local CA and installing the
//! resolver file. These run as the user and ask for elevation themselves
//! (macOS password dialog for the keychain, `sudo` for `/etc/resolver`), so
//! the user always sees what is being changed.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

/// Marker written into files RED creates outside its own directories.
const MARKER: &str = "# Added by RED Project.";

fn run(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("could not run {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed ({status})", args.join(" "));
    }
    Ok(())
}

/// Add the CA to the user's login keychain as a trusted root (macOS).
pub fn trust(ca: &Path) -> anyhow::Result<()> {
    if !ca.is_file() {
        bail!("{} does not exist; is redd running?", ca.display());
    }
    if cfg!(target_os = "macos") {
        let home = std::env::var("HOME").context("HOME is not set")?;
        let keychain = format!("{home}/Library/Keychains/login.keychain-db");
        println!("Adding RED's local CA to your login keychain. macOS will ask for your password.");
        run(
            "security",
            &[
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-k",
                &keychain,
                &ca.display().to_string(),
            ],
        )?;
        println!("Trusted. Browsers now accept certificates for secured sites.");
        println!(
            "Firefox uses its own store: enable security.enterprise_roots.enabled in about:config."
        );
    } else {
        println!("Automatic trust is not implemented on this OS yet. Add this CA manually:");
        println!("  {}", ca.display());
        println!(
            "Debian/Ubuntu: sudo cp {} /usr/local/share/ca-certificates/red-project.crt && sudo update-ca-certificates",
            ca.display()
        );
    }
    Ok(())
}

/// Remove the CA's trust settings and certificate from the keychain (macOS).
pub fn untrust(ca: &Path) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        run(
            "security",
            &["remove-trusted-cert", &ca.display().to_string()],
        )?;
        println!("RED's local CA is no longer trusted.");
    } else {
        println!(
            "Remove {} from your system trust store manually.",
            ca.display()
        );
    }
    Ok(())
}

fn resolver_contents(port: u16) -> String {
    format!("{MARKER} Remove with `red dns:uninstall`.\nnameserver 127.0.0.1\nport {port}\n")
}

/// Send queries for `.<tld>` to RED's DNS responder.
pub fn install_resolver(tld: &str, port: u16) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        let path = format!("/etc/resolver/{tld}");
        println!("Writing {path} (sudo will ask for your password):");
        print!("{}", resolver_contents(port));
        run("sudo", &["mkdir", "-p", "/etc/resolver"])?;
        let mut child = Command::new("sudo")
            .args(["tee", &path])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .context("could not run sudo")?;
        child
            .stdin
            .take()
            .context("no stdin for sudo tee")?
            .write_all(resolver_contents(port).as_bytes())?;
        if !child.wait()?.success() {
            bail!("could not write {path}");
        }
        println!("Done. *.{tld} now resolves to this machine.");
    } else {
        println!("Automatic DNS setup is not implemented on this OS yet.");
        println!("With systemd-resolved, create /etc/systemd/resolved.conf.d/red.conf containing:");
        println!("  [Resolve]\n  DNS=127.0.0.1:{port}\n  Domains=~{tld}");
        println!("then run: sudo systemctl restart systemd-resolved");
    }
    Ok(())
}

/// Remove the resolver file, but only if RED created it.
pub fn uninstall_resolver(tld: &str) -> anyhow::Result<()> {
    if !cfg!(target_os = "macos") {
        println!("Remove /etc/systemd/resolved.conf.d/red.conf if you created it.");
        return Ok(());
    }
    let path = format!("/etc/resolver/{tld}");
    match std::fs::read_to_string(&path) {
        Ok(contents) if contents.starts_with(MARKER) => {
            println!("Removing {path} (sudo will ask for your password).");
            run("sudo", &["rm", &path])?;
            println!("Removed.");
        }
        Ok(_) => bail!("{path} was not created by RED; leaving it alone"),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("{path} does not exist; nothing to do.");
        }
        Err(err) => return Err(err).with_context(|| format!("could not read {path}")),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolver_file_names_port_and_marker() {
        let contents = resolver_contents(53530);
        assert!(contents.starts_with(MARKER));
        assert!(contents.contains("nameserver 127.0.0.1\n"));
        assert!(contents.contains("port 53530\n"));
    }
}
