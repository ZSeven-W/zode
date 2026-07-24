//! MCP server discovery for direct cross-agent configuration, Zode-owned
//! configuration, and managed Zode plugins.
//!
//! Other agents' plugin caches are deliberately not scanned.

use std::path::Path;

use agent::mcp::McpConfig;
use serde_json::{json, Value};

use crate::config::ConfigManager;

/// Load and merge direct MCP configuration from Claude Code, Codex, Cursor,
/// opencode, Gemini, and Zode. Home configuration is treated as the user's
/// trusted setup; project-local foreign configuration is discovered disabled.
/// Other agents' installed plugin trees are never scanned.
pub fn discover_mcp_config(cwd: &Path) -> Option<McpConfig> {
    let home = dirs::home_dir();
    let mut home_json = Vec::new();
    let mut home_toml = Vec::new();
    if let Some(home) = &home {
        home_json.push(home.join(".cursor").join("mcp.json"));
        home_json.push(home.join(".config").join("opencode").join("opencode.json"));
        home_json.push(home.join(".gemini").join("settings.json"));
        home_json.push(home.join(".claude.json"));
        home_toml.push(home.join(".codex").join("config.toml"));
    }
    let project_json = [
        cwd.join(".cursor").join("mcp.json"),
        cwd.join(".vscode").join("mcp.json"),
        cwd.join(".opencode").join("opencode.json"),
    ];
    let project_toml = [cwd.join(".codex").join("config.toml")];

    // Zode-owned sources, low → high precedence.
    let mut zode_json: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(global) = ConfigManager::config_dir() {
        zode_json.push(global.join("mcp.json"));
    }
    zode_json.push(cwd.join(".mcp.json"));
    zode_json.push(cwd.join(".zode").join("mcp.json"));

    let mut servers: serde_json::Map<String, Value> = serde_json::Map::new();
    // Zode's own plugin tree and managed plugin registry.
    if let Ok(global) = ConfigManager::config_dir() {
        collect_plugin_mcp(&global.join("plugins"), &mut servers);
    }
    for component in crate::plugin_package::installed_package_configs(
        crate::plugin_package::PackageConfigKind::Mcp,
    ) {
        if let Some(value) = component.load_json() {
            for (name, spec) in extract_servers(&value) {
                servers.insert(name, spec);
            }
        }
    }
    // Direct home configuration is an explicit personal setup and is enabled
    // unless the source itself disables a server.
    for path in &home_toml {
        if let Ok(text) = std::fs::read_to_string(path) {
            for (name, spec) in parse_codex_mcp_toml(&text) {
                servers.insert(name, spec);
            }
        }
    }
    for path in &home_json {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&text) {
                for (name, spec) in extract_servers(&root) {
                    servers.insert(name, spec);
                }
            }
        }
    }

    // Project-local foreign configuration is visible but cannot auto-spawn or
    // replace a trusted home definition with the same name.
    let adopted: std::collections::HashSet<String> = servers.keys().cloned().collect();
    for path in &project_toml {
        if let Ok(text) = std::fs::read_to_string(path) {
            for (name, mut spec) in parse_codex_mcp_toml(&text) {
                if !adopted.contains(&name) {
                    if let Some(object) = spec.as_object_mut() {
                        object.insert("enabled".into(), Value::Bool(false));
                    }
                    servers.insert(name, spec);
                }
            }
        }
    }
    for path in &project_json {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(root) = serde_json::from_str::<Value>(&text) {
                for (name, mut spec) in extract_servers(&root) {
                    if !adopted.contains(&name) {
                        if let Some(object) = spec.as_object_mut() {
                            object.insert("enabled".into(), Value::Bool(false));
                        }
                        servers.insert(name, spec);
                    }
                }
            }
        }
    }

    // Explicit Zode project configuration wins over plugin definitions.
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
    // op-bridge owns OpenPencil — never surface it as an MCP server even if a
    // Zode config or installed plugin declares it.
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

type PendingCodexServer = (
    String,
    Option<String>,
    Vec<String>,
    serde_json::Map<String, Value>,
    bool,
);

/// Parse the simple `[mcp_servers.<name>]` sections used by Codex config.toml.
fn parse_codex_mcp_toml(text: &str) -> Vec<(String, Value)> {
    fn finish(pending: Option<PendingCodexServer>, output: &mut Vec<(String, Value)>) {
        if let Some((name, Some(command), args, env, enabled)) = pending {
            let mut spec = json!({
                "transport": "stdio",
                "command": command,
                "args": args,
                "env": Value::Object(env),
            });
            if !enabled {
                spec["enabled"] = Value::Bool(false);
            }
            output.push((name, spec));
        }
    }

    let mut output = Vec::new();
    let mut pending: Option<PendingCodexServer> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            finish(pending.take(), &mut output);
            if let Some(name) = section.trim().strip_prefix("mcp_servers.") {
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
        let Some(server) = pending.as_mut() else {
            continue;
        };
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "command" => server.1 = Some(value.trim().trim_matches('"').to_string()),
                "args" => server.2 = parse_toml_string_array(value),
                "env" => server.3 = parse_toml_inline_table(value),
                "enabled" => server.4 = value.trim() != "false",
                _ => {}
            }
        }
    }
    finish(pending.take(), &mut output);
    output
}

fn parse_toml_string_array(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_toml_inline_table(value: &str) -> serde_json::Map<String, Value> {
    let mut output = serde_json::Map::new();
    let body = value.trim().trim_start_matches('{').trim_end_matches('}');
    for item in body.split(',') {
        if let Some((key, value)) = item.split_once('=') {
            let key = key.trim().trim_matches('"');
            if !key.is_empty() {
                output.insert(
                    key.to_string(),
                    Value::String(value.trim().trim_matches('"').to_string()),
                );
            }
        }
    }
    output
}

/// Pull a name → server map out of supported MCP JSON shapes and normalize each
/// entry to Zode's transport-tagged shape. Compatible managed plugins may use
/// `mcp`, `mcpServers`, or `servers` as their top-level key.
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
/// already-tagged (passthrough, aliasing `http`/dropping `websocket` — see
/// below); stdio (`command` as string or `[cmd, ...args]`, `env`/`environment`);
/// and remote (`url` → the Streamable HTTP transport).
fn normalize_server(spec: &Value) -> Option<Value> {
    let obj = spec.as_object()?;
    // Explicitly disabled servers are dropped.
    if obj.get("enabled") == Some(&Value::Bool(false)) {
        return None;
    }
    // Already in zode's tagged shape. `http` is zode's user-facing spelling
    // for the Streamable HTTP transport — the vendor parser's wire tag is
    // still `sse` (an rmcp/serde naming leftover), so alias it here rather
    // than exposing that quirk in every config file. `websocket` is accepted
    // by the schema but has no real connector (rmcp 1.5 gates it behind a
    // disabled feature) — drop it rather than register a server that can
    // never connect.
    if let Some(t) = obj.get("transport").and_then(|v| v.as_str()) {
        return match t {
            "websocket" => None,
            "http" => {
                let mut tagged = spec.clone();
                tagged["transport"] = json!("sse");
                Some(tagged)
            }
            _ => Some(spec.clone()),
        };
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
    // remote: url-based → Streamable HTTP (wire tag `sse`, see above).
    if let Some(url) = obj.get("url").and_then(|v| v.as_str()) {
        let headers = obj.get("headers").cloned().unwrap_or_else(|| json!({}));
        return Some(json!({ "transport": "sse", "url": url, "headers": headers }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_only_spec_infers_streamable_http() {
        let spec = json!({ "url": "https://mcp.example.com/mcp" });
        let got = normalize_server(&spec).unwrap();
        assert_eq!(got["transport"], "sse");
        assert_eq!(got["url"], "https://mcp.example.com/mcp");
    }

    #[test]
    fn normalize_aliases_http_tag_to_wire_sse() {
        let spec = json!({
            "transport": "http",
            "url": "https://mcp.example.com/mcp",
            "headers": { "Authorization": "Bearer $TOK" }
        });
        let got = normalize_server(&spec).unwrap();
        assert_eq!(got["transport"], "sse");
        assert_eq!(got["headers"]["Authorization"], "Bearer $TOK");
    }

    #[test]
    fn normalize_passes_through_explicit_sse_tag() {
        let spec = json!({ "transport": "sse", "url": "https://mcp.example.com/mcp" });
        let got = normalize_server(&spec).unwrap();
        assert_eq!(got["transport"], "sse");
    }

    #[test]
    fn normalize_drops_websocket_transport() {
        let spec = json!({ "transport": "websocket", "url": "wss://mcp.example.com" });
        assert!(normalize_server(&spec).is_none());
    }

    /// Run `f` with isolated Zode and home directories.
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
    #[serial_test::serial]
    fn openpencil_is_never_surfaced_as_mcp_server() {
        with_isolated_home(|| {
            let proj = tempfile::tempdir().unwrap();
            std::env::set_var("ZODE_CONFIG_DIR", proj.path().join("none"));
            std::fs::write(
                proj.path().join(".mcp.json"),
                r#"{"servers":{"openpencil":{"transport":"stdio","command":"op"},"keep":{"transport":"stdio","command":"x"}}}"#,
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
    fn direct_foreign_configs_are_imported_but_plugin_trees_are_not() {
        with_isolated_home(|| {
            let home = dirs::home_dir().expect("isolated HOME");
            std::env::set_var("ZODE_CONFIG_DIR", home.join("none"));
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
            std::fs::write(
                home.join(".claude.json"),
                r#"{"mcpServers":{"deepwiki":{"command":"npx","args":["deepwiki"]}}}"#,
            )
            .unwrap();
            std::fs::create_dir_all(home.join(".codex")).unwrap();
            std::fs::write(
                home.join(".codex").join("config.toml"),
                "[mcp_servers.codex]\ncommand = \"codex-mcp\"\n",
            )
            .unwrap();
            let project = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(project.path().join(".cursor")).unwrap();
            std::fs::write(
                project.path().join(".cursor").join("mcp.json"),
                r#"{"mcpServers":{"cursor":{"command":"cursor-mcp"}}}"#,
            )
            .unwrap();

            let cfg = discover_mcp_config(project.path()).expect("direct configs discovered");
            std::env::remove_var("ZODE_CONFIG_DIR");
            assert!(cfg.servers.contains_key("deepwiki"));
            assert!(cfg.servers.contains_key("codex"));
            assert!(cfg.servers.contains_key("cursor"));
            assert!(cfg.servers["deepwiki"].enabled());
            assert!(cfg.servers["codex"].enabled());
            assert!(!cfg.servers["cursor"].enabled());
            assert!(!cfg.servers.contains_key("discord"));
            assert!(!cfg.servers.contains_key("ghplugin"));
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
    fn parses_codex_mcp_toml() {
        let servers = parse_codex_mcp_toml(
            "[mcp_servers.docs]\n\
             command = \"uvx\"\n\
             args = [\"mcp-docs\"]\n\
             env = { API_KEY = \"x\" }\n",
        );
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].0, "docs");
        assert_eq!(servers[0].1["command"], "uvx");
        assert_eq!(servers[0].1["args"][0], "mcp-docs");
        assert_eq!(servers[0].1["env"]["API_KEY"], "x");
    }
}
