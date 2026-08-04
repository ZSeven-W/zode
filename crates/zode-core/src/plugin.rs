//! Unified plugin model. A "plugin" is any toggleable capability provider —
//! a built-in tool group, an MCP server, a skill, or an LSP server. The
//! `/plugin` command and picker list them and flip them on/off; the engine
//! consults the manager during assembly so a disabled plugin's tools aren't
//! registered, its MCP server isn't connected, and its skill isn't loaded.
//!
//! Enable/disable state persists in `ZodeConfig.plugins.disabled` (a list of
//! plugin ids). Default is enabled, so the list holds only the off ones.

use std::collections::HashSet;

use crate::config::ZodeConfig;

/// What a plugin provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    /// An installed plugin package (`zode plugin install`), which may bundle
    /// skills, commands, agents, hooks, MCP/LSP servers, and UI renderers.
    /// Unlike the other kinds its on/off state lives in the install registry,
    /// not in `plugins.disabled`.
    Package,
    /// A category of built-in tools (filesystem, shell, git, …).
    Tools,
    /// An MCP server (its tools are surfaced as `mcp__<server>__<tool>`).
    Mcp,
    /// A skill (a `SKILL.md` instruction pack).
    Skill,
    /// A language server, surfaced as the `lsp_*` tools.
    Lsp,
}

impl PluginKind {
    pub fn prefix(self) -> &'static str {
        match self {
            PluginKind::Package => "plugin",
            PluginKind::Tools => "tools",
            PluginKind::Mcp => "mcp",
            PluginKind::Skill => "skill",
            PluginKind::Lsp => "lsp",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PluginKind::Package => "plugin package",
            PluginKind::Tools => "tool group",
            PluginKind::Mcp => "MCP server",
            PluginKind::Skill => "skill",
            PluginKind::Lsp => "LSP server",
        }
    }
}

/// One entry in the plugin picker.
#[derive(Debug, Clone)]
pub struct Plugin {
    /// Stable id: `<kind-prefix>:<name>`, e.g. `tools:git`, `mcp:deepwiki`.
    pub id: String,
    pub kind: PluginKind,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    /// One-line status detail (member count, connection state, …).
    pub detail: String,
}

/// An installed plugin package, flattened for the picker. Its `enabled` flag
/// comes from the install registry (`plugin_package::installed_package_entries`),
/// which is the sole authority — `plugins.disabled` never governs a package.
#[derive(Debug, Clone)]
pub struct PackageEntry {
    pub name: String,
    /// Manifest description, or a components summary when it has none.
    pub description: String,
    /// One-line detail: the version, or `plugin` when unversioned.
    pub detail: String,
    pub enabled: bool,
}

/// Built-in tool categories: group name → member tool names. Tools not in any
/// group (Skill, ToolSearch, and `mcp__*`) are always on and not toggleable.
pub const TOOL_GROUPS: &[(&str, &str, &[&str])] = &[
    (
        "filesystem",
        "Read, write, edit, list, move, and remove files",
        &[
            "FileRead",
            "FileWrite",
            "FileEdit",
            "ListDir",
            "Mkdir",
            "Move",
            "Remove",
        ],
    ),
    (
        "search",
        "Find files and search file contents",
        &["Glob", "Grep"],
    ),
    (
        "shell",
        "Run shell commands (foreground and background)",
        &["Bash", "BashRun", "BashOutput", "KillShell"],
    ),
    (
        "git",
        "Inspect and operate on the git repository",
        &[
            "GitStatus",
            "GitDiff",
            "GitLog",
            "GitCommit",
            "GitBranch",
            "GitCheckout",
            "GitStash",
            "GitBlame",
            "GitWorktree",
        ],
    ),
    (
        "web",
        "Fetch URLs and search the web",
        &["WebFetch", "WebSearch"],
    ),
    ("notebook", "Edit Jupyter notebooks", &["NotebookEdit"]),
    ("todo", "Track a task list across the turn", &["TodoWrite"]),
    (
        "subagent",
        "Delegate sub-tasks to child agents (Task)",
        &["Task"],
    ),
    (
        "op",
        "Drive a live OpenPencil design (read + write + generate)",
        &["OpRead", "OpWrite", "OpDesign"],
    ),
    (
        "browser",
        "Built-in browser control (navigate, screenshot, click, evaluate)",
        &[
            "BrowserRead",
            "BrowserAct",
            "BrowserEval",
            "BrowserTabs",
            "BrowserUpload",
        ],
    ),
    (
        "team",
        "Hire and coordinate a team of agents (persistent teammates + board)",
        &[
            "TeamHire",
            "TeamSend",
            "TeamDismiss",
            "TeamList",
            "TeamBoardRead",
            "TeamBoardUpdate",
            "TeamBoardAppend",
            "TeamClaim",
            "TeamRelease",
        ],
    ),
];

/// The tool group a tool belongs to, if any. `None` → always-on (Skill,
/// ToolSearch, MCP tools).
pub fn group_of(tool_name: &str) -> Option<&'static str> {
    for (group, _desc, members) in TOOL_GROUPS {
        if members.contains(&tool_name) {
            return Some(group);
        }
    }
    None
}

/// Manages plugin enable/disable state. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct PluginManager {
    /// Plugin ids that are turned OFF (default is on).
    disabled: HashSet<String>,
}

impl PluginManager {
    pub fn from_config(cfg: &ZodeConfig) -> Self {
        Self {
            disabled: cfg.plugins.disabled.iter().cloned().collect(),
        }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id)
    }

    /// Whether a built-in tool should be registered. Tools outside every group
    /// (Skill, ToolSearch, MCP) are always enabled.
    pub fn tool_enabled(&self, tool_name: &str) -> bool {
        match group_of(tool_name) {
            Some(group) => self.is_enabled(&format!("tools:{group}")),
            None => true,
        }
    }

    /// Whether an MCP server (by name) should be connected.
    pub fn mcp_enabled(&self, server: &str) -> bool {
        self.is_enabled(&format!("mcp:{server}"))
    }

    /// Whether a skill (by name) should be loaded.
    pub fn skill_enabled(&self, name: &str) -> bool {
        self.is_enabled(&format!("skill:{name}"))
    }

    /// Whether an LSP server (by language key) should run.
    pub fn lsp_enabled(&self, lang: &str) -> bool {
        self.is_enabled(&format!("lsp:{lang}"))
    }

    /// Flip a plugin. Returns the new enabled state.
    pub fn toggle(&mut self, id: &str) -> bool {
        if self.disabled.remove(id) {
            true
        } else {
            self.disabled.insert(id.to_string());
            false
        }
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.to_string());
        }
    }

    /// The disabled ids, sorted — for persisting back to config.
    pub fn disabled_ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.disabled.iter().cloned().collect();
        v.sort();
        v
    }

    /// Enumerate every plugin given what was discovered at assembly: installed
    /// packages, MCP servers (name, connected), skills (name, description), LSP
    /// servers (language key). Tool groups are always present.
    pub fn list(
        &self,
        packages: &[PackageEntry],
        mcp_servers: &[(String, bool)],
        skills: &[(String, String)],
        lsp_servers: &[String],
    ) -> Vec<Plugin> {
        let mut out = Vec::new();
        for pkg in packages {
            out.push(Plugin {
                id: format!("plugin:{}", pkg.name),
                kind: PluginKind::Package,
                name: pkg.name.clone(),
                description: pkg.description.clone(),
                // Registry-backed: `plugins.disabled` has no say here.
                enabled: pkg.enabled,
                detail: pkg.detail.clone(),
            });
        }
        for (group, desc, members) in TOOL_GROUPS {
            let id = format!("tools:{group}");
            out.push(Plugin {
                enabled: self.is_enabled(&id),
                id,
                kind: PluginKind::Tools,
                name: group.to_string(),
                description: desc.to_string(),
                detail: format!("{} tools", members.len()),
            });
        }
        for (server, connected) in mcp_servers {
            let id = format!("mcp:{server}");
            out.push(Plugin {
                enabled: self.is_enabled(&id),
                id,
                kind: PluginKind::Mcp,
                name: server.clone(),
                description: "MCP server".to_string(),
                detail: if *connected {
                    "connected"
                } else {
                    "not connected"
                }
                .to_string(),
            });
        }
        for (name, desc) in skills {
            let id = format!("skill:{name}");
            out.push(Plugin {
                enabled: self.is_enabled(&id),
                id,
                kind: PluginKind::Skill,
                name: name.clone(),
                description: desc.clone(),
                detail: "skill".to_string(),
            });
        }
        for lang in lsp_servers {
            let id = format!("lsp:{lang}");
            out.push(Plugin {
                enabled: self.is_enabled(&id),
                id,
                kind: PluginKind::Lsp,
                name: lang.clone(),
                description: "Language server".to_string(),
                detail: "LSP".to_string(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_membership() {
        assert_eq!(group_of("GitStatus"), Some("git"));
        assert_eq!(group_of("FileRead"), Some("filesystem"));
        assert_eq!(group_of("Bash"), Some("shell"));
        assert_eq!(group_of("Skill"), None); // always on
        assert_eq!(group_of("ToolSearch"), None);
    }

    #[test]
    fn op_tools_are_grouped() {
        assert_eq!(group_of("OpRead"), Some("op"));
        assert_eq!(group_of("OpWrite"), Some("op"));
    }

    #[test]
    fn op_design_is_grouped() {
        assert_eq!(group_of("OpDesign"), Some("op"));
    }

    #[test]
    fn browser_tools_are_grouped() {
        assert_eq!(group_of("BrowserRead"), Some("browser"));
        assert_eq!(group_of("BrowserAct"), Some("browser"));
        assert_eq!(group_of("BrowserEval"), Some("browser"));
        assert_eq!(group_of("BrowserTabs"), Some("browser"));
        assert_eq!(group_of("BrowserUpload"), Some("browser"));
    }

    #[test]
    fn disabled_group_hides_its_tools() {
        let mut cfg = ZodeConfig::default();
        cfg.plugins.disabled = vec!["tools:git".into()];
        let pm = PluginManager::from_config(&cfg);
        assert!(!pm.tool_enabled("GitStatus"));
        assert!(pm.tool_enabled("FileRead")); // other groups stay on
        assert!(pm.tool_enabled("Skill")); // always on
    }

    #[test]
    fn toggle_round_trips() {
        let mut pm = PluginManager::default();
        assert!(pm.is_enabled("mcp:foo"));
        assert!(!pm.toggle("mcp:foo")); // now off
        assert!(!pm.is_enabled("mcp:foo"));
        assert_eq!(pm.disabled_ids(), vec!["mcp:foo".to_string()]);
        assert!(pm.toggle("mcp:foo")); // back on
        assert!(pm.disabled_ids().is_empty());
    }

    #[test]
    fn lists_all_kinds() {
        let pm = PluginManager::default();
        let plugins = pm.list(
            &[PackageEntry {
                name: "ui-demo".into(),
                description: "sidebar demo".into(),
                detail: "0.3.0".into(),
                enabled: true,
            }],
            &[("deepwiki".into(), true)],
            &[("review".into(), "review code".into())],
            &["rust".into()],
        );
        assert!(plugins.iter().any(|p| p.id == "plugin:ui-demo"));
        assert!(plugins.iter().any(|p| p.id == "tools:git"));
        assert!(plugins
            .iter()
            .any(|p| p.id == "mcp:deepwiki" && p.detail == "connected"));
        assert!(plugins.iter().any(|p| p.id == "skill:review"));
        assert!(plugins.iter().any(|p| p.id == "lsp:rust"));
    }

    /// A package row's state comes from the install registry, so a stray
    /// `plugin:*` entry in `plugins.disabled` must not switch it off.
    #[test]
    fn package_state_ignores_disabled_config() {
        let mut cfg = ZodeConfig::default();
        cfg.plugins.disabled = vec!["plugin:ui-demo".into()];
        let pm = PluginManager::from_config(&cfg);
        let plugins = pm.list(
            &[PackageEntry {
                name: "ui-demo".into(),
                description: "sidebar demo".into(),
                detail: "0.3.0".into(),
                enabled: true,
            }],
            &[],
            &[],
            &[],
        );
        let package = plugins
            .iter()
            .find(|p| p.id == "plugin:ui-demo")
            .expect("package row");
        assert!(package.enabled);
        assert_eq!(package.kind, PluginKind::Package);
    }
}
