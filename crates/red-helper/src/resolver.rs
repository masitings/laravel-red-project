//! Resolver files that send `.<tld>` lookups to redd's DNS responder.

use std::path::{Path, PathBuf};

/// First line of every file the helper writes, so it only ever removes its own.
pub const MARKER: &str = "# Added by RED Project.";

/// Where resolver configuration lives on this OS.
pub fn default_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        "/etc/resolver".into()
    } else {
        "/etc/systemd/resolved.conf.d".into()
    }
}

fn file_for(dir: &Path, tld: &str) -> PathBuf {
    if cfg!(target_os = "macos") {
        dir.join(tld)
    } else {
        dir.join(format!("red-{tld}.conf"))
    }
}

/// The resolver file contents for this OS.
pub fn contents(tld: &str, port: u16) -> String {
    if cfg!(target_os = "macos") {
        format!("{MARKER} Remove with `red dns:uninstall`.\nnameserver 127.0.0.1\nport {port}\n")
    } else {
        format!(
            "{MARKER} Remove with `red dns:uninstall`.\n[Resolve]\nDNS=127.0.0.1:{port}\nDomains=~{tld}\n"
        )
    }
}

/// Write the resolver file for `tld`. The caller has validated both values.
pub fn install(dir: &Path, tld: &str, port: u16) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = file_for(dir, tld);
    std::fs::write(&path, contents(tld, port))?;
    Ok(path)
}

/// Remove the resolver file for `tld` if RED wrote it. Returns whether a
/// file was removed.
pub fn remove(dir: &Path, tld: &str) -> std::io::Result<bool> {
    let path = file_for(dir, tld);
    match std::fs::read_to_string(&path) {
        Ok(text) if text.starts_with(MARKER) => {
            std::fs::remove_file(&path)?;
            Ok(true)
        }
        Ok(_) => Err(std::io::Error::other(format!(
            "{} was not written by RED",
            path.display()
        ))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_and_removes_only_its_own_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = install(dir.path(), "test", 53530).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(MARKER));
        assert!(text.contains("53530"));
        assert!(remove(dir.path(), "test").unwrap());
        assert!(!remove(dir.path(), "test").unwrap());

        std::fs::write(&path, "nameserver 1.1.1.1\n").unwrap();
        assert!(remove(dir.path(), "test").is_err());
        assert!(path.exists());
    }
}
