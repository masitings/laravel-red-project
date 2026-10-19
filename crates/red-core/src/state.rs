//! Machine-managed state (`state.json` in the data directory).
//!
//! Unlike `config.toml`, this file is written by RED whenever a command
//! changes something: parking a directory, linking a site, choosing a PHP
//! version. Writes are atomic (temporary file + rename).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Everything RED remembers between runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Directories whose sub-directories are each served as a site.
    pub parked: Vec<PathBuf>,
    /// Sites linked by name to a directory anywhere on disk.
    pub links: BTreeMap<String, PathBuf>,
    /// PHP version (`"8.3"`) pinned per site name.
    pub isolated: BTreeMap<String, String>,
    /// PHP version used by sites that are not isolated.
    pub default_php: Option<String>,
    /// Sites served over HTTPS, with their certificate's expiry (Unix time).
    pub secured: BTreeMap<String, u64>,
    /// Names proxied to a local URL, e.g. `app` → `http://127.0.0.1:5173`.
    pub proxies: BTreeMap<String, String>,
    /// Database and cache services by instance name, e.g. `mysql`.
    pub services: BTreeMap<String, crate::ServiceConfig>,
    /// Send `dump()` output from sites to RED instead of the page.
    pub capture_dumps: bool,
    /// When sites run with Xdebug (see [`crate::xdebug`]).
    pub xdebug: crate::xdebug::XdebugMode,
}

/// Errors produced while reading or writing the state file.
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is corrupt: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl State {
    /// Load the state from `path`. A missing file yields an empty state.
    pub fn load(path: &Path) -> Result<Self, StateError> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|source| StateError::Parse {
                path: path.to_owned(),
                source,
            }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(StateError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    /// Write the state to `path` atomically.
    pub fn save(&self, path: &Path) -> Result<(), StateError> {
        let write_err = |source| StateError::Write {
            path: path.to_owned(),
            source,
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(write_err)?;
        }
        let mut bytes = serde_json::to_vec_pretty(self).expect("state is always serialisable");
        bytes.push(b'\n');
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, bytes).map_err(write_err)?;
        std::fs::rename(&tmp, path).map_err(write_err)
    }

    /// Park `dir`. Returns `false` when it was already parked.
    pub fn park(&mut self, dir: PathBuf) -> bool {
        if self.parked.contains(&dir) {
            return false;
        }
        self.parked.push(dir);
        true
    }

    /// Stop serving the sub-directories of `dir`. Returns `false` when it was
    /// not parked.
    pub fn forget(&mut self, dir: &Path) -> bool {
        let before = self.parked.len();
        self.parked.retain(|p| p != dir);
        self.parked.len() != before
    }

    /// Link `name` to `dir`, replacing an existing link of that name.
    pub fn link(&mut self, name: String, dir: PathBuf) {
        self.links.insert(name, dir);
    }

    /// Remove the link `name`. Returns `false` when there was none.
    pub fn unlink(&mut self, name: &str) -> bool {
        self.links.remove(name).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            State::load(&dir.path().join("state.json")).unwrap(),
            State::default()
        );
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("state.json");
        let mut state = State::default();
        assert!(state.park("/Users/me/Sites".into()));
        assert!(!state.park("/Users/me/Sites".into()));
        state.link("blog".into(), "/src/blog".into());
        state.isolated.insert("blog".into(), "8.2".into());
        state.default_php = Some("8.4".into());
        state.secured.insert("blog".into(), 1_900_000_000);
        state.save(&path).unwrap();

        assert_eq!(State::load(&path).unwrap(), state);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn forget_and_unlink_report_changes() {
        let mut state = State::default();
        state.park("/a".into());
        state.link("x".into(), "/x".into());
        assert!(state.forget(Path::new("/a")));
        assert!(!state.forget(Path::new("/a")));
        assert!(state.unlink("x"));
        assert!(!state.unlink("x"));
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{not json").unwrap();
        assert!(matches!(State::load(&path), Err(StateError::Parse { .. })));
    }
}
