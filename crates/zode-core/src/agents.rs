//! User-defined sub-agent definitions, mirroring Claude Code's `.claude/agents`.
//!
//! An agent definition is a Markdown file with YAML-ish frontmatter and a body
//! that becomes the sub-agent's system prompt:
//!
//! ```text
//! ---
//! name: refactorer
//! description: Refactors a target module; read-write
//! model: claude-opus-4-8        # optional, overrides the parent model
//! ---
//! You are a refactoring sub-agent. Improve the target's structure without
//! changing behavior, then report what you changed.
//! ```
//!
//! Definitions are discovered from `~/.zode/agents`, `<cwd>/.zode/agents`, and
//! `<cwd>/.claude/agents` (later dirs override same-named earlier ones). The
//! Task tool can then spawn them by `name`, alongside the built-in types, and
//! `/agents` lists them. `write_agent_def` powers `/agents <name>` creation and
//! the autonomous `define_agent` tool.

use std::path::{Path, PathBuf};

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::ConfigManager;
use crate::error::CoreError;

/// A parsed sub-agent definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    /// System prompt (the Markdown body after the frontmatter).
    pub system: String,
    /// Optional per-agent model override; `None` inherits the parent model.
    pub model: Option<String>,
}

/// Agent definition dirs, low → high precedence (later overrides same-named).
/// Cross-agent compat sources (Claude / opencode / agents.md, each global then
/// project) first, then zode's own (global then project) last so it always wins.
/// Definitions in a foreign format that don't parse are skipped with a warning.
pub fn agents_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = dirs::home_dir();
    // 1. Global cross-agent sources.
    if let Some(h) = &home {
        dirs.push(h.join(".claude").join("agents"));
        dirs.push(h.join(".config").join("opencode").join("agent"));
        dirs.push(h.join(".agents").join("agents"));
    }
    // 2. Project cross-agent sources.
    dirs.push(cwd.join(".claude").join("agents"));
    dirs.push(cwd.join(".opencode").join("agent"));
    dirs.push(cwd.join(".agents").join("agents"));
    // 3. zode's own (highest): global then project.
    if let Ok(global) = ConfigManager::config_dir() {
        dirs.push(global.join("agents"));
    }
    dirs.extend(crate::plugin_package::installed_package_dirs(
        crate::plugin_package::PackageDirectoryKind::Agents,
    ));
    dirs.push(cwd.join(".zode").join("agents"));
    dirs
}

/// Split a frontmatter document (`---\n…\n---\nbody`) into (frontmatter, body).
/// Shared by agent and workflow parsing. `None` if there's no leading/closing
/// `---`.
pub fn split_frontmatter_pub(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("---")?;
    split_frontmatter(rest)
}

/// Parse one agent-definition file. `None` if there's no frontmatter `name`.
pub fn parse_agent_def(text: &str) -> Option<AgentDef> {
    // Frontmatter ends at the next line that is exactly "---".
    let (front, body) = split_frontmatter_pub(text)?;
    let mut name = None;
    let mut description = String::new();
    let mut model = None;
    for line in front.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "name" => name = Some(v),
                "description" => description = v,
                "model" if !v.is_empty() => model = Some(v),
                _ => {}
            }
        }
    }
    let name = name.filter(|n| !n.is_empty())?;
    Some(AgentDef {
        name,
        description,
        system: body.trim().to_string(),
        model,
    })
}

/// Split the text after the opening `---` into (frontmatter, body) at the
/// closing `---` line.
fn split_frontmatter(after_open: &str) -> Option<(&str, &str)> {
    let after_open = after_open.strip_prefix('\n').unwrap_or(after_open);
    // Find a line that is exactly "---".
    let mut idx = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end() == "---" {
            let front = &after_open[..idx];
            let body = &after_open[idx + line.len()..];
            return Some((front, body));
        }
        idx += line.len();
    }
    None
}

/// Load all agent definitions from the standard dirs. Same-name defs from a
/// later (higher-precedence) dir override earlier ones. Bad files are skipped
/// with a warning.
pub fn load_agent_defs(cwd: &Path) -> Vec<AgentDef> {
    let mut by_name: std::collections::BTreeMap<String, AgentDef> = Default::default();
    for dir in agents_dirs(cwd) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => match parse_agent_def(&text) {
                    Some(def) => {
                        by_name.insert(def.name.clone(), def);
                    }
                    None => {
                        tracing::warn!("skip agent def {} (no frontmatter name)", path.display())
                    }
                },
                Err(e) => tracing::warn!("skip agent def {}: {e}", path.display()),
            }
        }
    }
    by_name.into_values().collect()
}

/// Write (create or overwrite) a global agent definition at
/// `~/.zode/agents/<name>.md`. Returns the path written.
pub fn write_agent_def(name: &str, description: &str, system: &str) -> Result<PathBuf, CoreError> {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if slug.is_empty() {
        return Err(CoreError::Other("agent name is empty".into()));
    }
    let dir = ConfigManager::config_dir()?.join("agents");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{slug}.md"));
    let body = format!("---\nname: {name}\ndescription: {description}\n---\n{system}\n");
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Delete a global agent definition (`~/.zode/agents/<name>.md`). Returns
/// Ok(false) if there was no such file. Project-level defs are not touched.
pub fn delete_agent_def(name: &str) -> Result<bool, CoreError> {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let path = ConfigManager::config_dir()?
        .join("agents")
        .join(format!("{slug}.md"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(CoreError::Io(e)),
    }
}

/// Tool that lets the agent create a new sub-agent definition at
/// `~/.zode/agents/<name>.md` (autonomous orchestration). Registered only when
/// `autonomous_orchestration` is on. The new agent becomes spawnable after the
/// next session rebuild (`/reload-plugins` or a new turn that reassembles).
#[derive(Debug, Default)]
pub struct DefineAgentTool;

#[async_trait]
impl Tool for DefineAgentTool {
    fn name(&self) -> &str {
        "define_agent"
    }

    fn description(&self) -> &str {
        "Create a reusable sub-agent definition (name, description, system prompt) \
         saved to ~/.zode/agents. Use this to specialize a sub-agent you will then \
         spawn with the Task tool. Takes effect on the next session rebuild."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Short kebab-case identifier"},
                "description": {"type": "string", "description": "One-line summary"},
                "system": {"type": "string", "description": "The sub-agent's system prompt"}
            },
            "required": ["name", "description", "system"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let system = input
            .get("system")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if name.is_empty() || system.is_empty() {
            return Err(AgentError::other(
                "define_agent requires non-empty name and system",
            ));
        }
        let path = write_agent_def(name, description, system)
            .map_err(|e| AgentError::other(e.to_string()))?;
        Ok(json!({
            "ok": true,
            "path": path.display().to_string(),
            "note": "available after the next session rebuild (/reload-plugins or next turn)"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frontmatter_and_body() {
        let text = "---\nname: refactorer\ndescription: Refactors a module\nmodel: claude-opus-4-8\n---\nYou are a refactoring sub-agent.\nReport what changed.\n";
        let def = parse_agent_def(text).expect("parses");
        assert_eq!(def.name, "refactorer");
        assert_eq!(def.description, "Refactors a module");
        assert_eq!(def.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(
            def.system,
            "You are a refactoring sub-agent.\nReport what changed."
        );
    }

    #[test]
    fn no_name_is_rejected() {
        assert!(parse_agent_def("---\ndescription: x\n---\nbody").is_none());
        assert!(parse_agent_def("no frontmatter at all").is_none());
    }

    #[test]
    fn model_is_optional() {
        let def = parse_agent_def("---\nname: a\ndescription: d\n---\nbody").unwrap();
        assert_eq!(def.model, None);
    }
}
