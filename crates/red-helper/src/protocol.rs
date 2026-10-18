//! Closed set of requests the helper accepts. Anything else is rejected.

use serde::{Deserialize, Serialize};

/// Bumped whenever a request or response changes shape.
pub const PROTOCOL_VERSION: u32 = 1;

/// A request from `redd` to the helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
#[allow(dead_code)] // used once the IPC lands in M1
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
