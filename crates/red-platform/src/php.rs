//! Where to look for `php-fpm` binaries on this machine.

use std::path::{Path, PathBuf};

/// Colon-separated list of extra `php-fpm` binaries to consider first.
/// Useful for development builds and tests.
pub const RED_PHP_FPM_ENV: &str = "RED_PHP_FPM";

/// Candidate `php-fpm` binaries, most preferred first. The caller runs each
/// one with `-v` to learn its version; non-existent paths are already
/// filtered out.
///
/// Order: `RED_PHP_FPM`, RED's own runtimes (`<data>/runtimes/php/*`),
/// Homebrew (`/opt/homebrew` and `/usr/local`), then the system packages.
pub fn fpm_candidates(data_dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();

    if let Some(list) = std::env::var_os(RED_PHP_FPM_ENV) {
        found.extend(std::env::split_paths(&list));
    }

    for dir in sorted_dirs(&data_dir.join("runtimes").join("php")) {
        found.push(dir.join("sbin").join("php-fpm"));
    }

    for prefix in ["/opt/homebrew/opt", "/usr/local/opt"] {
        for dir in sorted_dirs(Path::new(prefix)) {
            let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name == "php" || name.starts_with("php@") {
                found.push(dir.join("sbin").join("php-fpm"));
            }
        }
    }

    // Debian/Ubuntu install `php-fpm8.3`; Fedora/Arch install `php-fpm`.
    for dir in ["/usr/sbin", "/usr/bin", "/usr/local/sbin"] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut bins: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
                    n == "php-fpm"
                        || n.starts_with("php-fpm")
                            && n[7..].starts_with(|c: char| c.is_ascii_digit())
                })
            })
            .collect();
        bins.sort();
        found.extend(bins);
    }

    let mut seen = Vec::new();
    found.retain(|p| {
        let keep = p.is_file() && !seen.contains(p);
        if keep {
            seen.push(p.clone());
        }
        keep
    });
    found
}

/// Where Xdebug is for a PHP installation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Xdebug {
    /// `xdebug.so` that can be loaded with `-d zend_extension=...`.
    pub extension: Option<PathBuf>,
    /// Xdebug is already loaded by the installation's own `php.ini`
    /// (`pecl install xdebug`, `php-xdebug` packages), so it always runs.
    pub loaded_by_ini: bool,
}

/// Find Xdebug for the PHP whose CLI binary is `php`. RED's own builds keep
/// it at `<root>/lib/xdebug.so`; other installations are asked for their
/// `extension_dir`.
pub fn xdebug_for(php: &Path) -> Xdebug {
    if crate::binaries::php_ini_for(php).is_some() {
        let extension = php
            .parent()
            .and_then(Path::parent)
            .map(|root| root.join("lib").join("xdebug.so"))
            .filter(|p| p.is_file());
        return Xdebug {
            extension,
            loaded_by_ini: false,
        };
    }
    let Ok(output) = std::process::Command::new(php)
        .args([
            "-r",
            "echo ini_get('extension_dir'), PHP_EOL, extension_loaded('xdebug') ? 1 : 0, PHP_EOL;",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
    else {
        return Xdebug::default();
    };
    parse_probe(&String::from_utf8_lossy(&output.stdout))
}

fn parse_probe(output: &str) -> Xdebug {
    let mut lines = output.lines();
    let extension_dir = lines.next().unwrap_or_default().trim();
    let loaded_by_ini = lines.next().is_some_and(|l| l.trim() == "1");
    let extension = (!extension_dir.is_empty())
        .then(|| Path::new(extension_dir).join("xdebug.so"))
        .filter(|p| p.is_file());
    Xdebug {
        extension,
        loaded_by_ini,
    }
}

fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_red_runtimes() {
        let data = tempfile::tempdir().unwrap();
        let bin = data.path().join("runtimes/php/8.4/sbin/php-fpm");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, "").unwrap();
        // A runtime directory without a binary is skipped.
        std::fs::create_dir_all(data.path().join("runtimes/php/8.2")).unwrap();

        let found = fpm_candidates(data.path());
        assert!(found.contains(&bin));
        assert!(
            !found
                .iter()
                .any(|p| p.starts_with(data.path().join("runtimes/php/8.2")))
        );
    }

    #[test]
    fn finds_xdebug_next_to_reds_php() {
        let root = tempfile::tempdir().unwrap();
        for dir in ["bin", "etc", "lib"] {
            std::fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        let php = root.path().join("bin/php");
        std::fs::write(&php, "").unwrap();
        std::fs::write(root.path().join("etc/php.ini"), "").unwrap();
        assert_eq!(xdebug_for(&php).extension, None);
        std::fs::write(root.path().join("lib/xdebug.so"), "").unwrap();
        let found = xdebug_for(&php);
        assert_eq!(found.extension, Some(root.path().join("lib/xdebug.so")));
        assert!(!found.loaded_by_ini);
    }

    #[test]
    fn reads_extension_dir_probes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("xdebug.so"), "").unwrap();
        let probe = parse_probe(&format!("{}\n1\n", dir.path().display()));
        assert_eq!(probe.extension, Some(dir.path().join("xdebug.so")));
        assert!(probe.loaded_by_ini);
        assert_eq!(parse_probe("/nowhere\n0\n"), Xdebug::default());
        assert_eq!(parse_probe(""), Xdebug::default());
    }
}
