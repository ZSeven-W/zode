//! Plugin manifest: what a plugin declares (capabilities), persisted as
//! `manifest.json` in the plugin's own install directory so a detail page
//! (a later UI wave) can render capability sections without re-scanning.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::PluginMarketError;

pub const MANIFEST_FILE_NAME: &str = "manifest.json";

/// One capability a plugin's tree declares. Paths are stored relative to the
/// plugin's own root dir so the manifest stays valid if `~/.zode` moves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Capability {
    /// A `SKILL.md` prompt pack. Prompt-only — never trust-gated, see
    /// [`Capability::is_reviewable`].
    Skill { name: String, path: String },
    /// An MCP server declaration (`plugin.json` / `.mcp.json` / `mcp.json`).
    /// `command`/`args` is the exact stdio launch line; for a remote
    /// (url-based) server `command` holds the URL and `args` is empty.
    Mcp {
        name: String,
        command: String,
        args: Vec<String>,
    },
    /// A hook script declared in a `hooks.json` under the plugin tree.
    /// `script` is the plugin-relative path to the script file.
    Hook {
        event: String,
        tool: Option<String>,
        script: String,
    },
}

impl Capability {
    /// Hooks and MCP servers execute code/requests the plugin author
    /// controls — they go through the trust-review gate (`trust.rs`).
    /// Skills are prompt-only text, no gate.
    pub fn is_reviewable(&self) -> bool {
        !matches!(self, Capability::Skill { .. })
    }

    /// Stable key identifying this capability within a plugin; used as the
    /// `trust.json` record key. Two installs of the same plugin at the same
    /// ref produce the same keys, so a grant carries over across a
    /// re-install (as long as nothing about the capability changed).
    pub fn trust_key(&self) -> String {
        match self {
            Capability::Skill { name, .. } => format!("skill:{name}"),
            Capability::Mcp { name, .. } => format!("mcp:{name}"),
            Capability::Hook {
                event,
                tool,
                script,
            } => format!("hook:{event}:{}:{script}", tool.as_deref().unwrap_or("*")),
        }
    }
}

/// What was installed: source, pinned ref, and everything the capability
/// scan found at install (or last re-sync) time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    /// The spec the user installed with (`owner/repo` or a full git URL),
    /// kept verbatim for display and for a later "check for updates" fetch.
    pub repo: String,
    /// The ref this install is pinned to (branch/tag/sha, or `"HEAD"` when
    /// the caller didn't pin one — the default branch at clone time).
    #[serde(rename = "ref")]
    pub reference: String,
    /// Epoch milliseconds.
    pub installed_at: u64,
    pub capabilities: Vec<Capability>,
}

impl PluginManifest {
    pub fn save(&self, plugin_dir: &Path) -> Result<(), PluginMarketError> {
        let path = Self::path_in(plugin_dir);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| PluginMarketError::Manifest(e.to_string()))?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load(plugin_dir: &Path) -> Result<Self, PluginMarketError> {
        let path = Self::path_in(plugin_dir);
        let s = std::fs::read_to_string(path)?;
        serde_json::from_str(&s).map_err(|e| PluginMarketError::Manifest(e.to_string()))
    }

    pub fn path_in(plugin_dir: &Path) -> PathBuf {
        plugin_dir.join(MANIFEST_FILE_NAME)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_capability_is_not_reviewable() {
        let cap = Capability::Skill {
            name: "review".into(),
            path: "skills/review".into(),
        };
        assert!(!cap.is_reviewable());
    }

    #[test]
    fn mcp_and_hook_capabilities_are_reviewable() {
        let mcp = Capability::Mcp {
            name: "srv".into(),
            command: "npx".into(),
            args: vec!["-y".into(), "tool".into()],
        };
        let hook = Capability::Hook {
            event: "before_tool_use".into(),
            tool: Some("Bash".into()),
            script: "hooks/check.sh".into(),
        };
        assert!(mcp.is_reviewable());
        assert!(hook.is_reviewable());
    }

    #[test]
    fn trust_keys_are_stable_and_distinct() {
        let a = Capability::Mcp {
            name: "srv".into(),
            command: "npx".into(),
            args: vec![],
        };
        let b = Capability::Hook {
            event: "before_tool_use".into(),
            tool: None,
            script: "x.sh".into(),
        };
        assert_ne!(a.trust_key(), b.trust_key());
        assert_eq!(a.trust_key(), a.trust_key());
    }

    #[test]
    fn manifest_round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let manifest = PluginManifest {
            repo: "acme/tools".into(),
            reference: "main".into(),
            installed_at: 1_700_000_000_000,
            capabilities: vec![Capability::Skill {
                name: "review".into(),
                path: "skills/review".into(),
            }],
        };
        manifest.save(dir.path()).unwrap();
        let loaded = PluginManifest::load(dir.path()).unwrap();
        assert_eq!(manifest, loaded);
    }

    #[test]
    fn load_missing_manifest_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(PluginManifest::load(dir.path()).is_err());
    }
}
