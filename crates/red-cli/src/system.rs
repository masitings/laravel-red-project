//! One-time system integration: trusting the local CA and installing the
//! resolver file. These run as the user and ask for elevation themselves
//! (macOS password dialog for the keychain, `sudo` for system files), so the
//! user always sees what is being changed.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use red_helper::resolver;
use red_platform::binaries::in_path;
use red_platform::trust::{ANCHOR_FILE, NSS_NICKNAME, SystemStore, nss_databases};

pub(crate) fn run(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("could not run {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed ({status})", args.join(" "));
    }
    Ok(())
}

/// Like [`run`], without the command's standard output.
fn run_quiet(program: &str, args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .status()
        .with_context(|| format!("could not run {program}"))?;
    if !status.success() {
        bail!("`{program} {}` failed ({status})", args.join(" "));
    }
    Ok(())
}

fn has_tool(name: &str) -> bool {
    !in_path(name).is_empty()
}

fn home() -> anyhow::Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// Write `contents` to a root-owned file with `sudo tee`.
fn sudo_tee(path: &Path, contents: &str) -> anyhow::Result<()> {
    let mut child = Command::new("sudo")
        .arg("tee")
        .arg(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .context("could not run sudo")?;
    child
        .stdin
        .take()
        .context("no stdin for sudo tee")?
        .write_all(contents.as_bytes())?;
    if !child.wait()?.success() {
        bail!("could not write {}", path.display());
    }
    Ok(())
}

/// Trust RED's CA: the macOS login keychain, or on Linux the system store
/// plus the NSS databases Chrome and Firefox read.
pub fn trust(ca: &Path) -> anyhow::Result<()> {
    if !ca.is_file() {
        bail!("{} does not exist; is redd running?", ca.display());
    }
    if cfg!(target_os = "macos") {
        let keychain = home()?.join("Library/Keychains/login.keychain-db");
        println!("Adding RED's local CA to your login keychain. macOS will ask for your password.");
        run(
            "security",
            &[
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-k",
                &keychain.display().to_string(),
                &ca.display().to_string(),
            ],
        )?;
        println!("Trusted. Browsers now accept certificates for secured sites.");
        println!(
            "Firefox uses its own store: enable security.enterprise_roots.enabled in about:config."
        );
        return Ok(());
    }
    trust_system_linux(ca)?;
    trust_nss(ca)?;
    Ok(())
}

fn trust_system_linux(ca: &Path) -> anyhow::Result<()> {
    let ca_arg = ca.display().to_string();
    match SystemStore::detect(has_tool) {
        Some(SystemStore::Debian { dir }) => {
            let dest = dir.join(ANCHOR_FILE);
            println!(
                "Adding RED's local CA to the system store as {} (sudo will ask for your password).",
                dest.display()
            );
            run(
                "sudo",
                &[
                    "install",
                    "-m",
                    "0644",
                    &ca_arg,
                    &dest.display().to_string(),
                ],
            )?;
            run("sudo", &["update-ca-certificates"])?;
        }
        Some(SystemStore::P11Kit) => {
            println!(
                "Adding RED's local CA to the system store with `trust anchor` (sudo will ask for your password)."
            );
            run("sudo", &["trust", "anchor", "--store", &ca_arg])?;
        }
        Some(SystemStore::CaTrust { dir }) => {
            let dest = dir.join(ANCHOR_FILE);
            println!(
                "Adding RED's local CA to the system store as {} (sudo will ask for your password).",
                dest.display()
            );
            run(
                "sudo",
                &[
                    "install",
                    "-m",
                    "0644",
                    &ca_arg,
                    &dest.display().to_string(),
                ],
            )?;
            run("sudo", &["update-ca-trust", "extract"])?;
        }
        None => {
            println!(
                "This distribution's trust store was not recognised. Add this CA to it by hand:"
            );
            println!("  {}", ca.display());
            return Ok(());
        }
    }
    println!("The system (curl, PHP, command-line tools) now trusts secured sites.");
    Ok(())
}

/// Add the CA to Chrome's and every Firefox profile's NSS database.
fn trust_nss(ca: &Path) -> anyhow::Result<()> {
    let databases = nss_databases(&home()?);
    if !has_tool("certutil") {
        println!(
            "Browsers keep their own certificate store. Install certutil and run `red trust` again:"
        );
        println!("  Debian/Ubuntu: sudo apt install libnss3-tools");
        println!("  Fedora: sudo dnf install nss-tools    Arch: sudo pacman -S nss");
        return Ok(());
    }
    for db in &databases {
        let spec = format!("sql:{}", db.display());
        if !db.join("cert9.db").is_file() {
            // Chrome's shared database may not exist yet; create it empty.
            std::fs::create_dir_all(db)?;
            run("certutil", &["-d", &spec, "-N", "--empty-password"])?;
        }
        // Replace an older copy (the CA changes when RED's data is reset).
        let _ = Command::new("certutil")
            .args(["-d", &spec, "-D", "-n", NSS_NICKNAME])
            .stderr(Stdio::null())
            .status();
        run(
            "certutil",
            &[
                "-d",
                &spec,
                "-A",
                "-t",
                "C,,",
                "-n",
                NSS_NICKNAME,
                "-i",
                &ca.display().to_string(),
            ],
        )?;
        println!("Trusted in {}.", db.display());
    }
    println!("Restart your browser so it picks up the change.");
    Ok(())
}

/// Stop trusting RED's CA.
pub fn untrust(ca: &Path) -> anyhow::Result<()> {
    if cfg!(target_os = "macos") {
        run(
            "security",
            &["remove-trusted-cert", &ca.display().to_string()],
        )?;
        println!("RED's local CA is no longer trusted.");
        return Ok(());
    }
    match SystemStore::detect(has_tool) {
        Some(SystemStore::Debian { dir }) => {
            run(
                "sudo",
                &["rm", "-f", &dir.join(ANCHOR_FILE).display().to_string()],
            )?;
            // --fresh also drops the symlinks to the removed file; it lists
            // every certificate, so keep it quiet.
            run_quiet("sudo", &["update-ca-certificates", "--fresh"])?;
        }
        Some(SystemStore::P11Kit) => {
            run(
                "sudo",
                &["trust", "anchor", "--remove", &ca.display().to_string()],
            )?;
        }
        Some(SystemStore::CaTrust { dir }) => {
            run(
                "sudo",
                &["rm", "-f", &dir.join(ANCHOR_FILE).display().to_string()],
            )?;
            run("sudo", &["update-ca-trust", "extract"])?;
        }
        None => println!(
            "Remove {} from your system trust store by hand.",
            ca.display()
        ),
    }
    if has_tool("certutil") {
        for db in nss_databases(&home()?) {
            if db.join("cert9.db").is_file() {
                let _ = Command::new("certutil")
                    .arg("-d")
                    .arg(format!("sql:{}", db.display()))
                    .args(["-D", "-n", NSS_NICKNAME])
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }
    println!("RED's local CA is no longer trusted.");
    Ok(())
}

/// Send queries for `.<tld>` to RED's DNS responder without the helper:
/// `/etc/resolver/<tld>` on macOS, a systemd-resolved drop-in on Linux.
pub fn install_resolver(tld: &str, port: u16) -> anyhow::Result<()> {
    if cfg!(target_os = "linux") && !red_platform::trust::systemd_resolved_running() {
        println!("systemd-resolved is not running, so RED cannot set up .{tld} by itself.");
        println!(
            "Send .{tld} to 127.0.0.1 port {port} in your DNS setup, for example with dnsmasq:"
        );
        println!("  server=/{tld}/127.0.0.1#{port}");
        return Ok(());
    }
    let dir = resolver::default_dir();
    let path = resolver::path_for(&dir, tld);
    let contents = resolver::contents(tld, port);
    println!(
        "Writing {} (sudo will ask for your password):",
        path.display()
    );
    print!("{contents}");
    run("sudo", &["mkdir", "-p", &dir.display().to_string()])?;
    sudo_tee(&path, &contents)?;
    reload_resolver()?;
    println!("Done. *.{tld} now resolves to this machine.");
    Ok(())
}

/// Remove the resolver file, but only if RED created it.
pub fn uninstall_resolver(tld: &str) -> anyhow::Result<()> {
    let path = resolver::path_for(&resolver::default_dir(), tld);
    match std::fs::read_to_string(&path) {
        Ok(contents) if contents.starts_with(resolver::MARKER) => {
            println!(
                "Removing {} (sudo will ask for your password).",
                path.display()
            );
            run("sudo", &["rm", &path.display().to_string()])?;
            reload_resolver()?;
            println!("Removed.");
        }
        Ok(_) => bail!(
            "{} was not created by RED; leaving it alone",
            path.display()
        ),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            println!("{} does not exist; nothing to do.", path.display());
        }
        Err(err) => return Err(err).with_context(|| format!("could not read {}", path.display())),
    }
    Ok(())
}

/// systemd-resolved reads drop-ins only when it starts; macOS watches
/// `/etc/resolver` itself.
fn reload_resolver() -> anyhow::Result<()> {
    if cfg!(target_os = "linux") {
        run("sudo", &["systemctl", "try-restart", "systemd-resolved"])?;
    }
    Ok(())
}
