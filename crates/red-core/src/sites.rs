//! Which directories are served, under which names.
//!
//! A site is either *parked* (a sub-directory of a parked path, named after
//! the directory) or *linked* (an explicit name pointing at any directory).
//! A link wins when both define the same name.

use std::path::{Path, PathBuf};

use crate::State;

/// How a site came to exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteKind {
    Parked,
    Linked,
}

/// A directory served under `<name>.<tld>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    pub name: String,
    pub path: PathBuf,
    pub kind: SiteKind,
}

/// Turn a directory name into a site name: lowercase, a leading `www.`
/// removed, and anything other than letters, digits, `.` and `-` replaced by
/// `-`. Returns `None` for hidden directories or names with nothing left.
pub fn normalize_name(dir_name: &str) -> Option<String> {
    if dir_name.starts_with('.') {
        return None;
    }
    let lower = dir_name.to_lowercase();
    let lower = lower.strip_prefix("www.").unwrap_or(&lower);
    let mut name = String::with_capacity(lower.len());
    for c in lower.chars() {
        let c = if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
            c
        } else {
            '-'
        };
        if c == '-' && name.ends_with('-') {
            continue;
        }
        name.push(c);
    }
    let name = name.trim_matches(|c| c == '-' || c == '.');
    (!name.is_empty()).then(|| name.to_owned())
}

/// Every site, sorted by name. Links shadow parked sites of the same name.
pub fn list(state: &State) -> Vec<Site> {
    let mut sites: Vec<Site> = state
        .links
        .iter()
        .map(|(name, path)| Site {
            name: name.clone(),
            path: path.clone(),
            kind: SiteKind::Linked,
        })
        .collect();

    for parked in &state.parked {
        for (name, path) in parked_sites(parked) {
            if !sites.iter().any(|s| s.name == name) {
                sites.push(Site {
                    name,
                    path,
                    kind: SiteKind::Parked,
                });
            }
        }
    }
    sites.sort_by(|a, b| a.name.cmp(&b.name));
    sites
}

/// The site called exactly `name`, if any.
pub fn find(state: &State, name: &str) -> Option<Site> {
    if let Some(path) = state.links.get(name) {
        return Some(Site {
            name: name.to_owned(),
            path: path.clone(),
            kind: SiteKind::Linked,
        });
    }
    state.parked.iter().find_map(|parked| {
        parked_sites(parked)
            .into_iter()
            .find(|(site_name, _)| site_name == name)
            .map(|(name, path)| Site {
                name,
                path,
                kind: SiteKind::Parked,
            })
    })
}

/// The site that should answer a request for `host`.
///
/// Accepts `<name>.<tld>` and `<name>.localhost` (browsers resolve the
/// latter to loopback on their own), with or without a port. When no site
/// matches the full name, leading labels are dropped, so `api.blog.test`
/// falls back to the `blog` site.
pub fn resolve_host(state: &State, host: &str, tld: &str) -> Option<Site> {
    match resolve_target(state, host, tld)? {
        Target::Site(site) => Some(site),
        Target::Proxy { .. } => None,
    }
}

/// What answers a host: a site on disk, or a proxy to a local port.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Site(Site),
    Proxy { name: String, url: String },
}

impl Target {
    /// The site or proxy name.
    pub fn name(&self) -> &str {
        match self {
            Target::Site(site) => &site.name,
            Target::Proxy { name, .. } => name,
        }
    }
}

/// Like [`resolve_host`], but proxies count too. At each level a proxy wins
/// over a site of the same name.
pub fn resolve_target(state: &State, host: &str, tld: &str) -> Option<Target> {
    let host = strip_port(host).trim_end_matches('.').to_ascii_lowercase();
    let mut name = host
        .strip_suffix(&format!(".{tld}"))
        .or_else(|| host.strip_suffix(".localhost"))?;
    loop {
        if let Some(url) = state.proxies.get(name) {
            return Some(Target::Proxy {
                name: name.to_owned(),
                url: url.clone(),
            });
        }
        if let Some(site) = find(state, name) {
            return Some(Target::Site(site));
        }
        name = name.split_once('.')?.1;
    }
}

/// Normalise what a user typed as a proxy target into `http://host:port`.
/// Accepts a bare port (`5173`), `host:port`, or an `http://` URL.
pub fn normalize_proxy_target(input: &str) -> Option<String> {
    let input = input.trim().trim_end_matches('/');
    if let Ok(port) = input.parse::<u16>() {
        return (port != 0).then(|| format!("http://127.0.0.1:{port}"));
    }
    let rest = match input.split_once("://") {
        Some(("http", rest)) => rest,
        Some(_) => return None, // only plain HTTP upstreams for now
        None => input,
    };
    let (authority, path) = rest.split_once('/').map_or((rest, ""), |(a, p)| (a, p));
    let (host, port) = authority.rsplit_once(':')?;
    let port: u16 = port.parse().ok().filter(|p| *p != 0)?;
    let host_ok = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'));
    if !host_ok {
        return None;
    }
    Some(if path.is_empty() {
        format!("http://{host}:{port}")
    } else {
        format!("http://{host}:{port}/{path}")
    })
}

fn strip_port(host: &str) -> &str {
    if host.starts_with('[') {
        return host; // IPv6 literal: never a site name
    }
    host.rsplit_once(':').map_or(host, |(h, _)| h)
}

fn parked_sites(parked: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(parked) else {
        return Vec::new();
    };
    let mut sites: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = normalize_name(&entry.file_name().to_string_lossy())?;
            Some((name, entry.path()))
        })
        .collect();
    sites.sort();
    sites
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxies_win_and_targets_are_normalised() {
        let (_root, mut state) = fixture();
        state
            .proxies
            .insert("blog".into(), "http://127.0.0.1:5173".into());
        let target = resolve_target(&state, "hmr.blog.test", "test").unwrap();
        assert_eq!(
            target,
            Target::Proxy {
                name: "blog".into(),
                url: "http://127.0.0.1:5173".into()
            }
        );
        assert_eq!(resolve_host(&state, "blog.test", "test"), None);

        let n = |s: &str| normalize_proxy_target(s);
        assert_eq!(n("5173").as_deref(), Some("http://127.0.0.1:5173"));
        assert_eq!(
            n("localhost:3000").as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(
            n("http://127.0.0.1:8000/").as_deref(),
            Some("http://127.0.0.1:8000")
        );
        assert_eq!(
            n("http://127.0.0.1:8000/app").as_deref(),
            Some("http://127.0.0.1:8000/app")
        );
        assert_eq!(n("https://x:1"), None);
        assert_eq!(n("0"), None);
        assert_eq!(n("nope"), None);
        assert_eq!(n("bad host:1"), None);
    }

    #[test]
    fn normalizes_directory_names() {
        assert_eq!(normalize_name("Blog").as_deref(), Some("blog"));
        assert_eq!(normalize_name("www.shop.com").as_deref(), Some("shop.com"));
        assert_eq!(normalize_name("My App_v2").as_deref(), Some("my-app-v2"));
        assert_eq!(normalize_name("--x--").as_deref(), Some("x"));
        assert_eq!(normalize_name(".git"), None);
        assert_eq!(normalize_name("___"), None);
    }

    fn fixture() -> (tempfile::TempDir, State) {
        let root = tempfile::tempdir().unwrap();
        let parked = root.path().join("Sites");
        for dir in ["blog", "Shop", ".hidden", "herd.laravel.com"] {
            std::fs::create_dir_all(parked.join(dir)).unwrap();
        }
        std::fs::write(parked.join("notes.txt"), "not a site").unwrap();
        let elsewhere = root.path().join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();

        let mut state = State::default();
        state.park(parked);
        state.link("api".into(), elsewhere.clone());
        state.link("shop".into(), elsewhere); // shadows the parked Shop
        (root, state)
    }

    #[test]
    fn lists_parked_and_linked_sites() {
        let (_root, state) = fixture();
        let sites = list(&state);
        let names: Vec<_> = sites.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["api", "blog", "herd.laravel.com", "shop"]);
        let shop = sites.iter().find(|s| s.name == "shop").unwrap();
        assert_eq!(shop.kind, SiteKind::Linked);
    }

    #[test]
    fn resolves_hosts() {
        let (_root, state) = fixture();
        let name = |host: &str| resolve_host(&state, host, "test").map(|s| s.name);
        assert_eq!(name("blog.test").as_deref(), Some("blog"));
        assert_eq!(name("BLOG.test:8180").as_deref(), Some("blog"));
        assert_eq!(name("blog.localhost").as_deref(), Some("blog"));
        assert_eq!(name("v2.api.blog.test").as_deref(), Some("blog"));
        assert_eq!(
            name("herd.laravel.com.test").as_deref(),
            Some("herd.laravel.com")
        );
        assert_eq!(name("nope.test"), None);
        assert_eq!(name("blog.com"), None);
        assert_eq!(name("127.0.0.1:8180"), None);
        assert_eq!(name("[::1]:8180"), None);
    }
}
