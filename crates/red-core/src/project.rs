//! Per-project settings in `red.toml`, committed with the project so every
//! developer gets the same setup from `red init`.
//!
//! ```toml
//! name = "blog"                 # site name: blog.test
//! php = "8.3"                   # PHP version for this site
//! secure = true                 # serve over HTTPS
//! node = "22"                   # Node.js for `red node` / `red npm` / `red npx`
//! services = ["mysql", "redis"] # services `red init` makes sure are running
//! ```
//!
//! Every key is optional. redd reads `php` on its own (so editing the file
//! takes effect right away); the other keys are applied by `red init`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name of the project settings.
pub const FILE_NAME: &str = "red.toml";

/// Contents of `red.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    /// Site name; defaults to the directory name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// PHP version, e.g. `"8.3"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub php: Option<String>,
    /// Serve the site over HTTPS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secure: Option<bool>,
    /// Node.js version (major, `major.minor` or exact), e.g. `"22"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Services the project needs: `mysql`, `postgres`, `redis`, or the
    /// name of a configured service instance.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<String>,
}

/// Errors reading `red.toml`.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: Box<toml::de::Error>,
    },
}

impl ProjectConfig {
    /// Load `red.toml` from `dir`. `Ok(None)` when there is none.
    pub fn load(dir: &Path) -> Result<Option<Self>, ProjectError> {
        let path = dir.join(FILE_NAME);
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text)
                .map(Some)
                .map_err(|source| ProjectError::Parse {
                    path,
                    source: Box::new(source),
                }),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(ProjectError::Read { path, source }),
        }
    }

    /// Find the nearest `red.toml` in `dir` or its parents. Returns the
    /// project root and its settings.
    pub fn find(dir: &Path) -> Result<Option<(PathBuf, Self)>, ProjectError> {
        for ancestor in dir.ancestors() {
            if let Some(config) = Self::load(ancestor)? {
                return Ok(Some((ancestor.to_path_buf(), config)));
            }
        }
        Ok(None)
    }

    /// The file's text, with a short header explaining it.
    pub fn render(&self) -> String {
        let body = toml::to_string(self).expect("project config serialises");
        format!(
            "# RED project settings. Commit this file; `red init` applies it.\n\
             # Keys: name, php, secure, node, services. See `red init --help`.\n\n{body}"
        )
    }
}

/// Node.js version requested by a project without `red.toml`: `.nvmrc`
/// or `.node-version`, e.g. `v22.11.0` or `20`.
pub fn node_version_file(dir: &Path) -> Option<String> {
    [".nvmrc", ".node-version"].iter().find_map(|name| {
        let text = std::fs::read_to_string(dir.join(name)).ok()?;
        let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
        Some(line.trim_start_matches('v').to_owned())
    })
}

/// `true` when Node.js `version` (e.g. `22.11.0`, with or without `v`)
/// satisfies `wanted` (`22`, `22.11` or `22.11.0`).
pub fn node_version_matches(version: &str, wanted: &str) -> bool {
    let version = version.trim().trim_start_matches('v');
    let wanted = wanted.trim().trim_start_matches('v');
    if wanted.is_empty() {
        return false;
    }
    let mut have = version.split('.');
    wanted.split('.').all(|part| have.next() == Some(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let config = ProjectConfig {
            name: Some("blog".into()),
            php: Some("8.3".into()),
            secure: Some(true),
            node: Some("22".into()),
            services: vec!["mysql".into(), "redis".into()],
        };
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), config.render()).unwrap();
        assert_eq!(ProjectConfig::load(dir.path()).unwrap(), Some(config));
    }

    #[test]
    fn finds_the_nearest_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "php = \"8.2\"\n").unwrap();
        let nested = dir.path().join("app/Http");
        std::fs::create_dir_all(&nested).unwrap();
        let (root, config) = ProjectConfig::find(&nested).unwrap().unwrap();
        assert_eq!(root, dir.path());
        assert_eq!(config.php.as_deref(), Some("8.2"));
    }

    #[test]
    fn rejects_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "pph = \"8.2\"\n").unwrap();
        assert!(matches!(
            ProjectConfig::load(dir.path()),
            Err(ProjectError::Parse { .. })
        ));
    }

    #[test]
    fn reads_node_version_files() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(node_version_file(dir.path()), None);
        std::fs::write(dir.path().join(".nvmrc"), "v20.11.1\n").unwrap();
        assert_eq!(node_version_file(dir.path()).as_deref(), Some("20.11.1"));
    }

    #[test]
    fn matches_node_versions() {
        assert!(node_version_matches("v22.11.0", "22"));
        assert!(node_version_matches("22.11.0", "22.11"));
        assert!(!node_version_matches("v22.11.0", "2"));
        assert!(!node_version_matches("v20.1.0", "22"));
        assert!(!node_version_matches("v20.1.0", ""));
    }
}
