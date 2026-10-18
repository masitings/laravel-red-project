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
}
