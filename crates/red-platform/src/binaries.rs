//! Where to look for service servers (MySQL, PostgreSQL, Redis), PHP CLI
//! binaries and Node.js installations on this machine.
//!
//! Like [`crate::php::fpm_candidates`], these functions only list paths that
//! exist; callers run them with `--version` to learn what they are.

use std::path::{Path, PathBuf};

/// Kind of service server, mirroring `red_core::ServiceKind` without
/// depending on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Server {
    Mysql,
    Postgres,
    Redis,
}

impl Server {
    /// Executable names, most preferred first.
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Mysql => &["mysqld", "mariadbd"],
            Self::Postgres => &["postgres"],
            Self::Redis => &["valkey-server", "redis-server"],
        }
    }

    /// Homebrew formula names (exact, or followed by `@<version>`).
    fn formulae(self) -> &'static [&'static str] {
        match self {
            Self::Mysql => &["mysql", "mariadb"],
            Self::Postgres => &["postgresql"],
            Self::Redis => &["valkey", "redis"],
        }
    }
}

/// Homebrew prefixes on Apple Silicon and Intel.
const BREW_OPT: [&str; 2] = ["/opt/homebrew/opt", "/usr/local/opt"];

/// Candidate server binaries for `server`, most preferred first:
/// Homebrew formulae, Postgres.app, the Oracle MySQL package, distribution
/// packages, then `PATH`.
pub fn server_candidates(server: Server) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for opt in BREW_OPT {
        for dir in sorted_dirs(Path::new(opt)).into_iter().rev() {
            let name = file_name(&dir);
            let formula = name.split('@').next().unwrap_or_default();
            if server.formulae().contains(&formula) {
                for exe in server.names() {
                    found.push(dir.join("bin").join(exe));
                }
            }
        }
    }
    match server {
        Server::Postgres => {
            let app = Path::new("/Applications/Postgres.app/Contents/Versions");
            for dir in sorted_dirs(app).into_iter().rev() {
                found.push(dir.join("bin/postgres"));
            }
            for dir in sorted_dirs(Path::new("/usr/lib/postgresql"))
                .into_iter()
                .rev()
            {
                found.push(dir.join("bin/postgres"));
            }
        }
        Server::Mysql => found.push(PathBuf::from("/usr/local/mysql/bin/mysqld")),
        Server::Redis => {}
    }
    for dir in ["/usr/sbin", "/usr/bin", "/usr/local/sbin", "/usr/local/bin"] {
        for exe in server.names() {
            found.push(Path::new(dir).join(exe));
        }
    }
    for exe in server.names() {
        found.extend(in_path(exe));
    }
    existing_unique(found)
}

/// A helper program that ships next to a server binary, e.g. `initdb` next
/// to `postgres` or `mariadb-install-db` next to `mariadbd`. Looks in the
/// server's directory, its sibling `bin`, then `PATH`.
pub fn companion(server_binary: &Path, names: &[&str]) -> Option<PathBuf> {
    let dir = server_binary.parent()?;
    let mut dirs = vec![dir.to_path_buf()];
    if let Some(prefix) = dir.parent() {
        dirs.push(prefix.join("bin"));
        dirs.push(prefix.join("scripts"));
    }
    for name in names {
        for dir in &dirs {
            let path = dir.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    names
        .iter()
        .find_map(|name| in_path(name).into_iter().next())
}

/// The `php` CLI binary that belongs to a `php-fpm` binary: `../bin/php`
/// for Homebrew and RED runtimes (`sbin/php-fpm`), `php8.3` next to
/// `php-fpm8.3` on Debian.
pub fn php_cli_for_fpm(fpm: &Path) -> Option<PathBuf> {
    let name = file_name(fpm);
    let suffix = name.strip_prefix("php-fpm").unwrap_or_default();
    let dir = fpm.parent()?;
    let mut candidates = vec![dir.join(format!("php{suffix}"))];
    if let Some(prefix) = dir.parent() {
        candidates.push(prefix.join("bin").join(format!("php{suffix}")));
        candidates.push(prefix.join("bin").join("php"));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Node.js binaries from nvm, fnm, Volta, asdf, mise, Homebrew and `PATH`.
pub fn node_candidates(home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let managers = [
        (home.join(".nvm/versions/node"), "bin/node"),
        (
            home.join(".local/share/fnm/node-versions"),
            "installation/bin/node",
        ),
        (
            home.join("Library/Application Support/fnm/node-versions"),
            "installation/bin/node",
        ),
        (home.join(".volta/tools/image/node"), "bin/node"),
        (home.join(".asdf/installs/nodejs"), "bin/node"),
        (home.join(".local/share/mise/installs/node"), "bin/node"),
    ];
    for (dir, binary) in managers {
        for version in sorted_dirs(&dir).into_iter().rev() {
            found.push(version.join(binary));
        }
    }
    for opt in BREW_OPT {
        for dir in sorted_dirs(Path::new(opt)).into_iter().rev() {
            let name = file_name(&dir);
            if name == "node" || name.starts_with("node@") {
                found.push(dir.join("bin/node"));
            }
        }
    }
    found.extend(in_path("node"));
    existing_unique(found)
}

/// Every executable called `name` on `PATH`, in order.
pub fn in_path(name: &str) -> Vec<PathBuf> {
    let Some(path) = std::env::var_os("PATH") else {
        return Vec::new();
    };
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .filter(|p| p.is_file())
        .collect()
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
}

fn existing_unique(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen: Vec<PathBuf> = Vec::new();
    for path in paths {
        if !path.is_file() {
            continue;
        }
        // Homebrew links `bin/x` to `Cellar/...`; keep one entry per target.
        let real = path.canonicalize().unwrap_or_else(|_| path.clone());
        if seen
            .iter()
            .any(|p| p.canonicalize().unwrap_or_else(|_| p.clone()) == real)
        {
            continue;
        }
        seen.push(path);
    }
    seen
}

pub(crate) fn sorted_dirs(dir: &Path) -> Vec<PathBuf> {
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

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn finds_php_cli_next_to_fpm() {
        let root = tempfile::tempdir().unwrap();
        let brew_fpm = root.path().join("php@8.3/sbin/php-fpm");
        let brew_cli = root.path().join("php@8.3/bin/php");
        touch(&brew_fpm);
        touch(&brew_cli);
        assert_eq!(php_cli_for_fpm(&brew_fpm), Some(brew_cli));

        let deb_fpm = root.path().join("usr/sbin/php-fpm8.2");
        let deb_cli = root.path().join("usr/bin/php8.2");
        touch(&deb_fpm);
        touch(&deb_cli);
        assert_eq!(php_cli_for_fpm(&deb_fpm), Some(deb_cli));
    }

    #[test]
    fn finds_node_from_version_managers() {
        let home = tempfile::tempdir().unwrap();
        let old = home.path().join(".nvm/versions/node/v18.20.0/bin/node");
        let new = home.path().join(".nvm/versions/node/v22.11.0/bin/node");
        let fnm = home
            .path()
            .join(".local/share/fnm/node-versions/v20.1.0/installation/bin/node");
        for p in [&old, &new, &fnm] {
            touch(p);
        }
        let found = node_candidates(home.path());
        assert!(found.contains(&old) && found.contains(&new) && found.contains(&fnm));
        let pos = |p: &PathBuf| found.iter().position(|f| f == p).unwrap();
        assert!(pos(&new) < pos(&old), "newest first: {found:?}");
    }

    #[test]
    fn finds_companions() {
        let root = tempfile::tempdir().unwrap();
        let server = root.path().join("sbin/mariadbd");
        let install = root.path().join("bin/mariadb-install-db");
        touch(&server);
        touch(&install);
        assert_eq!(
            companion(&server, &["mariadb-install-db", "mysql_install_db"]),
            Some(install)
        );
        assert_eq!(companion(&server, &["definitely-not-a-red-tool"]), None);
    }
}
