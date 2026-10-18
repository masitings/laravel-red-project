//! Framework detection and request routing.
//!
//! A driver says where a project's web root is and which script handles
//! requests that don't match a file. Routing then follows the same rules for
//! every framework:
//!
//! 1. Hidden paths (`/.env`, `/.git/...`) are never served, except
//!    `/.well-known/`.
//! 2. An existing `.php` file runs (with `PATH_INFO` for `/index.php/extra`).
//! 3. Any other existing file is served as-is.
//! 4. A directory redirects to its trailing-slash form, then serves its
//!    `index.php` or `index.html`.
//! 5. Everything else goes to the front controller, if the driver has one.

use std::path::{Component, Path, PathBuf};

/// A framework (or generic layout) RED knows how to serve.
#[derive(Debug)]
pub struct Driver {
    /// Short identifier shown to users, e.g. `laravel`.
    pub name: &'static str,
    /// Web root relative to the project directory (`""` = the project itself).
    pub web_root: &'static str,
    /// Script (relative to the web root) that handles unmatched requests.
    pub front_controller: Option<&'static str>,
    detect: fn(&Path) -> bool,
}

/// Built-in drivers, most specific first. The last one matches anything.
pub static DRIVERS: &[Driver] = &[
    Driver {
        name: "laravel",
        web_root: "public",
        front_controller: Some("index.php"),
        detect: |p| p.join("artisan").is_file() && p.join("public/index.php").is_file(),
    },
    Driver {
        name: "bedrock",
        web_root: "web",
        front_controller: Some("index.php"),
        detect: |p| {
            p.join("config/application.php").is_file() && p.join("web/wp-config.php").is_file()
        },
    },
    Driver {
        name: "craft",
        web_root: "web",
        front_controller: Some("index.php"),
        detect: |p| p.join("craft").is_file() && p.join("web/index.php").is_file(),
    },
    Driver {
        name: "symfony",
        web_root: "public",
        front_controller: Some("index.php"),
        detect: |p| p.join("bin/console").is_file() && p.join("public/index.php").is_file(),
    },
    Driver {
        name: "drupal",
        web_root: "web",
        front_controller: Some("index.php"),
        detect: |p| p.join("web/core/lib/Drupal.php").is_file(),
    },
    Driver {
        name: "drupal",
        web_root: "",
        front_controller: Some("index.php"),
        detect: |p| p.join("core/lib/Drupal.php").is_file(),
    },
    Driver {
        name: "public",
        web_root: "public",
        front_controller: Some("index.php"),
        detect: |p| p.join("public/index.php").is_file(),
    },
    Driver {
        name: "wordpress",
        web_root: "",
        front_controller: Some("index.php"),
        detect: |p| p.join("wp-config.php").is_file() || p.join("wp-load.php").is_file(),
    },
    Driver {
        name: "php",
        web_root: "",
        front_controller: Some("index.php"),
        detect: |p| p.join("index.php").is_file(),
    },
    Driver {
        name: "static",
        web_root: "",
        front_controller: None,
        detect: |_| true,
    },
];

/// The driver for the project in `site_path`.
pub fn detect(site_path: &Path) -> &'static Driver {
    DRIVERS
        .iter()
        .find(|driver| (driver.detect)(site_path))
        .unwrap_or(&DRIVERS[DRIVERS.len() - 1])
}

/// What to do with a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send this file as-is.
    Static(PathBuf),
    /// Run a PHP script.
    Php(PhpScript),
    /// Redirect to this path (a directory requested without its slash).
    Redirect(String),
    NotFound,
}

/// The CGI variables that describe which script handles a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhpScript {
    /// Absolute path of the script (`SCRIPT_FILENAME`).
    pub script_filename: PathBuf,
    /// URL path of the script (`SCRIPT_NAME`), e.g. `/index.php`.
    pub script_name: String,
    /// Absolute web root (`DOCUMENT_ROOT`).
    pub document_root: PathBuf,
    /// Extra path after the script (`PATH_INFO`), e.g. `/foo` in `/index.php/foo`.
    pub path_info: Option<String>,
}

impl Driver {
    /// Absolute web root for the project in `site_path`.
    pub fn web_root(&self, site_path: &Path) -> PathBuf {
        if self.web_root.is_empty() {
            site_path.to_owned()
        } else {
            site_path.join(self.web_root)
        }
    }

    /// Decide how to answer a request for `raw_path` (the URL path, still
    /// percent-encoded, without the query string).
    pub fn resolve(&self, site_path: &Path, raw_path: &str) -> Action {
        let root = self.web_root(site_path);
        let Some(segments) = decode_segments(raw_path) else {
            return Action::NotFound;
        };
        if is_hidden(&segments) {
            return Action::NotFound;
        }

        // A `.php` segment that exists as a file runs, with the rest as PATH_INFO.
        let mut candidate = root.clone();
        for (i, segment) in segments.iter().enumerate() {
            candidate.push(segment);
            if segment.ends_with(".php") && candidate.is_file() {
                let rest = &segments[i + 1..];
                let trailing = if raw_path.ends_with('/') { "/" } else { "" };
                let path_info = match (rest.is_empty(), trailing) {
                    (true, "") => None,
                    (true, _) => Some("/".to_owned()),
                    (false, _) => Some(format!("/{}{trailing}", rest.join("/"))),
                };
                return Action::Php(PhpScript {
                    script_filename: candidate,
                    script_name: format!("/{}", segments[..=i].join("/")),
                    document_root: root,
                    path_info,
                });
            }
        }

        let target = segments.iter().fold(root.clone(), |p, s| p.join(s));
        if target.is_file() {
            return Action::Static(target);
        }
        if target.is_dir() {
            if !segments.is_empty() && !raw_path.ends_with('/') {
                return Action::Redirect(format!("{raw_path}/"));
            }
            if target.join("index.php").is_file() {
                let mut script_name = String::from("/");
                for segment in &segments {
                    script_name.push_str(segment);
                    script_name.push('/');
                }
                script_name.push_str("index.php");
                return Action::Php(PhpScript {
                    script_filename: target.join("index.php"),
                    script_name,
                    document_root: root,
                    path_info: None,
                });
            }
            if target.join("index.html").is_file() {
                return Action::Static(target.join("index.html"));
            }
        }

        match self.front_controller {
            Some(script) if root.join(script).is_file() => Action::Php(PhpScript {
                script_filename: root.join(script),
                script_name: format!("/{script}"),
                document_root: root,
                path_info: None,
            }),
            _ => Action::NotFound,
        }
    }
}

/// Percent-decode the path and split it into safe segments. Returns `None`
/// for anything that could escape the web root.
fn decode_segments(raw_path: &str) -> Option<Vec<String>> {
    let decoded = percent_encoding::percent_decode_str(raw_path)
        .decode_utf8()
        .ok()?;
    if decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    let mut segments = Vec::new();
    for component in Path::new(decoded.as_ref()).components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(part) => segments.push(part.to_str()?.to_owned()),
            Component::ParentDir | Component::Prefix(_) => return None,
        }
    }
    Some(segments)
}

fn is_hidden(segments: &[String]) -> bool {
    segments
        .iter()
        .enumerate()
        .any(|(i, s)| s.starts_with('.') && !(i == 0 && s == ".well-known"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "x").unwrap();
        }
        dir
    }

    #[test]
    fn detects_frameworks() {
        let cases: &[(&[&str], &str)] = &[
            (&["artisan", "public/index.php"], "laravel"),
            (&["bin/console", "public/index.php"], "symfony"),
            (&["config/application.php", "web/wp-config.php"], "bedrock"),
            (&["craft", "web/index.php"], "craft"),
            (&["web/core/lib/Drupal.php", "web/index.php"], "drupal"),
            (&["public/index.php"], "public"),
            (&["wp-config.php", "index.php"], "wordpress"),
            (&["index.php"], "php"),
            (&["index.html"], "static"),
            (&[], "static"),
        ];
        for (files, expected) in cases {
            let dir = project(files);
            assert_eq!(detect(dir.path()).name, *expected, "{files:?}");
        }
    }

    #[test]
    fn laravel_routes_through_front_controller() {
        let dir = project(&["artisan", "public/index.php", "public/css/app.css", ".env"]);
        let driver = detect(dir.path());
        let root = dir.path().join("public");

        assert_eq!(
            driver.resolve(dir.path(), "/css/app.css"),
            Action::Static(root.join("css/app.css"))
        );
        let Action::Php(script) = driver.resolve(dir.path(), "/users/42") else {
            panic!("expected PHP");
        };
        assert_eq!(script.script_filename, root.join("index.php"));
        assert_eq!(script.script_name, "/index.php");
        assert_eq!(script.document_root, root);
        assert_eq!(script.path_info, None);
    }

    #[test]
    fn never_serves_hidden_or_escaping_paths() {
        let dir = project(&["index.php", ".env", ".git/config", ".well-known/acme"]);
        let driver = detect(dir.path());
        for path in [
            "/.env",
            "/.git/config",
            "/../etc/passwd",
            "/a/%2e%2e/%2e%2e/x",
            "/x%00.php",
        ] {
            assert_eq!(driver.resolve(dir.path(), path), Action::NotFound, "{path}");
        }
        assert_eq!(
            driver.resolve(dir.path(), "/.well-known/acme"),
            Action::Static(dir.path().join(".well-known/acme"))
        );
    }

    #[test]
    fn wordpress_runs_php_files_and_directory_indexes() {
        let dir = project(&[
            "wp-config.php",
            "index.php",
            "wp-login.php",
            "wp-admin/index.php",
        ]);
        let driver = detect(dir.path());

        let Action::Php(login) = driver.resolve(dir.path(), "/wp-login.php") else {
            panic!("expected PHP");
        };
        assert_eq!(login.script_name, "/wp-login.php");

        assert_eq!(
            driver.resolve(dir.path(), "/wp-admin"),
            Action::Redirect("/wp-admin/".into())
        );
        let Action::Php(admin) = driver.resolve(dir.path(), "/wp-admin/") else {
            panic!("expected PHP");
        };
        assert_eq!(admin.script_name, "/wp-admin/index.php");

        let Action::Php(pretty) = driver.resolve(dir.path(), "/2026/10/hello-world/") else {
            panic!("expected PHP");
        };
        assert_eq!(pretty.script_name, "/index.php");
    }

    #[test]
    fn path_info_after_script() {
        let dir = project(&["index.php"]);
        let driver = detect(dir.path());
        let Action::Php(script) = driver.resolve(dir.path(), "/index.php/api/users") else {
            panic!("expected PHP");
        };
        assert_eq!(script.script_name, "/index.php");
        assert_eq!(script.path_info.as_deref(), Some("/api/users"));
    }

    #[test]
    fn static_sites_serve_index_html() {
        let dir = project(&["index.html", "docs/index.html"]);
        let driver = detect(dir.path());
        assert_eq!(
            driver.resolve(dir.path(), "/"),
            Action::Static(dir.path().join("index.html"))
        );
        assert_eq!(
            driver.resolve(dir.path(), "/docs/"),
            Action::Static(dir.path().join("docs/index.html"))
        );
        assert_eq!(driver.resolve(dir.path(), "/missing"), Action::NotFound);
    }

    #[test]
    fn decodes_percent_encoding() {
        let dir = project(&["my file.txt"]);
        let driver = detect(dir.path());
        assert_eq!(
            driver.resolve(dir.path(), "/my%20file.txt"),
            Action::Static(dir.path().join("my file.txt"))
        );
    }
}
