//! MCP integration. agent-rs provides Lifecycle + registry but no Tool
//! adapter, so each discovered MCP tool is wrapped in a ZodeMcpTool that
//! routes through Lifecycle::call_tool (master §4.6②). Connection happens at
//! assembly, bounded by a per-server timeout so a hung server can't block the
//! launch; tools are registered under `mcp__<server>__<tool>`.

use std::path::Path;
use std::sync::Arc;

use agent::error::AgentError;
use agent::mcp::{Lifecycle, McpConfig, McpRegistry, RmcpConnector};
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::ConfigManager;

pub fn prefixed_tool_name(server: &str, tool: &str) -> String {
    format!("mcp__{server}__{tool}")
}

/// Load + merge MCP servers across the agent ecosystem (claude / cursor /
/// vscode / opencode / codex-style JSON), low → high precedence so zode's own
/// config always wins a same-name clash. Each file is normalized into zode's
/// `{servers:{…transport…}}` shape by [`extract_servers`], so heterogeneous
/// formats (`mcpServers` / `mcp` / `servers`, string-or-array `command`,
/// `env`/`environment`) all merge. Unparseable files are skipped.
pub fn discover_mcp_config(cwd: &Path) -> Option<McpConfig> {
    let home = dirs::home_dir();

    // FOREIGN sources (other agents' configs + plugin trees). These are
    // discovered for compatibility but default to DISABLED, so a machine full
    // of claude/codex/cursor MCP servers doesn't auto-connect dozens of them or
    // flood the slash palette. The user enables the wanted ones by adding them
    // to zode's own config; `/mcp` lists them either way.
    let mut foreign_json: Vec<std::path::PathBuf> = Vec::new();
    if let Some(h) = &home {
        foreign_json.push(h.join(".cursor").join("mcp.json"));
        foreign_json.push(h.join(".config").join("opencode").join("opencode.json"));
        foreign_json.push(h.join(".gemini").join("settings.json"));
        foreign_json.push(h.join(".claude.json"));
    }
    foreign_json.push(cwd.join(".cursor").join("mcp.json"));
    foreign_json.push(cwd.join(".vscode").join("mcp.json"));
    foreign_json.push(cwd.join(".opencode").join("opencode.json"));

    // codex keeps MCP under [mcp_servers.*] in TOML (also foreign).
    let mut toml_paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(h) = &home {
        toml_paths.push(h.join(".codex").join("config.toml"));
    }
    toml_paths.push(cwd.join(".codex").join("config.toml"));

    // zode's OWN sources (enabled): global → project, `.mcp.json` kept high.
    let mut zode_json: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(global) = ConfigManager::config_dir() {
        zode_json.push(global.join("mcp.json"));
    }
    zode_json.push(cwd.join(".mcp.json"));
    zode_json.push(cwd.join(".zode").join("mcp.json"));

    let mut servers: serde_json::Map<String, Value> = serde_json::Map::new();
    // --- Foreign: plugin trees, codex TOML, then foreign JSON ---
    if let Some(h) = &home {
        for root in [
            h.join(".claude").join("plugins"),
            h.join(".codex").join("plugins"),
            h.join(".config").join("opencode").join("plugin"),
        ] {
            collect_plugin_mcp(&root, &mut servers);
        }
    }
    if let Ok(global) = ConfigManager::config_dir() {
        collect_plugin_mcp(&global.join("plugins"), &mut servers);
    }
    for path in &toml_paths {
        if let Ok(s) = std::fs::read_to_string(path) {
            for (name, spec) in parse_codex_mcp_toml(&s) {
                servers.insert(name, spec);
            }
        }
    }
    for path in &foreign_json {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&s) {
                for (name, spec) in extract_servers(&root) {
                    servers.insert(name, spec);
                }
            }
        }
    }
    // Everything collected so far is foreign → default it OFF.
    for spec in servers.values_mut() {
        if let Some(obj) = spec.as_object_mut() {
            obj.insert("enabled".into(), Value::Bool(false));
        }
    }
    // --- zode's own (highest precedence; enabled) override any foreign ---
    collect_plugin_mcp(&cwd.join(".zode").join("plugins"), &mut servers);
    for path in &zode_json {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&s) {
                for (name, spec) in extract_servers(&root) {
                    servers.insert(name, spec); // re-enabled (default true)
                }
            }
        }
    }
    if servers.is_empty() {
        return None;
    }
    let normalized = json!({ "servers": servers });
    match agent::mcp::parse_json_str(&normalized.to_string()) {
        Ok(cfg) => Some(cfg).filter(|c| !c.servers.is_empty()),
        Err(e) => {
            tracing::warn!("merged mcp config failed to parse: {e}");
            None
        }
    }
}

/// Scan a plugin tree (depth ≤6) for MCP declarations — `plugin.json`,
/// `.mcp.json`, or `mcp.json` — and merge any servers into `out`. Best-effort:
/// unreadable/unparseable files are skipped. Lets plugins ship MCP servers.
fn collect_plugin_mcp(root: &Path, out: &mut serde_json::Map<String, Value>) {
    fn walk(dir: &Path, depth: usize, out: &mut serde_json::Map<String, Value>) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, depth + 1, out);
            } else if matches!(
                p.file_name().and_then(|n| n.to_str()),
                Some("plugin.json" | ".mcp.json" | "mcp.json")
            ) {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(root) = serde_json::from_str::<Value>(&s) {
                        for (name, spec) in extract_servers(&root) {
                            out.insert(name, spec);
                        }
                    }
                }
            }
        }
    }
    walk(root, 0, out);
}

/// The fields accumulated for one `[mcp_servers.<name>]` section while parsing:
/// `(name, command, args, env)`. `command` is optional until its line is seen.
type PendingMcpServer = (
    String,
    Option<String>,
    Vec<String>,
    serde_json::Map<String, Value>,
);

/// Minimal parser for codex's `[mcp_servers.<name>]` TOML sections → zode's
/// stdio server shape. Handles single-line `command = "…"`, `args = [..]`, and
/// inline `env = { K = "v" }`. Other keys / formats are ignored (best-effort;
/// codex MCP entries are simple). Returns (name, spec) pairs.
fn parse_codex_mcp_toml(s: &str) -> Vec<(String, Value)> {
    fn finish(pending: Option<PendingMcpServer>, out: &mut Vec<(String, Value)>) {
        if let Some((name, Some(command), args, env)) = pending {
            out.push((
                name,
                json!({
                    "transport": "stdio",
                    "command": command,
                    "args": args,
                    "env": Value::Object(env),
                }),
            ));
        }
    }

    let mut out = Vec::new();
    let mut pending: Option<PendingMcpServer> = None;
    for raw in s.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(inner) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            finish(pending.take(), &mut out);
            // `[mcp_servers.NAME]` — ignore deeper subsections (e.g. `.env`).
            if let Some(name) = inner.trim().strip_prefix("mcp_servers.") {
                if !name.contains('.') {
                    pending = Some((
                        name.trim().trim_matches('"').to_string(),
                        None,
                        Vec::new(),
                        serde_json::Map::new(),
                    ));
                }
            }
            continue;
        }
        let Some(p) = pending.as_mut() else {
            continue;
        };
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "command" => p.1 = Some(v.trim().trim_matches('"').to_string()),
                "args" => p.2 = toml_str_array(v.trim()),
                "env" => p.3 = toml_inline_table(v.trim()),
                _ => {}
            }
        }
    }
    finish(pending.take(), &mut out);
    out
}

/// Parse a single-line TOML string array: `["a", "b"]` → ["a","b"].
fn toml_str_array(v: &str) -> Vec<String> {
    v.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a single-line TOML inline table: `{ K = "v", K2 = "w" }` → JSON map.
fn toml_inline_table(v: &str) -> serde_json::Map<String, Value> {
    let mut map = serde_json::Map::new();
    let body = v.trim_start_matches('{').trim_end_matches('}');
    for pair in body.split(',') {
        if let Some((k, val)) = pair.split_once('=') {
            let k = k.trim().trim_matches('"');
            if !k.is_empty() {
                map.insert(
                    k.to_string(),
                    Value::String(val.trim().trim_matches('"').to_string()),
                );
            }
        }
    }
    map
}

/// Pull a name → server map out of any agent's MCP JSON and normalize each
/// entry to zode's transport-tagged shape. Accepts the `mcp` (opencode),
/// `mcpServers` (claude/cursor), or `servers` (zode/vscode) top-level key.
fn extract_servers(root: &Value) -> Vec<(String, Value)> {
    let map = root
        .get("mcp")
        .or_else(|| root.get("mcpServers"))
        .or_else(|| root.get("servers"))
        .and_then(|v| v.as_object());
    let Some(map) = map else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(name, spec)| normalize_server(spec).map(|s| (name.clone(), s)))
        .collect()
}

/// Normalize one server spec to zode's `{transport, …}` shape. Handles:
/// already-tagged (passthrough); stdio (`command` as string or `[cmd, ...args]`,
/// `env`/`environment`); and remote (`url`, `type` remote/sse/http → sse).
fn normalize_server(spec: &Value) -> Option<Value> {
    let obj = spec.as_object()?;
    // Explicitly disabled servers are dropped.
    if obj.get("enabled") == Some(&Value::Bool(false)) {
        return None;
    }
    // Already in zode's tagged shape — pass through.
    if obj.contains_key("transport") {
        return Some(spec.clone());
    }
    // stdio: command is a string (claude) or an array (opencode local).
    if let Some(cmd) = obj.get("command") {
        let (command, mut args) = match cmd {
            Value::String(s) => (s.clone(), Vec::new()),
            Value::Array(a) => {
                let mut it = a.iter().filter_map(|v| v.as_str().map(String::from));
                let command = it.next()?;
                (command, it.collect::<Vec<_>>())
            }
            _ => return None,
        };
        if let Some(extra) = obj.get("args").and_then(|v| v.as_array()) {
            args.extend(extra.iter().filter_map(|v| v.as_str().map(String::from)));
        }
        let env = obj
            .get("env")
            .or_else(|| obj.get("environment"))
            .cloned()
            .unwrap_or_else(|| json!({}));
        return Some(json!({
            "transport": "stdio",
            "command": command,
            "args": args,
            "env": env,
        }));
    }
    // remote: url-based → sse (zode supports stdio/sse/websocket).
    if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
        let headers = obj.get("headers").cloned().unwrap_or_else(|| json!({}));
        return Some(json!({ "transport": "sse", "url": url, "headers": headers }));
    }
    None
}

/// Per-server connect timeout — a hung/unreachable server must never block
/// the whole launch.
const CONNECT_TIMEOUT_SECS: u64 = 10;

/// Connect all servers in `config` (concurrently, each bounded by a timeout)
/// and return the live Lifecycle. Failures/timeouts are logged and skipped;
/// the lifecycle is returned regardless so /mcp can report state and the
/// session starts without the unreachable server.
pub async fn connect(config: McpConfig) -> Arc<Lifecycle> {
    let registry = McpRegistry::new();
    // Register every server (so `/mcp` can list disabled ones), but only
    // CONNECT the enabled ones — foreign/cross-agent servers default to
    // disabled, so a machine full of them doesn't auto-connect dozens.
    let mut names: Vec<String> = Vec::new();
    for (name, server_cfg) in config.servers {
        if server_cfg.enabled() {
            names.push(name.clone());
        }
        registry.upsert(name, server_cfg);
    }
    let lifecycle = Arc::new(Lifecycle::new(registry, Arc::new(RmcpConnector::new())));

    let timeout = std::time::Duration::from_secs(CONNECT_TIMEOUT_SECS);
    let connects = names.into_iter().map(|name| {
        let lc = lifecycle.clone();
        async move {
            // A failed connect is expected for cross-agent-discovered servers
            // (not installed / need another env). The `/mcp` dialog shows live
            // connection state, so log at debug to avoid spamming every launch.
            match tokio::time::timeout(timeout, lc.connect(&name)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => tracing::debug!("mcp server {name} failed to connect: {e}"),
                Err(_) => {
                    tracing::debug!(
                        "mcp server {name} connect timed out after {CONNECT_TIMEOUT_SECS}s"
                    )
                }
            }
        }
    });
    futures::future::join_all(connects).await;
    lifecycle
}

/// Build a ZodeMcpTool for every tool discovered on a connected server.
pub fn mcp_tools(lifecycle: &Arc<Lifecycle>) -> Vec<Arc<dyn Tool>> {
    let mut out: Vec<Arc<dyn Tool>> = Vec::new();
    for server in lifecycle.registry.snapshot() {
        for tool in server.state.tool_names() {
            out.push(Arc::new(ZodeMcpTool::new(
                lifecycle.clone(),
                server.name.clone(),
                tool.clone(),
            )));
        }
    }
    out
}

/// A single MCP tool surfaced as an agent Tool. The Connection trait does
/// not expose a per-tool JSON schema, so we advertise a permissive object
/// schema (agent-rs gap) — the server validates the real shape.
#[derive(Debug)]
pub struct ZodeMcpTool {
    lifecycle: Arc<Lifecycle>,
    server: String,
    tool: String,
    display_name: String,
}

impl ZodeMcpTool {
    pub fn new(lifecycle: Arc<Lifecycle>, server: String, tool: String) -> Self {
        let display_name = prefixed_tool_name(&server, &tool);
        Self {
            lifecycle,
            server,
            tool,
            display_name,
        }
    }
}

#[async_trait]
impl Tool for ZodeMcpTool {
    fn name(&self) -> &str {
        &self.display_name
    }
    fn description(&self) -> &str {
        "MCP server tool."
    }
    fn input_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": true})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Unknown // gated by default (unknown side effects)
    }
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        self.lifecycle
            .call_tool(&self.server, &self.tool, input)
            .await
            .map_err(|e| AgentError::other(format!("mcp {}: {e}", self.server)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixed_name() {
        assert_eq!(prefixed_tool_name("deepwiki", "ask"), "mcp__deepwiki__ask");
    }

    /// Run `f` with HOME pointed at an empty temp dir so cross-agent global
    /// configs (~/.claude.json, ~/.cursor/...) don't leak into the assertion.
    fn with_isolated_home(f: impl FnOnce()) {
        let fake = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        std::env::set_var("HOME", fake.path());
        f();
        match prev {
            Some(h) => std::env::set_var("HOME", h),
            None => std::env::remove_var("HOME"),
        }
    }

    #[test]
    #[serial_test::serial]
    fn discover_merges_project_over_global() {
        with_isolated_home(|| {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_var("ZODE_CONFIG_DIR", dir.path());
            // agent's McpConfig uses {"servers": {name: {transport, ...}}}.
            std::fs::write(
                dir.path().join("mcp.json"),
                r#"{"servers":{"a":{"transport":"stdio","command":"echo"}}}"#,
            )
            .unwrap();
            let proj = tempfile::tempdir().unwrap();
            std::fs::write(
                proj.path().join(".mcp.json"),
                r#"{"servers":{"b":{"transport":"stdio","command":"cat"}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).unwrap();
            std::env::remove_var("ZODE_CONFIG_DIR");
            // Global `a` + project `b` are both present after the merge.
            assert!(
                cfg.servers.contains_key("a"),
                "servers: {:?}",
                cfg.servers.keys().collect::<Vec<_>>()
            );
            assert!(cfg.servers.contains_key("b"));
        });
    }

    #[test]
    #[serial_test::serial]
    fn discover_none_when_absent() {
        with_isolated_home(|| {
            let dir = tempfile::tempdir().unwrap();
            std::env::set_var("ZODE_CONFIG_DIR", dir.path().join("none"));
            let cfg = discover_mcp_config(&dir.path().join("noproj"));
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(cfg.is_none());
        });
    }

    #[test]
    fn parses_codex_mcp_toml() {
        let toml = "\
[mcp_servers.everything]\n\
command = \"npx\"\n\
args = [\"-y\", \"@modelcontextprotocol/server-everything\"]\n\
\n\
[mcp_servers.docs]\n\
command = \"uvx\"\n\
args = [\"mcp-docs\"]\n\
env = { API_KEY = \"x\" }\n\
\n\
[other_section]\n\
foo = \"bar\"\n";
        let got = parse_codex_mcp_toml(toml);
        assert_eq!(got.len(), 2);
        let docs = got.iter().find(|(n, _)| n == "docs").expect("docs");
        assert_eq!(docs.1["transport"], "stdio");
        assert_eq!(docs.1["command"], "uvx");
        assert_eq!(docs.1["args"][0], "mcp-docs");
        assert_eq!(docs.1["env"]["API_KEY"], "x");
    }

    #[test]
    #[serial_test::serial]
    fn discover_adapts_claude_and_opencode_formats() {
        with_isolated_home(|| {
            let proj = tempfile::tempdir().unwrap();
            std::env::set_var("ZODE_CONFIG_DIR", proj.path().join("none"));
            // Claude-style mcpServers (string command + args).
            std::fs::create_dir_all(proj.path().join(".cursor")).unwrap();
            std::fs::write(
                proj.path().join(".cursor").join("mcp.json"),
                r#"{"mcpServers":{"gh":{"command":"npx","args":["-y","srv"],"env":{"T":"1"}}}}"#,
            )
            .unwrap();
            // opencode-style mcp (array command, type local).
            std::fs::create_dir_all(proj.path().join(".opencode")).unwrap();
            std::fs::write(
                proj.path().join(".opencode").join("opencode.json"),
                r#"{"mcp":{"oc":{"type":"local","command":["uvx","tool"],"enabled":true}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(
                cfg.servers.contains_key("gh"),
                "{:?}",
                cfg.servers.keys().collect::<Vec<_>>()
            );
            assert!(cfg.servers.contains_key("oc"));
            // Foreign sources are discovered but DISABLED by default (no flood).
            assert!(!cfg.servers["gh"].enabled());
            assert!(!cfg.servers["oc"].enabled());
        });
    }

    #[test]
    #[serial_test::serial]
    fn zode_own_servers_stay_enabled_and_override_foreign() {
        with_isolated_home(|| {
            let proj = tempfile::tempdir().unwrap();
            std::env::set_var("ZODE_CONFIG_DIR", proj.path().join("none"));
            // Foreign cursor server "shared" — would be disabled…
            std::fs::create_dir_all(proj.path().join(".cursor")).unwrap();
            std::fs::write(
                proj.path().join(".cursor").join("mcp.json"),
                r#"{"mcpServers":{"shared":{"command":"x"},"foreignonly":{"command":"y"}}}"#,
            )
            .unwrap();
            // …but zode's own .mcp.json re-defines "shared" + adds "own".
            std::fs::write(
                proj.path().join(".mcp.json"),
                r#"{"servers":{"shared":{"transport":"stdio","command":"z"},"own":{"transport":"stdio","command":"w"}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(cfg.servers["own"].enabled(), "zode-own enabled");
            assert!(cfg.servers["shared"].enabled(), "zode override re-enables");
            assert!(
                !cfg.servers["foreignonly"].enabled(),
                "foreign-only disabled"
            );
        });
    }
}
