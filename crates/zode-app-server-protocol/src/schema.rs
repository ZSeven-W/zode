use serde_json::{json, Value};

use crate::ClientRequest;

pub const CAPABILITIES: &[&str] = &[
    "threads", "turns", "fs", "command", "models", "config", "skills", "hooks", "mcp", "plugins",
];

#[derive(Debug, Clone, PartialEq)]
pub struct FixtureMessage {
    pub name: &'static str,
    pub value: Value,
}

pub fn supported_methods() -> &'static [&'static str] {
    ClientRequest::ALL
}

pub fn protocol_schema() -> Value {
    json!({
        "name": "zode-app-server",
        "version": env!("CARGO_PKG_VERSION"),
        "envelope": {
            "request": {"required": ["jsonrpc", "id", "method"], "optional": ["params"]},
            "response": {"required": ["jsonrpc", "id", "result"]},
            "error": {"required": ["jsonrpc", "id", "error"]},
            "notification": {"required": ["jsonrpc", "method"], "optional": ["params"]}
        },
        "methods": supported_methods(),
    })
}

pub fn fixture_messages() -> Vec<FixtureMessage> {
    vec![
        FixtureMessage {
            name: "protocol.schema",
            value: protocol_schema(),
        },
        FixtureMessage {
            name: "initialize.request",
            value: json!({
                "jsonrpc": "2.0",
                "id": "init",
                "method": "initialize",
                "params": {
                    "clientInfo": {"name": "fixture", "version": "0.0.0"},
                    "approvalPolicy": "readOnly"
                }
            }),
        },
        FixtureMessage {
            name: "initialize.response",
            value: json!({
                "jsonrpc": "2.0",
                "id": "init",
                "result": {
                    "serverInfo": {"name": "zode", "version": "0.2.0-beta.1"},
                    "zodeHome": "/tmp/zode",
                    "platformFamily": "unix",
                    "platformOs": "macos",
                    "capabilities": CAPABILITIES,
                    "approvalPolicy": "readOnly"
                }
            }),
        },
        FixtureMessage {
            name: "thread-start.request",
            value: json!({
                "jsonrpc": "2.0",
                "id": "thread",
                "method": "thread/start",
                "params": {"cwd": "/tmp/project", "model": "default"}
            }),
        },
        FixtureMessage {
            name: "fs-read-file.request",
            value: json!({
                "jsonrpc": "2.0",
                "id": "read",
                "method": "fs/readFile",
                "params": {"path": "/tmp/project/hello.txt"}
            }),
        },
        FixtureMessage {
            name: "command-exec.request",
            value: json!({
                "jsonrpc": "2.0",
                "id": "cmd",
                "method": "command/exec",
                "params": {"command": ["sh", "-c", "printf hi"]}
            }),
        },
    ]
}
