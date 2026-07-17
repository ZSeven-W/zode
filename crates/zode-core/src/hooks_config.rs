//! External hook scripts from hooks.json. Each entry maps an event name
//! (+ optional tool filter) to a script. A ConfiguredHookHandler wraps
//! agent's ScriptHookHandler and only fires on a matching event, so a
//! `HookOutcome::Block` from the script blocks the tool.
//!
//! Schema: `{"hooks":[{"event":"before_tool_use","tool":"Bash","script":"~/.zode/hooks/check.sh"}]}`

use std::path::PathBuf;
use std::sync::Arc;

use agent::hook::{HookEvent, HookHandler, HookOutcome, ScriptHookHandler};
use async_trait::async_trait;
use serde::Deserialize;

use crate::config::ConfigManager;

#[derive(Debug, Clone, Deserialize)]
pub struct HookEntry {
    pub event: String,
    #[serde(default)]
    pub tool: Option<String>,
    pub script: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HooksConfig {
    #[serde(default)]
    pub hooks: Vec<HookEntry>,
}

/// Tool name a HookEvent targets, if any (for the optional tool filter).
fn event_tool(ev: &HookEvent) -> Option<&str> {
    match ev {
        HookEvent::BeforeToolUse { tool, .. }
        | HookEvent::AfterToolUse { tool, .. }
        | HookEvent::PostToolUseFailure { tool, .. } => Some(tool),
        _ => None,
    }
}

impl HookEntry {
    pub fn matches(&self, ev: &HookEvent) -> bool {
        // Use agent's canonical name() (covers every variant) rather than a
        // local partial map that could leave a supported event unreachable.
        if self.event != ev.name() {
            return false;
        }
        match &self.tool {
            None => true,
            Some(want) => event_tool(ev) == Some(want.as_str()),
        }
    }
}

/// HookHandler that delegates to a ScriptHookHandler only when the entry
/// matches the event. Block propagates (blocks the tool).
#[derive(Debug)]
pub struct ConfiguredHookHandler {
    entry: HookEntry,
    inner: ScriptHookHandler,
}

impl ConfiguredHookHandler {
    pub fn new(entry: HookEntry) -> Self {
        let path = expand_tilde(&entry.script);
        Self {
            inner: ScriptHookHandler::new(path),
            entry,
        }
    }
}

#[async_trait]
impl HookHandler for ConfiguredHookHandler {
    async fn handle(&self, event: &HookEvent) -> HookOutcome {
        if self.entry.matches(event) {
            self.inner.handle(event).await
        } else {
            HookOutcome::Ok
        }
    }
}

fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(p)
}

/// Load the raw hook entries from hooks.json (global ⊕ project), for display
/// (`/hooks`) and for building handlers.
pub fn load_hook_entries(cwd: &std::path::Path) -> Vec<HookEntry> {
    let mut entries: Vec<HookEntry> = Vec::new();
    let candidates = [
        ConfigManager::config_dir()
            .ok()
            .map(|d| d.join("hooks.json")),
        Some(cwd.join(".zode").join("hooks.json")),
    ];
    for path in candidates.into_iter().flatten() {
        if let Ok(s) = std::fs::read_to_string(&path) {
            match serde_json::from_str::<HooksConfig>(&s) {
                Ok(cfg) => entries.extend(cfg.hooks),
                Err(e) => tracing::warn!("skip hooks {}: {e}", path.display()),
            }
        }
    }
    for component in crate::plugin_package::installed_package_configs(
        crate::plugin_package::PackageConfigKind::Hooks,
    ) {
        let Some(value) = component.load_json() else {
            continue;
        };
        match serde_json::from_value::<HooksConfig>(value) {
            Ok(mut cfg) => {
                let data = ConfigManager::config_dir()
                    .unwrap_or_else(|_| PathBuf::from(".zode"))
                    .join("plugin-data")
                    .join(&component.plugin);
                for entry in &mut cfg.hooks {
                    entry.script = substitute_plugin_path(&entry.script, &component.root, &data);
                }
                entries.extend(cfg.hooks);
            }
            Err(error) => tracing::warn!(
                plugin = %component.plugin,
                "skip plugin hooks: {error}"
            ),
        }
    }
    entries
}

fn substitute_plugin_path(script: &str, root: &std::path::Path, data: &std::path::Path) -> String {
    let root_text = root.display().to_string();
    let data_text = data.display().to_string();
    let expanded = script
        .replace("${ZODE_PLUGIN_ROOT}", &root_text)
        .replace("${CLAUDE_PLUGIN_ROOT}", &root_text)
        .replace("${GROK_PLUGIN_ROOT}", &root_text)
        .replace("${ZODE_PLUGIN_DATA}", &data_text)
        .replace("${CLAUDE_PLUGIN_DATA}", &data_text)
        .replace("${GROK_PLUGIN_DATA}", &data_text);
    let path = std::path::Path::new(&expanded);
    if path.is_absolute() || expanded.starts_with("~/") {
        expanded
    } else {
        root.join(path).display().to_string()
    }
}

/// Load hooks.json (global ⊕ project) into handlers ready to register.
pub fn load_hook_handlers(cwd: &std::path::Path) -> Vec<Arc<dyn HookHandler>> {
    load_hook_entries(cwd)
        .into_iter()
        .map(|e| Arc::new(ConfiguredHookHandler::new(e)) as Arc<dyn HookHandler>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hooks_config() {
        let json = r#"{"hooks":[
            {"event":"before_tool_use","tool":"Bash","script":"/x/check.sh"},
            {"event":"on_session_start","script":"/x/start.sh"}
        ]}"#;
        let cfg: HooksConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.hooks.len(), 2);
        assert_eq!(cfg.hooks[0].tool.as_deref(), Some("Bash"));
        assert!(cfg.hooks[1].tool.is_none());
    }

    #[test]
    fn entry_matches_event_and_tool_filter() {
        let entry = HookEntry {
            event: "before_tool_use".into(),
            tool: Some("Bash".into()),
            script: "/x".into(),
        };
        assert!(entry.matches(&HookEvent::BeforeToolUse {
            tool: "Bash".into(),
            input: serde_json::json!({}),
        }));
        // Wrong tool.
        assert!(!entry.matches(&HookEvent::BeforeToolUse {
            tool: "FileRead".into(),
            input: serde_json::json!({}),
        }));
        // Wrong event.
        assert!(!entry.matches(&HookEvent::OnSessionStart));
    }

    #[test]
    fn no_tool_filter_matches_any_tool() {
        let entry = HookEntry {
            event: "after_tool_use".into(),
            tool: None,
            script: "/x".into(),
        };
        assert!(entry.matches(&HookEvent::AfterToolUse {
            tool: "anything".into(),
            input: serde_json::json!({}),
            output: serde_json::json!({}),
            ok: true,
        }));
    }

    #[test]
    fn expand_tilde_resolves_home() {
        let p = expand_tilde("~/x/y.sh");
        assert!(p.is_absolute());
        assert!(p.ends_with("x/y.sh"));
        assert_eq!(expand_tilde("/abs/p.sh"), PathBuf::from("/abs/p.sh"));
    }
}
