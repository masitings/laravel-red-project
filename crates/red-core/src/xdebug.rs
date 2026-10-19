//! Xdebug on demand.
//!
//! Sites normally run in a PHP-FPM pool without Xdebug, so they stay fast.
//! RED starts a second pool per PHP version with Xdebug loaded and sends a
//! request there when Xdebug should run for it:
//!
//! - `off`: never;
//! - `auto` (default): when the request carries an Xdebug trigger (the
//!   `XDEBUG_SESSION` cookie a browser extension or IDE sets, or the
//!   `XDEBUG_SESSION_START` / `XDEBUG_TRIGGER` query parameter);
//! - `on`: always, and every request starts a debug session.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Port RED tells Xdebug to connect to (the IDE listens there). Xdebug 3's
/// own default.
pub const DEFAULT_CLIENT_PORT: u16 = 9003;

/// When Xdebug runs for a request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum XdebugMode {
    Off,
    #[default]
    Auto,
    On,
}

impl XdebugMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Auto => "auto",
            Self::On => "on",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "auto" | "trigger" => Some(Self::Auto),
            "on" | "always" => Some(Self::On),
            _ => None,
        }
    }
}

impl std::fmt::Display for XdebugMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Cookies Xdebug 3 treats as triggers.
const TRIGGER_COOKIES: [&str; 4] = [
    "XDEBUG_SESSION",
    "XDEBUG_TRIGGER",
    "XDEBUG_PROFILE",
    "XDEBUG_TRACE",
];

/// Query parameters Xdebug 3 treats as triggers.
const TRIGGER_PARAMS: [&str; 5] = [
    "XDEBUG_SESSION_START",
    "XDEBUG_SESSION",
    "XDEBUG_TRIGGER",
    "XDEBUG_PROFILE",
    "XDEBUG_TRACE",
];

/// `true` when a request with these `Cookie` headers and query string asks
/// for Xdebug.
pub fn has_trigger<'a>(cookies: impl IntoIterator<Item = &'a str>, query: Option<&str>) -> bool {
    let in_cookies = cookies.into_iter().any(|header| {
        header.split(';').any(|pair| {
            let name = pair.split('=').next().unwrap_or_default().trim();
            TRIGGER_COOKIES.contains(&name)
        })
    });
    let in_query = query.is_some_and(|q| {
        q.split('&').any(|pair| {
            let name = pair.split('=').next().unwrap_or_default();
            TRIGGER_PARAMS.contains(&name)
        })
    });
    in_cookies || in_query
}

/// `true` when the environment of a CLI run asks for Xdebug.
pub fn has_env_trigger(get: impl Fn(&str) -> Option<String>) -> bool {
    ["XDEBUG_TRIGGER", "XDEBUG_SESSION", "XDEBUG_CONFIG"]
        .iter()
        .any(|name| get(name).is_some_and(|v| !v.is_empty()))
}

/// `-d` settings that load Xdebug from `extension` for `mode`. In `auto`,
/// Xdebug only starts when triggered; in `on`, with every request.
/// Profiles and traces go to `output_dir`.
pub fn ini_settings(
    extension: &Path,
    mode: XdebugMode,
    output_dir: &Path,
    client_port: u16,
) -> Vec<String> {
    vec![
        format!("zend_extension={}", extension.display()),
        "xdebug.mode=debug,profile,trace".to_owned(),
        format!(
            "xdebug.start_with_request={}",
            if mode == XdebugMode::On {
                "yes"
            } else {
                "trigger"
            }
        ),
        "xdebug.client_host=127.0.0.1".to_owned(),
        format!("xdebug.client_port={client_port}"),
        format!("xdebug.output_dir={}", output_dir.display()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_triggers_in_cookies_and_query() {
        assert!(has_trigger(["a=1; XDEBUG_SESSION=PHPSTORM"], None));
        assert!(has_trigger(["XDEBUG_TRIGGER=1"], None));
        assert!(has_trigger([], Some("page=2&XDEBUG_SESSION_START=1")));
        assert!(has_trigger([], Some("XDEBUG_PROFILE")));
        assert!(!has_trigger(
            ["session=abc; NOT_XDEBUG_SESSION=1"],
            Some("x=XDEBUG_SESSION")
        ));
        assert!(!has_trigger([], None));
    }

    #[test]
    fn finds_triggers_in_the_environment() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert!(has_env_trigger(env(&[("XDEBUG_TRIGGER", "1")])));
        assert!(has_env_trigger(env(&[("XDEBUG_SESSION", "PHPSTORM")])));
        assert!(!has_env_trigger(env(&[("XDEBUG_TRIGGER", "")])));
        assert!(!has_env_trigger(env(&[("PATH", "/bin")])));
    }

    #[test]
    fn builds_settings_per_mode() {
        let auto = ini_settings(
            Path::new("/rt/lib/xdebug.so"),
            XdebugMode::Auto,
            Path::new("/logs/xdebug"),
            9003,
        );
        assert_eq!(auto[0], "zend_extension=/rt/lib/xdebug.so");
        assert!(auto.contains(&"xdebug.start_with_request=trigger".to_owned()));
        assert!(auto.contains(&"xdebug.client_port=9003".to_owned()));
        let on = ini_settings(Path::new("/x.so"), XdebugMode::On, Path::new("/o"), 9010);
        assert!(on.contains(&"xdebug.start_with_request=yes".to_owned()));
        assert!(on.contains(&"xdebug.client_port=9010".to_owned()));
    }

    #[test]
    fn parses_modes() {
        assert_eq!(XdebugMode::parse("Auto"), Some(XdebugMode::Auto));
        assert_eq!(XdebugMode::parse("always"), Some(XdebugMode::On));
        assert_eq!(XdebugMode::parse("nope"), None);
        assert_eq!(XdebugMode::default(), XdebugMode::Auto);
    }
}
