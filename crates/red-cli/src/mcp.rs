//! `red mcp`: a Model Context Protocol server on stdio.
//!
//! AI coding agents (Claude Code, Cursor, Zed, …) start `red mcp` and talk
//! JSON-RPC 2.0 to it over stdin/stdout, one message per line. Each tool is
//! a thin wrapper around redd's Control API, so the agent sees the same
//! sites, logs, mail and dumps as `red` and the app.
//!
//! Only MCP messages go to stdout; anything else goes to stderr.

use std::path::Path;

use red_api::client::{Client, ClientError, Method};
use red_api::{SiteInfo, endpoints};
use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::inspect::{encode, html_to_text};

/// Protocol versions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "RED is the local PHP development environment on this machine. \
It serves every linked or parked project directory as https://<name>.test (or another TLD), \
runs PHP-FPM, databases and other services, and catches what apps send: mail (SMTP), \
dump() output and logs. Use `site_for_path` to find the site for the project you are \
working in, then read its logs, caught mail and dumps to check what the app did. \
`doctor` explains setup problems.";

/// Run the server until stdin closes.
pub(crate) async fn serve(client: Client) -> anyhow::Result<()> {
    let server = Server::new(client);
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = server.handle_line(&line).await {
            // serde_json never emits raw newlines, as the transport requires.
            let mut out = serde_json::to_vec(&reply)?;
            out.push(b'\n');
            stdout.write_all(&out).await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

pub(crate) struct Server {
    client: Client,
}

impl Server {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Handle one line of input; `None` when there is nothing to send back
    /// (notifications and responses).
    pub(crate) async fn handle_line(&self, line: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(line) {
            Ok(value) => value,
            Err(err) => {
                return Some(error(
                    Value::Null,
                    PARSE_ERROR,
                    &format!("parse error: {err}"),
                ));
            }
        };
        match message {
            // Batches were part of 2025-03-26; answer them for older clients.
            Value::Array(batch) if !batch.is_empty() => {
                let mut replies = Vec::new();
                for message in batch {
                    if let Some(reply) = self.handle(message).await {
                        replies.push(reply);
                    }
                }
                (!replies.is_empty()).then_some(Value::Array(replies))
            }
            message => self.handle(message).await,
        }
    }

    async fn handle(&self, message: Value) -> Option<Value> {
        let Value::Object(message) = message else {
            return Some(error(Value::Null, INVALID_REQUEST, "expected an object"));
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to something we never ask, or garbage.
            return id.map(|id| error(id, INVALID_REQUEST, "missing method"));
        };
        // Notifications (no id) never get an answer.
        let id = id?;
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "initialize" => Ok(initialize(&params)),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_list() })),
            "tools/call" => self.call(&params).await,
            "resources/list" => Ok(json!({ "resources": [] })),
            "resources/templates/list" => Ok(json!({ "resourceTemplates": [] })),
            "prompts/list" => Ok(json!({ "prompts": [] })),
            _ => Err((METHOD_NOT_FOUND, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => error(id, code, &message),
        })
    }

    async fn call(&self, params: &Value) -> Result<Value, (i64, String)> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or((INVALID_PARAMS, "missing tool name".to_owned()))?;
        if !TOOLS.iter().any(|t| t.name == name) {
            return Err((INVALID_PARAMS, format!("unknown tool: {name}")));
        }
        let empty = Map::new();
        let args = params
            .get("arguments")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        Ok(match self.run_tool(name, args).await {
            Ok(value) => tool_result(value),
            Err(err) => json!({
                "content": [{ "type": "text", "text": err.to_string() }],
                "isError": true,
            }),
        })
    }

    async fn get(&self, path: &str) -> Result<Value, ToolError> {
        Ok(self.client.request(Method::GET, path, None).await?)
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, ToolError> {
        Ok(self.client.request(method, path, body).await?)
    }

    async fn run_tool(&self, name: &str, args: &Map<String, Value>) -> Result<Value, ToolError> {
        match name {
            "status" => self.get(endpoints::STATUS).await,
            "doctor" => self.get(endpoints::DOCTOR).await,
            "list_sites" => Ok(json!({ "sites": self.get(endpoints::SITES).await? })),
            "site_for_path" => {
                let path = required(args, "path")?;
                let dir = Path::new(path);
                if !dir.is_absolute() {
                    return Err(ToolError::Input("path must be absolute".into()));
                }
                let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_owned());
                let sites: Vec<SiteInfo> =
                    serde_json::from_value(self.get(endpoints::SITES).await?)
                        .map_err(ClientError::from)?;
                match crate::site_containing(&sites, &dir) {
                    Some(site) => Ok(json!({ "site": site })),
                    None => Err(ToolError::Input(format!(
                        "{} is not inside a RED site; link it with the link_site tool",
                        dir.display()
                    ))),
                }
            }
            "link_site" => {
                let path = required(args, "path")?;
                if !Path::new(path).is_absolute() {
                    return Err(ToolError::Input("path must be absolute".into()));
                }
                let mut body = json!({ "path": path });
                if let Some(name) = optional(args, "name")? {
                    body["name"] = json!(name);
                }
                let site = self
                    .send(Method::POST, endpoints::LINKS, Some(body))
                    .await?;
                Ok(json!({ "site": site }))
            }
            "secure_site" => {
                let name = required(args, "name")?;
                let secure = args.get("secure").and_then(Value::as_bool).unwrap_or(true);
                let method = if secure { Method::PUT } else { Method::DELETE };
                let site = self
                    .send(method, &endpoints::site_secure(name), None)
                    .await?;
                Ok(json!({ "site": site }))
            }
            "php_versions" => self.get(endpoints::PHP).await,
            "set_xdebug" => {
                let mode = required(args, "mode")?;
                if !["off", "auto", "on"].contains(&mode) {
                    return Err(ToolError::Input("mode must be off, auto or on".into()));
                }
                self.send(
                    Method::PUT,
                    endpoints::XDEBUG,
                    Some(json!({ "mode": mode })),
                )
                .await
            }
            "list_logs" => Ok(json!({ "logs": self.get(endpoints::LOGS).await? })),
            "read_log" => {
                let id = required(args, "id")?;
                let lines = args
                    .get("lines")
                    .and_then(Value::as_u64)
                    .unwrap_or(50)
                    .clamp(1, 1000);
                let path = format!("{}?id={}&lines={lines}", endpoints::LOG_READ, encode(id));
                self.get(&path).await
            }
            "list_mail" => {
                let path = match optional(args, "mailbox")? {
                    Some(m) => format!("{}?mailbox={}", endpoints::MAIL, encode(m)),
                    None => endpoints::MAIL.to_owned(),
                };
                Ok(json!({ "messages": self.get(&path).await? }))
            }
            "read_mail" => {
                let id = required(args, "id")?;
                let include_html = args
                    .get("include_html")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut message = self.get(&endpoints::mail(&encode(id))).await?;
                slim_mail(&mut message, include_html);
                Ok(message)
            }
            "clear_mail" => {
                let path = match optional(args, "mailbox")? {
                    Some(m) => format!("{}?mailbox={}", endpoints::MAIL, encode(m)),
                    None => endpoints::MAIL.to_owned(),
                };
                self.send(Method::DELETE, &path, None).await?;
                Ok(json!({ "cleared": true }))
            }
            "list_dumps" => {
                let after = args.get("after_id").and_then(Value::as_u64).unwrap_or(0);
                let mut response = self.get(endpoints::DUMPS).await?;
                slim_dumps(&mut response, after);
                Ok(response)
            }
            "clear_dumps" => {
                self.send(Method::DELETE, endpoints::DUMPS, None).await?;
                Ok(json!({ "cleared": true }))
            }
            "list_services" => self.get(endpoints::SERVICES).await,
            "service_action" => {
                let name = required(args, "name")?;
                let action = required(args, "action")?;
                if !["start", "stop", "restart"].contains(&action) {
                    return Err(ToolError::Input(
                        "action must be start, stop or restart".into(),
                    ));
                }
                let service = self
                    .send(Method::POST, &endpoints::service_action(name, action), None)
                    .await?;
                Ok(json!({ "service": service }))
            }
            "list_runtimes" => self.get(endpoints::RUNTIMES).await,
            "list_proxies" => Ok(json!({ "proxies": self.get(endpoints::PROXIES).await? })),
            _ => unreachable!("checked against TOOLS"),
        }
    }
}

/// Why a tool call failed; reported to the model as a tool error.
#[derive(Debug, thiserror::Error)]
enum ToolError {
    #[error("{0}")]
    Input(String),
    #[error("{}", client_hint(.0))]
    Client(#[from] ClientError),
}

fn client_hint(err: &ClientError) -> String {
    match err {
        ClientError::DaemonNotRunning { .. } => {
            format!("{err}. Start it with `red restart` or by opening the RED app, then try again.")
        }
        _ => err.to_string(),
    }
}

fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, ToolError> {
    match args.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Ok(s),
        Some(_) => Err(ToolError::Input(format!(
            "`{key}` must be a non-empty string"
        ))),
        None => Err(ToolError::Input(format!("missing argument `{key}`"))),
    }
}

fn optional<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ToolError::Input(format!("`{key}` must be a string"))),
    }
}

/// A successful result: the JSON as structured content plus a text copy
/// for clients that only read `content`.
fn tool_result(value: Value) -> Value {
    let structured = match value {
        Value::Object(_) => value,
        Value::Null => json!({ "ok": true }),
        other => json!({ "result": other }),
    };
    let text = serde_json::to_string_pretty(&structured).unwrap_or_default();
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": structured,
        "isError": false,
    })
}

/// Give a caught message a plain-text body and drop the HTML unless asked;
/// it is mostly markup the model does not need.
fn slim_mail(message: &mut Value, include_html: bool) {
    let Some(message) = message.as_object_mut() else {
        return;
    };
    let html = message
        .get("html")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let has_text = message.get("text").is_some_and(|t| t.is_string());
    if !has_text && let Some(html) = &html {
        message.insert("text".into(), json!(html_to_text(html)));
    }
    if !include_html {
        message.remove("html");
    }
}

/// Keep dumps after `after`, with their HTML turned into text.
fn slim_dumps(response: &mut Value, after: u64) {
    let Some(dumps) = response.get_mut("dumps").and_then(Value::as_array_mut) else {
        return;
    };
    dumps.retain(|d| d.get("id").and_then(Value::as_u64).unwrap_or(0) > after);
    for dump in dumps {
        if let Some(dump) = dump.as_object_mut()
            && let Some(Value::String(html)) = dump.remove("html")
        {
            dump.insert("text".into(), json!(html_to_text(&html)));
        }
    }
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|v| PROTOCOL_VERSIONS.contains(v))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": {
            "name": "red",
            "title": "RED Project",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "instructions": INSTRUCTIONS,
    })
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

// ---- tool definitions ---------------------------------------------------------------

/// What a tool does to the machine, for the client's confirmation prompts.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Effect {
    /// Only reads.
    Read,
    /// Changes something; repeating it changes nothing more.
    Change,
    /// Deletes caught data.
    Delete,
}

struct Tool {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    effect: Effect,
    /// `(name, JSON type, description, required)`.
    params: &'static [(&'static str, &'static str, &'static str, bool)],
    /// Allowed values for string parameters.
    enums: &'static [(&'static str, &'static [&'static str])],
}

const TOOLS: &[Tool] = &[
    Tool {
        name: "status",
        title: "RED status",
        description: "Whether redd is running, its version, TLD, ports (HTTP, HTTPS, DNS, SMTP) and where RED keeps its files.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "doctor",
        title: "Check RED's setup",
        description: "Run RED's health checks (DNS, certificates, ports, PHP, Xdebug) and return each check with a fix hint when it fails.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "list_sites",
        title: "List sites",
        description: "Every site RED serves: name, URL, directory, driver (laravel, wordpress, static, …), PHP version and whether it uses HTTPS.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "site_for_path",
        title: "Find the site for a directory",
        description: "The site that serves an absolute directory path (for example the project you are working in), with its URL and PHP version.",
        effect: Effect::Read,
        params: &[(
            "path",
            "string",
            "Absolute path of a project directory or a file inside it.",
            true,
        )],
        enums: &[],
    },
    Tool {
        name: "link_site",
        title: "Link a directory as a site",
        description: "Serve an absolute directory as a site at <name>.<tld>. The name defaults to the directory name.",
        effect: Effect::Change,
        params: &[
            (
                "path",
                "string",
                "Absolute path of the project directory.",
                true,
            ),
            (
                "name",
                "string",
                "Site name (the host name without the TLD).",
                false,
            ),
        ],
        enums: &[],
    },
    Tool {
        name: "secure_site",
        title: "Turn HTTPS on or off for a site",
        description: "Serve a site over HTTPS with a certificate from RED's local CA (secure: true, the default), or over plain HTTP again (secure: false).",
        effect: Effect::Change,
        params: &[
            ("name", "string", "Site name.", true),
            (
                "secure",
                "boolean",
                "true for HTTPS (default), false for HTTP.",
                false,
            ),
        ],
        enums: &[],
    },
    Tool {
        name: "php_versions",
        title: "PHP versions and Xdebug",
        description: "Installed PHP versions, the default version, which sites use which, and the Xdebug mode and IDE port.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "set_xdebug",
        title: "Set the Xdebug mode",
        description: "off: never load Xdebug. auto: only for requests with an Xdebug trigger (XDEBUG_SESSION cookie, XDEBUG_TRIGGER) and `red php` with XDEBUG_TRIGGER=1. on: every request and every `red php` run starts a debug session.",
        effect: Effect::Change,
        params: &[("mode", "string", "off, auto or on.", true)],
        enums: &[("mode", &["off", "auto", "on"])],
    },
    Tool {
        name: "list_logs",
        title: "List logs",
        description: "Log files RED knows about: redd, services, PHP-FPM and each site's application log (for example storage/logs/laravel.log). Use an id with read_log.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "read_log",
        title: "Read a log",
        description: "The last entries of a log from list_logs, parsed into time, level and message where the format is known (Laravel, PHP-FPM).",
        effect: Effect::Read,
        params: &[
            ("id", "string", "Log id from list_logs.", true),
            (
                "lines",
                "integer",
                "How many entries to return (default 50, at most 1000).",
                false,
            ),
        ],
        enums: &[],
    },
    Tool {
        name: "list_mail",
        title: "List caught mail",
        description: "Mail that sites sent to RED's SMTP server, newest first. RED catches it all; nothing is delivered.",
        effect: Effect::Read,
        params: &[(
            "mailbox",
            "string",
            "Only this mailbox (the SMTP user name the app used).",
            false,
        )],
        enums: &[],
    },
    Tool {
        name: "read_mail",
        title: "Read a caught message",
        description: "One caught message: headers, attachments and a plain-text body.",
        effect: Effect::Read,
        params: &[
            ("id", "string", "Message id from list_mail.", true),
            (
                "include_html",
                "boolean",
                "Also return the HTML body.",
                false,
            ),
        ],
        enums: &[],
    },
    Tool {
        name: "clear_mail",
        title: "Delete caught mail",
        description: "Delete all caught mail, or one mailbox's.",
        effect: Effect::Delete,
        params: &[("mailbox", "string", "Only this mailbox.", false)],
        enums: &[],
    },
    Tool {
        name: "list_dumps",
        title: "List dumps",
        description: "Output of dump() and dd() calls in sites, captured when dump capturing is on, as text with the source file and line.",
        effect: Effect::Read,
        params: &[(
            "after_id",
            "integer",
            "Only dumps with a higher id, to see what is new since a previous call.",
            false,
        )],
        enums: &[],
    },
    Tool {
        name: "clear_dumps",
        title: "Delete dumps",
        description: "Delete the captured dumps.",
        effect: Effect::Delete,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "list_services",
        title: "List services",
        description: "Databases and other services RED runs (MySQL, PostgreSQL, Redis/Valkey, …): status, port and the .env variables to connect.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "service_action",
        title: "Start, stop or restart a service",
        description: "Start, stop or restart a service from list_services.",
        effect: Effect::Change,
        params: &[
            ("name", "string", "Service name.", true),
            ("action", "string", "start, stop or restart.", true),
        ],
        enums: &[("action", &["start", "stop", "restart"])],
    },
    Tool {
        name: "list_runtimes",
        title: "List runtimes",
        description: "PHP, Node and service runtimes RED can install, which are installed, and available updates.",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
    Tool {
        name: "list_proxies",
        title: "List proxies",
        description: "Host names RED forwards to another local server (for example a Vite dev server).",
        effect: Effect::Read,
        params: &[],
        enums: &[],
    },
];

fn tool_list() -> Vec<Value> {
    TOOLS.iter().map(tool_json).collect()
}

fn tool_json(tool: &Tool) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for &(name, kind, description, is_required) in tool.params {
        let mut schema = json!({ "type": kind, "description": description });
        if let Some((_, values)) = tool.enums.iter().find(|(n, _)| *n == name) {
            schema["enum"] = json!(values);
        }
        if kind == "integer" {
            schema["minimum"] = json!(0);
        }
        properties.insert(name.to_owned(), schema);
        if is_required {
            required.push(name);
        }
    }
    let mut schema = json!({
        "type": "object",
        "properties": properties,
        "additionalProperties": false,
    });
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    json!({
        "name": tool.name,
        "title": tool.title,
        "description": tool.description,
        "inputSchema": schema,
        "annotations": {
            "title": tool.title,
            "readOnlyHint": tool.effect == Effect::Read,
            "destructiveHint": tool.effect == Effect::Delete,
            "idempotentHint": true,
            "openWorldHint": false,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        // No daemon listens here, so tool calls fail like a stopped redd.
        Server::new(Client::new("/nonexistent/red-test.sock"))
    }

    async fn ask(server: &Server, message: Value) -> Value {
        server
            .handle_line(&message.to_string())
            .await
            .expect("a reply")
    }

    #[tokio::test]
    async fn negotiates_the_protocol_version() {
        let s = server();
        let reply = ask(
            &s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
                "protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"test","version":"1"}}}),
        )
        .await;
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(reply["result"]["serverInfo"]["name"], "red");
        assert!(reply["result"]["capabilities"]["tools"].is_object());

        let reply = ask(
            &s,
            json!({"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        )
        .await;
        assert_eq!(reply["id"], "a");
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
    }

    #[tokio::test]
    async fn ignores_notifications_and_answers_ping() {
        let s = server();
        assert!(
            s.handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .await
                .is_none()
        );
        let reply = ask(&s, json!({"jsonrpc":"2.0","id":7,"method":"ping"})).await;
        assert_eq!(reply["result"], json!({}));
    }

    #[tokio::test]
    async fn reports_protocol_errors() {
        let s = server();
        let reply = s.handle_line("{not json").await.unwrap();
        assert_eq!(reply["error"]["code"], PARSE_ERROR);
        assert_eq!(reply["id"], Value::Null);

        let reply = ask(&s, json!({"jsonrpc":"2.0","id":2,"method":"nope"})).await;
        assert_eq!(reply["error"]["code"], METHOD_NOT_FOUND);

        let reply = ask(
            &s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"rm_rf"}}),
        )
        .await;
        assert_eq!(reply["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn lists_valid_tools() {
        let s = server();
        let reply = ask(&s, json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})).await;
        let tools = reply["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), TOOLS.len());
        let mut names = std::collections::HashSet::new();
        for tool in tools {
            let name = tool["name"].as_str().unwrap();
            assert!(names.insert(name), "duplicate tool {name}");
            assert!(
                name.len() <= 128
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_-.".contains(c))
            );
            assert_eq!(tool["inputSchema"]["type"], "object");
            for req in tool["inputSchema"]["required"]
                .as_array()
                .into_iter()
                .flatten()
            {
                assert!(tool["inputSchema"]["properties"][req.as_str().unwrap()].is_object());
            }
        }
        let xdebug = tools.iter().find(|t| t["name"] == "set_xdebug").unwrap();
        assert_eq!(
            xdebug["inputSchema"]["properties"]["mode"]["enum"],
            json!(["off", "auto", "on"])
        );
        let clear = tools.iter().find(|t| t["name"] == "clear_mail").unwrap();
        assert_eq!(clear["annotations"]["destructiveHint"], true);
        assert_eq!(clear["annotations"]["readOnlyHint"], false);
    }

    #[tokio::test]
    async fn tool_failures_are_tool_errors() {
        let s = server();
        let reply = ask(
            &s,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_sites","arguments":{}}}),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("redd is not running"), "{text}");
        assert!(text.contains("red restart"));

        let reply = ask(
            &s,
            json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"set_xdebug","arguments":{"mode":"maybe"}}}),
        )
        .await;
        assert_eq!(reply["result"]["isError"], true);
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("off, auto or on")
        );

        let reply = ask(
            &s,
            json!({"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"read_log"}}),
        )
        .await;
        assert!(
            reply["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("missing argument `id`")
        );
    }

    #[tokio::test]
    async fn answers_batches() {
        let s = server();
        let reply = s
            .handle_line(
                r#"[{"jsonrpc":"2.0","id":1,"method":"ping"},{"jsonrpc":"2.0","method":"notifications/initialized"}]"#,
            )
            .await
            .unwrap();
        assert_eq!(reply.as_array().unwrap().len(), 1);
    }

    #[test]
    fn wraps_results_as_objects() {
        let r = tool_result(json!([1, 2]));
        assert_eq!(r["structuredContent"], json!({"result": [1, 2]}));
        assert_eq!(r["isError"], false);
        assert_eq!(
            tool_result(Value::Null)["structuredContent"],
            json!({"ok": true})
        );
        let text = r["content"][0]["text"].as_str().unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(text).unwrap(),
            r["structuredContent"]
        );
    }

    #[test]
    fn slims_mail_and_dumps() {
        let mut mail = json!({"text": null, "html": "<p>Hello</p><p>there</p>"});
        slim_mail(&mut mail, false);
        assert_eq!(mail, json!({"text": "Hello\nthere"}));
        let mut mail = json!({"text": "Hi", "html": "<b>Hi</b>"});
        slim_mail(&mut mail, true);
        assert_eq!(mail["html"], "<b>Hi</b>");

        let mut dumps = json!({"enabled": true, "dumps": [
            {"id": 1, "html": "<pre>a</pre>"},
            {"id": 2, "html": "<pre>b</pre>"}
        ]});
        slim_dumps(&mut dumps, 1);
        assert_eq!(dumps["dumps"], json!([{"id": 2, "text": "b"}]));
    }
}
