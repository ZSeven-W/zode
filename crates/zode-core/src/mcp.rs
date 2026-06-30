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

    // FOREIGN config files (other agents' MCP config: cursor / opencode / gemini
    // / claude JSON + codex TOML). zode ADOPTS the ones in the user's HOME by
    // default — a deliberate personal setup, so a user's existing MCP servers
    // "just work" without being re-declared here. Unwanted ones are turned off in
    // `/mcp` (persisted to `plugins.disabled`), the single gate on whether a
    // server connects.
    //
    // PROJECT-LOCAL foreign configs (the same files under `cwd`) are a different
    // trust boundary: they can ride along in an untrusted cloned repo, so they
    // are discovered but default to DISABLED (listed in `/mcp`, never auto-
    // spawned). Opt one in via `/mcp` or by declaring it in zode's own config.
    //
    // We also deliberately do NOT pull MCP out of foreign *plugin trees*
    // (`~/.claude/plugins`, `~/.codex/plugins`, opencode plugins) — those ship
    // product-bundled servers (discord / telegram / computer-use / github / …)
    // that are "built in" to another tool, not the user's own config. This mirrors
    // how foreign plugin *commands* are skipped (see `user_commands::commands_dirs`).
    // `openpencil` is excluded too: op-bridge drives OpenPencil natively (below).
    let mut home_json: Vec<std::path::PathBuf> = Vec::new();
    let mut home_toml: Vec<std::path::PathBuf> = Vec::new();
    if let Some(h) = &home {
        home_json.push(h.join(".cursor").join("mcp.json"));
        home_json.push(h.join(".config").join("opencode").join("opencode.json"));
        home_json.push(h.join(".gemini").join("settings.json"));
        home_json.push(h.join(".claude.json"));
        // codex keeps MCP under [mcp_servers.*] in TOML (also foreign).
        home_toml.push(h.join(".codex").join("config.toml"));
    }
    // Project-local foreign files (untrusted workspace → disabled by default).
    let cwd_json: Vec<std::path::PathBuf> = vec![
        cwd.join(".cursor").join("mcp.json"),
        cwd.join(".vscode").join("mcp.json"),
        cwd.join(".opencode").join("opencode.json"),
    ];
    let cwd_toml: Vec<std::path::PathBuf> = vec![cwd.join(".codex").join("config.toml")];

    // zode's OWN sources (enabled): global → project, `.mcp.json` kept high.
    let mut zode_json: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(global) = ConfigManager::config_dir() {
        zode_json.push(global.join("mcp.json"));
    }
    zode_json.push(cwd.join(".mcp.json"));
    zode_json.push(cwd.join(".zode").join("mcp.json"));

    let mut servers: serde_json::Map<String, Value> = serde_json::Map::new();
    // --- zode's OWN plugin tree is scanned (so zode-shipped plugins can declare
    // MCP servers); foreign plugin trees are intentionally NOT (see note). ---
    if let Ok(global) = ConfigManager::config_dir() {
        collect_plugin_mcp(&global.join("plugins"), &mut servers);
    }
    // --- HOME foreign config files → adopted (enabled by default). ---
    for path in &home_toml {
        if let Ok(s) = std::fs::read_to_string(path) {
            for (name, spec) in parse_codex_mcp_toml(&s) {
                servers.insert(name, spec);
            }
        }
    }
    for path in &home_json {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&s) {
                for (name, spec) in extract_servers(&root) {
                    servers.insert(name, spec);
                }
            }
        }
    }
    // --- PROJECT-LOCAL foreign config files → disabled by default. An untrusted
    // workspace must not auto-spawn processes, AND must not REPLACE the command
    // of a server already adopted from a trusted home config (a hijack vector).
    // So skip any already-adopted name entirely (never overwrite it), insert
    // only workspace-only names, and default those to `enabled:false`. Opt one in
    // via `/mcp` or by declaring it in zode's own config (below, which DOES
    // override). ---
    let adopted: std::collections::HashSet<String> = servers.keys().cloned().collect();
    for path in &cwd_toml {
        if let Ok(s) = std::fs::read_to_string(path) {
            for (name, spec) in parse_codex_mcp_toml(&s) {
                if !adopted.contains(&name) {
                    servers.insert(name, spec);
                }
            }
        }
    }
    for path in &cwd_json {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&s) {
                for (name, spec) in extract_servers(&root) {
                    if !adopted.contains(&name) {
                        servers.insert(name, spec);
                    }
                }
            }
        }
    }
    // Everything NOT already adopted is a workspace-only server → force OFF.
    // Use insert (overwrite), not entry().or_insert: a workspace file that ships
    // its own `"enabled": true` must NOT be able to keep itself enabled.
    for (name, spec) in servers.iter_mut() {
        if adopted.contains(name) {
            continue; // trusted home config / zode plugin — leave enabled
        }
        if let Some(obj) = spec.as_object_mut() {
            obj.insert("enabled".to_string(), Value::Bool(false));
        }
    }
    // --- zode's own config (highest precedence; enabled) overrides any foreign
    // definition — this is how a workspace-only server is opted in. ---
    collect_plugin_mcp(&cwd.join(".zode").join("plugins"), &mut servers);
    for path in &zode_json {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&s) {
                for (name, spec) in extract_servers(&root) {
                    servers.insert(name, spec);
                }
            }
        }
    }
    // op-bridge owns OpenPencil — never surface it as a discovered MCP server,
    // even if a foreign (or zode-own) config declares it.
    servers.remove("openpencil");
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
/// `(name, command, args, env, enabled)`. `command` is optional until its line
/// is seen; `enabled` defaults true and flips on an explicit `enabled = false`.
type PendingMcpServer = (
    String,
    Option<String>,
    Vec<String>,
    serde_json::Map<String, Value>,
    bool,
);

/// Minimal parser for codex's `[mcp_servers.<name>]` TOML sections → zode's
/// stdio server shape. Handles single-line `command = "…"`, `args = [..]`, and
/// inline `env = { K = "v" }`. Other keys / formats are ignored (best-effort;
/// codex MCP entries are simple). Returns (name, spec) pairs.
fn parse_codex_mcp_toml(s: &str) -> Vec<(String, Value)> {
    fn finish(pending: Option<PendingMcpServer>, out: &mut Vec<(String, Value)>) {
        if let Some((name, Some(command), args, env, enabled)) = pending {
            let mut spec = json!({
                "transport": "stdio",
                "command": command,
                "args": args,
                "env": Value::Object(env),
            });
            // Honor an explicit `enabled = false`; otherwise leave it unset so
            // the default-enabled (adopted) policy applies.
            if !enabled {
                spec["enabled"] = Value::Bool(false);
            }
            out.push((name, spec));
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
                        true,
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
                "enabled" => p.4 = v.trim() != "false",
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
            // Both come from PROJECT-LOCAL foreign files (untrusted workspace):
            // the formats parse, but the servers default to DISABLED — a
            // workspace can't auto-spawn, and even oc's explicit `enabled:true`
            // does not override the workspace-off default.
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
            assert!(cfg.servers["shared"].enabled(), "zode override enabled");
            // Prove the zode-own definition (command "z") actually won, not the
            // foreign "x".
            match &cfg.servers["shared"] {
                agent::mcp::McpServerConfig::Stdio { command, .. } => {
                    assert_eq!(command, "z", "zode-own command must override foreign");
                }
                other => panic!("shared should be a stdio server, got {other:?}"),
            }
            // "foreignonly" exists only in the project .cursor → off by default.
            assert!(
                !cfg.servers["foreignonly"].enabled(),
                "workspace-only foreign server is disabled by default"
            );
        });
    }

    #[test]
    #[serial_test::serial]
    fn workspace_cannot_hijack_home_server() {
        with_isolated_home(|| {
            let home = dirs::home_dir().expect("isolated HOME");
            std::env::set_var("ZODE_CONFIG_DIR", home.join("none"));
            // Trusted home server.
            std::fs::write(
                home.join(".claude.json"),
                r#"{"mcpServers":{"deepwiki":{"command":"good"}}}"#,
            )
            .unwrap();
            // Untrusted workspace re-declares the SAME name with a hostile command.
            let proj = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(proj.path().join(".cursor")).unwrap();
            std::fs::write(
                proj.path().join(".cursor").join("mcp.json"),
                r#"{"mcpServers":{"deepwiki":{"command":"evil"}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            // The home definition survives intact: still enabled, command unchanged.
            assert!(cfg.servers["deepwiki"].enabled());
            match &cfg.servers["deepwiki"] {
                agent::mcp::McpServerConfig::Stdio { command, .. } => {
                    assert_eq!(command, "good", "workspace must not replace home command");
                }
                other => panic!("expected stdio, got {other:?}"),
            }
        });
    }

    #[test]
    #[serial_test::serial]
    fn workspace_cannot_self_enable_via_enabled_true() {
        with_isolated_home(|| {
            let home = dirs::home_dir().expect("isolated HOME");
            std::env::set_var("ZODE_CONFIG_DIR", home.join("none"));
            // A workspace file that ships its own enabled:true (in zode's tagged
            // shape, which passes through normalize untouched) must still be off.
            let proj = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(proj.path().join(".vscode")).unwrap();
            std::fs::write(
                proj.path().join(".vscode").join("mcp.json"),
                r#"{"servers":{"sneaky":{"transport":"stdio","command":"x","enabled":true}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(
                !cfg.servers["sneaky"].enabled(),
                "workspace foreign must not self-enable via enabled:true"
            );
        });
    }

    #[test]
    #[serial_test::serial]
    fn openpencil_is_never_surfaced_as_mcp_server() {
        with_isolated_home(|| {
            let proj = tempfile::tempdir().unwrap();
            std::env::set_var("ZODE_CONFIG_DIR", proj.path().join("none"));
            // A foreign config declaring openpencil alongside a normal server.
            std::fs::create_dir_all(proj.path().join(".cursor")).unwrap();
            std::fs::write(
                proj.path().join(".cursor").join("mcp.json"),
                r#"{"mcpServers":{"openpencil":{"command":"op"},"keep":{"command":"x"}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            // op-bridge drives OpenPencil natively — it must not double as MCP.
            assert!(
                !cfg.servers.contains_key("openpencil"),
                "openpencil must not be surfaced as an MCP server"
            );
            assert!(cfg.servers.contains_key("keep"));
        });
    }

    #[test]
    #[serial_test::serial]
    fn foreign_plugin_tree_mcp_is_not_adopted() {
        with_isolated_home(|| {
            let home = dirs::home_dir().expect("isolated HOME");
            std::env::set_var("ZODE_CONFIG_DIR", home.join("none"));
            // A product-bundled MCP server inside a foreign plugin tree…
            let plug = home
                .join(".claude")
                .join("plugins")
                .join("marketplaces")
                .join("official")
                .join("discord");
            std::fs::create_dir_all(&plug).unwrap();
            std::fs::write(
                plug.join(".mcp.json"),
                r#"{"mcpServers":{"discord":{"command":"discord-mcp"}}}"#,
            )
            .unwrap();
            // …and one inside the codex plugin tree (also a foreign tree)…
            let codex_plug = home.join(".codex").join("plugins").join("cache").join("gh");
            std::fs::create_dir_all(&codex_plug).unwrap();
            std::fs::write(
                codex_plug.join(".mcp.json"),
                r#"{"mcpServers":{"ghplugin":{"command":"gh-mcp"}}}"#,
            )
            .unwrap();
            // …and a real user-config server the user actually wants.
            std::fs::write(
                home.join(".claude.json"),
                r#"{"mcpServers":{"deepwiki":{"command":"npx","args":["deepwiki"]}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(&home.join("noproj"));
            std::env::remove_var("ZODE_CONFIG_DIR");
            let cfg = cfg.expect("user-config server present");
            assert!(
                !cfg.servers.contains_key("discord"),
                "claude plugin-tree MCP must not be adopted"
            );
            assert!(
                !cfg.servers.contains_key("ghplugin"),
                "codex plugin-tree MCP must not be adopted"
            );
            assert!(
                cfg.servers["deepwiki"].enabled(),
                "home user-config MCP is adopted (enabled)"
            );
        });
    }

    #[test]
    #[serial_test::serial]
    fn home_foreign_adopted_but_workspace_foreign_disabled() {
        with_isolated_home(|| {
            let home = dirs::home_dir().expect("isolated HOME");
            std::env::set_var("ZODE_CONFIG_DIR", home.join("none"));
            // Home config server → trusted personal setup → adopted.
            std::fs::write(
                home.join(".claude.json"),
                r#"{"mcpServers":{"homesrv":{"command":"npx","args":["home"]}}}"#,
            )
            .unwrap();
            // Same kind of file inside a project → untrusted workspace → off.
            let proj = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(proj.path().join(".cursor")).unwrap();
            std::fs::write(
                proj.path().join(".cursor").join("mcp.json"),
                r#"{"mcpServers":{"worksrv":{"command":"evil"}}}"#,
            )
            .unwrap();
            let cfg = discover_mcp_config(proj.path()).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(cfg.servers["homesrv"].enabled(), "home foreign is adopted");
            assert!(
                !cfg.servers["worksrv"].enabled(),
                "workspace foreign must not auto-spawn"
            );
        });
    }

    #[test]
    #[serial_test::serial]
    fn codex_toml_enabled_false_is_respected() {
        with_isolated_home(|| {
            let home = dirs::home_dir().expect("isolated HOME");
            std::env::set_var("ZODE_CONFIG_DIR", home.join("none"));
            // A home codex server explicitly turned off must NOT be adopted.
            std::fs::create_dir_all(home.join(".codex")).unwrap();
            std::fs::write(
                home.join(".codex").join("config.toml"),
                "[mcp_servers.off]\ncommand = \"x\"\nenabled = false\n\
                 \n[mcp_servers.on]\ncommand = \"y\"\n",
            )
            .unwrap();
            let cfg = discover_mcp_config(&home.join("noproj")).expect("merged");
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(
                !cfg.servers["off"].enabled(),
                "explicit enabled=false honored"
            );
            assert!(cfg.servers["on"].enabled(), "default codex server adopted");
        });
    }
}
