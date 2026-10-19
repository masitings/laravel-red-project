//! Where Linux keeps trusted certificate authorities and DNS settings that
//! RED integrates with. Only names and paths live here; the CLI changes
//! them (with `sudo`) and the daemon's doctor reads them.

use std::path::{Path, PathBuf};

/// Nickname of RED's CA in NSS databases (Chrome, Firefox).
pub const NSS_NICKNAME: &str = "RED Project local CA";

/// File name of RED's CA in the system anchor directories.
pub const ANCHOR_FILE: &str = "red-project-ca.crt";

/// How this Linux distribution adds a trusted CA to the system store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemStore {
    /// Debian, Ubuntu: a `.crt` in `/usr/local/share/ca-certificates`, then
    /// `update-ca-certificates`.
    Debian { dir: PathBuf },
    /// Fedora, RHEL, Arch, openSUSE: p11-kit's `trust anchor --store`.
    P11Kit,
    /// Older Fedora/RHEL without `trust`: an anchor file, then
    /// `update-ca-trust extract`.
    CaTrust { dir: PathBuf },
}

impl SystemStore {
    /// The store this machine uses, given which tools are on `PATH`.
    pub fn detect(has_tool: impl Fn(&str) -> bool) -> Option<Self> {
        let debian = Path::new("/usr/local/share/ca-certificates");
        if has_tool("update-ca-certificates") && debian.is_dir() {
            return Some(Self::Debian {
                dir: debian.to_path_buf(),
            });
        }
        if has_tool("trust") {
            return Some(Self::P11Kit);
        }
        let anchors = Path::new("/etc/pki/ca-trust/source/anchors");
        if has_tool("update-ca-trust") && anchors.is_dir() {
            return Some(Self::CaTrust {
                dir: anchors.to_path_buf(),
            });
        }
        None
    }
}

/// NSS databases (`cert9.db`) of the user's browsers: Chrome/Chromium's
/// shared one and every Firefox profile (regular, Snap and Flatpak).
/// Chrome's is listed even when it does not exist yet, since `certutil`
/// can create it.
pub fn nss_databases(home: &Path) -> Vec<PathBuf> {
    let mut found = vec![home.join(".pki/nssdb")];
    for root in [
        ".mozilla/firefox",
        "snap/firefox/common/.mozilla/firefox",
        ".var/app/org.mozilla.firefox/.mozilla/firefox",
    ] {
        let Ok(entries) = std::fs::read_dir(home.join(root)) else {
            continue;
        };
        let mut profiles: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("cert9.db").is_file())
            .collect();
        profiles.sort();
        found.extend(profiles);
    }
    found
}

/// `true` when systemd-resolved is running (it owns `/run/systemd/resolve`).
pub fn systemd_resolved_running() -> bool {
    Path::new("/run/systemd/resolve").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_browser_databases() {
        let home = tempfile::tempdir().unwrap();
        let profile = home.path().join(".mozilla/firefox/abc.default-release");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("cert9.db"), b"").unwrap();
        let snap = home
            .path()
            .join("snap/firefox/common/.mozilla/firefox/x.default");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("cert9.db"), b"").unwrap();
        // A profile directory without a database is skipped.
        std::fs::create_dir_all(home.path().join(".mozilla/firefox/empty")).unwrap();

        let found = nss_databases(home.path());
        assert_eq!(found[0], home.path().join(".pki/nssdb"));
        assert!(found.contains(&profile));
        assert!(found.contains(&snap));
        assert_eq!(found.len(), 3);
    }

    #[test]
    fn prefers_debian_then_p11kit() {
        let none = SystemStore::detect(|_| false);
        assert_eq!(none, None);
        let p11 = SystemStore::detect(|tool| tool == "trust");
        assert_eq!(p11, Some(SystemStore::P11Kit));
    }
}
