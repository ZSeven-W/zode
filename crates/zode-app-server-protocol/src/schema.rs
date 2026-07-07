use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
pub struct FixtureMessage {
    pub name: &'static str,
    pub value: Value,
}

pub fn supported_methods() -> &'static [&'static str] {
    &[
        "initialize",
        "thread/start",
        "thread/resume",
        "thread/list",
        "thread/read",
        "thread/delete",
        "thread/name/set",
        "turn/start",
        "fs/readFile",
        "fs/writeFile",
        "fs/createDirectory",
        "fs/getMetadata",
        "fs/readDirectory",
        "fs/remove",
        "fs/copy",
        "command/exec",
        "model/list",
        "config/read",
        "config/list",
        "skills/list",
        "skills/read",
        "hooks/list",
        "mcpServerStatus/list",
        "plugin/list",
    ]
}

pub fn protocol_schema() -> Value {
    json!({
        "name": "zode-app-server",
        "version": env!("CARGO_PKG_VERSION"),
        "envelope": {
            "request": {"required": ["id", "method"], "optional": ["params"]},
            "response": {"required": ["id", "result"]},
            "error": {"required": ["id", "error"]},
            "notification": {"required": ["method"], "optional": ["params"]}
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
                "id": "init",
                "method": "initialize",
                "params": {"clientInfo": {"name": "fixture", "version": "0.0.0"}}
            }),
        },
        FixtureMessage {
            name: "initialize.response",
            value: json!({
                "id": "init",
                "result": {
                    "serverInfo": {"name": "zode", "version": "0.1.0-beta.5"},
                    "zodeHome": "/tmp/zode",
                    "platformFamily": "unix",
                    "platformOs": "macos",
                    "capabilities": ["threads", "turns", "fs", "command", "skills", "mcp"]
                }
            }),
        },
        FixtureMessage {
            name: "thread-start.request",
            value: json!({
                "id": "thread",
                "method": "thread/start",
                "params": {"cwd": "/tmp/project", "model": "default"}
            }),
        },
        FixtureMessage {
            name: "fs-read-file.request",
            value: json!({
                "id": "read",
                "method": "fs/readFile",
                "params": {"path": "/tmp/project/hello.txt"}
            }),
        },
        FixtureMessage {
            name: "command-exec.request",
            value: json!({
                "id": "cmd",
                "method": "command/exec",
                "params": {"command": ["sh", "-c", "printf hi"]}
            }),
        },
    ]
}
