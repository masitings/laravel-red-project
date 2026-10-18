//! Where RED keeps its files.
//!
//! RED follows each operating system's conventions:
//!
//! | Kind    | macOS                                     | Linux                                  |
//! |---------|-------------------------------------------|----------------------------------------|
//! | config  | `~/Library/Application Support/RED`       | `$XDG_CONFIG_HOME/red`                 |
//! | data    | `~/Library/Application Support/RED/data`  | `$XDG_DATA_HOME/red`                   |
//! | run     | `~/Library/Application Support/RED/run`   | `$XDG_RUNTIME_DIR/red` (or data/run)   |
//! | logs    | `~/Library/Logs/RED`                      | `$XDG_STATE_HOME/red/logs`             |
//! | shims   | `~/.red/bin`                              | `~/.red/bin`                           |
//!
//! Setting `RED_HOME` puts everything under that single directory instead.
//! It exists for tests and for running a development build next to an
//! installed one.

use std::path::{Path, PathBuf};

/// Environment variable that overrides every RED directory.
pub const RED_HOME_ENV: &str = "RED_HOME";

/// File name of the daemon's Control API socket.
pub const SOCKET_FILE: &str = "redd.sock";

/// Resolved directories for one RED installation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub run_dir: PathBuf,
    pub log_dir: PathBuf,
    pub bin_dir: PathBuf,
}

/// Error returned when the user's home directory cannot be determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoHomeDir;

impl std::fmt::Display for NoHomeDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("could not determine the home directory; set RED_HOME")
    }
}

impl std::error::Error for NoHomeDir {}

impl Paths {
    /// Resolve paths for the current user, honouring `RED_HOME`.
    pub fn discover() -> Result<Self, NoHomeDir> {
        match std::env::var_os(RED_HOME_ENV) {
            Some(home) if !home.is_empty() => Ok(Self::under(PathBuf::from(home))),
            _ => Self::system(),
        }
    }

    /// Every directory below a single root. Used for `RED_HOME` and tests.
    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            run_dir: root.join("run"),
            log_dir: root.join("logs"),
            bin_dir: root.join("bin"),
        }
    }

    /// OS-conventional locations for the current user.
    pub fn system() -> Result<Self, NoHomeDir> {
        let base = directories::BaseDirs::new().ok_or(NoHomeDir)?;
        let home = base.home_dir();
        let bin_dir = home.join(".red").join("bin");

        if cfg!(target_os = "macos") {
            let app = base.data_dir().join("RED");
            Ok(Self {
                config_dir: app.clone(),
                data_dir: app.join("data"),
                run_dir: app.join("run"),
                log_dir: home.join("Library").join("Logs").join("RED"),
                bin_dir,
            })
        } else {
            let data_dir = base.data_dir().join("red");
            let run_dir = base
                .runtime_dir()
                .map(|dir| dir.join("red"))
                .unwrap_or_else(|| data_dir.join("run"));
            let state_dir = base
                .state_dir()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| home.join(".local").join("state"));
            Ok(Self {
                config_dir: base.config_dir().join("red"),
                data_dir,
                run_dir,
                log_dir: state_dir.join("red").join("logs"),
                bin_dir,
            })
        }
    }

    /// Path of the daemon's Control API socket.
    pub fn socket(&self) -> PathBuf {
        self.run_dir.join(SOCKET_FILE)
    }

    /// Path of the user-editable global configuration file.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn under_puts_everything_below_root() {
        let paths = Paths::under("/tmp/red-test");
        assert_eq!(paths.socket(), PathBuf::from("/tmp/red-test/run/redd.sock"));
        assert_eq!(
            paths.config_file(),
            PathBuf::from("/tmp/red-test/config/config.toml")
        );
        assert!(paths.data_dir.starts_with("/tmp/red-test"));
        assert!(paths.log_dir.starts_with("/tmp/red-test"));
    }

    #[test]
    fn system_paths_follow_os_conventions() {
        let Ok(paths) = Paths::system() else {
            return; // no home directory in this environment
        };
        if cfg!(target_os = "macos") {
            assert!(
                paths
                    .config_dir
                    .ends_with("Library/Application Support/RED")
            );
            assert!(paths.log_dir.ends_with("Library/Logs/RED"));
        } else {
            assert!(paths.config_dir.ends_with("red"));
            assert!(paths.data_dir.ends_with("red"));
        }
        assert!(paths.bin_dir.ends_with(".red/bin"));
    }
}
