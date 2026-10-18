//! Closed set of requests the helper accepts. Anything else is rejected.

use serde::{Deserialize, Serialize};

/// Bumped whenever a request or response changes shape.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from `redd` to the helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Report version and protocol.
    Status,
    /// Open loopback listeners on 80 and 443 and pass their fds back.
    OpenWebPorts,
    /// Point `<tld>` at redd's DNS responder on `port`. The helper builds the
    /// file contents itself after validating both values.
    InstallResolver { tld: String, port: u16 },
    /// Remove the resolver file for `<tld>`.
    RemoveResolver { tld: String },
}

/// The helper's answer: one JSON line. For `OpenWebPorts` the listening
/// sockets travel alongside it as `SCM_RIGHTS` ancillary data, in the order
/// named by `fds`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u32>,
    /// Labels of the passed descriptors: `http4`, `http6`, `https4`, `https6`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fds: Vec<String>,
}

impl Response {
    pub fn ok() -> Self {
        Self {
            ok: true,
            ..Self::default()
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(message.into()),
            ..Self::default()
        }
    }
}

/// `true` for a TLD the helper will write a resolver file for.
pub fn valid_tld(tld: &str) -> bool {
    !tld.is_empty()
        && tld.len() <= 63
        && !tld.starts_with('-')
        && !tld.ends_with('-')
        && tld
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_use_tagged_json() {
        let json = serde_json::to_value(Request::InstallResolver {
            tld: "test".into(),
            port: 5353,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"op": "install_resolver", "tld": "test", "port": 5353})
        );
    }

    #[test]
    fn validates_tlds() {
        assert!(valid_tld("test"));
        assert!(valid_tld("dev-local"));
        for bad in ["", "Test", "../etc", "a/b", "-x", "x-", "a.b"] {
            assert!(!valid_tld(bad), "{bad}");
        }
    }

    #[test]
    fn unknown_ops_and_fields_are_rejected() {
        assert!(serde_json::from_str::<Request>(r#"{"op":"run_shell"}"#).is_err());
        assert!(
            serde_json::from_str::<Request>(
                r#"{"op":"remove_resolver","tld":"test","path":"/etc"}"#
            )
            .is_err()
        );
    }
}
