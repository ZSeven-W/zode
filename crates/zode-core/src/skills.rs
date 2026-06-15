//! Skills: load SKILL.md trees into a SkillRegistry, expose them via a
//! system-prompt index + a SkillTool for on-demand rendering. agent-rs
//! provides the registry but no tool, so Zode adds the tool (master §4.6③).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent::error::AgentError;
use agent::skills::{load_dir, SkillRegistry};
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::ConfigManager;

/// Skills dirs, low → high precedence (later dirs override same-named earlier
/// skills). Cross-agent compat sources (opencode / Claude / agents.md / codex,
/// each global then project) come first, then zode's own (global then project)
/// last — so a zode skill always wins a name clash, per "zode highest priority".
pub fn skills_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = dirs::home_dir();

    // 1. Global cross-agent sources (lowest precedence).
    if let Some(h) = &home {
        dirs.push(h.join(".config").join("opencode").join("skills")); // opencode
        dirs.push(h.join(".claude").join("skills")); // Claude
        dirs.push(h.join(".agents").join("skills")); // agents.md ecosystem
        dirs.push(h.join(".codex").join("skills")); // codex
        dirs.push(h.join(".gemini").join("antigravity-cli").join("skills")); // antigravity
        dirs.push(h.join(".pi").join("agent").join("skills")); // pi coding agent
        dirs.push(h.join(".kilo").join("skills")); // kilo
        dirs.push(h.join(".cursor").join("skills")); // cursor
    }
    // 1b. Skills bundled in installed plugins (e.g. Claude's `superpowers`
    // lives at ~/.claude/plugins/cache/<mp>/<plugin>/<ver>/skills). load_dir
    // only looks one level deep, so each `skills` dir must be listed; scan the
    // plugin trees (claude / codex / opencode) for them.
    if let Some(h) = &home {
        collect_plugin_skill_dirs(&h.join(".claude").join("plugins"), &mut dirs);
        collect_plugin_skill_dirs(&h.join(".codex").join("plugins"), &mut dirs);
        collect_plugin_skill_dirs(
            &h.join(".config").join("opencode").join("plugin"),
            &mut dirs,
        );
    }
    // 2. Project cross-agent sources (incl. their plugin trees).
    dirs.push(cwd.join(".opencode").join("skills"));
    dirs.push(cwd.join(".claude").join("skills"));
    dirs.push(cwd.join(".agents").join("skills"));
    dirs.push(cwd.join(".codex").join("skills"));
    collect_plugin_skill_dirs(&cwd.join(".claude").join("plugins"), &mut dirs);
    collect_plugin_skill_dirs(&cwd.join(".codex").join("plugins"), &mut dirs);
    // 3. zode's own (highest precedence): global then project, including its
    // own plugins folder (~/.zode/plugins, .zode/plugins) scanned for skills.
    if let Ok(global) = ConfigManager::config_dir() {
        collect_plugin_skill_dirs(&global.join("plugins"), &mut dirs);
        dirs.push(global.join("skills"));
    }
    collect_plugin_skill_dirs(&cwd.join(".zode").join("plugins"), &mut dirs);
    dirs.push(cwd.join(".zode").join("skills"));
    dirs
}

/// Recursively find every directory named `skills` under `root` (a plugin
/// tree), bounded in depth so a deep/cyclic layout can't run away. Each found
/// dir is one `load_dir` target (its subdirs hold `SKILL.md`). A `skills` dir
/// is not descended into. No-op if `root` doesn't exist.
fn collect_plugin_skill_dirs(root: &Path, out: &mut Vec<PathBuf>) {
    fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > 6 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            if p.file_name().and_then(|n| n.to_str()) == Some("skills") {
                out.push(p);
            } else {
                walk(&p, depth + 1, out);
            }
        }
    }
    walk(root, 0, out);
}

/// Load all skills from the given dirs into one registry. Missing dirs are
/// skipped; same-name skills from later dirs override earlier ones (insert
/// replaces). Load warnings are logged via tracing.
pub fn load_skills_from(dirs: &[PathBuf]) -> SkillRegistry {
    load_skills_filtered(dirs, |_| true)
}

/// Like [`load_skills_from`], but only inserts skills for which `keep(name)`
/// returns true — used to drop skills disabled via the plugin manager.
pub fn load_skills_filtered(dirs: &[PathBuf], keep: impl Fn(&str) -> bool) -> SkillRegistry {
    let registry = SkillRegistry::new();
    let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        // A dir can be reached via more than one source (e.g. a plugin's skills
        // dir under both cache/ and marketplaces/) — load each once.
        let key = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        if !seen.insert(key) {
            continue;
        }
        match load_dir(dir) {
            Ok(outcome) => {
                for w in &outcome.warnings {
                    // Foreign/plugin skills we don't own (e.g. a missing
                    // description) aren't user-actionable here — keep it quiet.
                    tracing::debug!("skill load warning in {}: {w:?}", dir.display());
                }
                for skill in outcome.skills {
                    if !keep(&skill.name) {
                        continue;
                    }
                    // Drop skills whose body hard-codes another agent's host
                    // variables (e.g. `${CLAUDE_PLUGIN_ROOT}/scripts/…`): they
                    // resolve to nothing under zode and would fail if invoked.
                    if let Some(var) = crate::portability::foreign_host_var(&skill.prompt) {
                        tracing::debug!(
                            "skip non-portable skill {} in {}: references ${}",
                            skill.name,
                            dir.display(),
                            var
                        );
                        continue;
                    }
                    registry.insert(skill);
                }
            }
            Err(e) => tracing::debug!("skip skills dir {}: {e}", dir.display()),
        }
    }
    registry
}

/// "- name: description" index for the system prompt. Empty string if none.
pub fn skills_index(registry: &SkillRegistry) -> String {
    registry
        .list()
        .iter()
        .map(|s| format!("- {}: {}", s.name, s.description))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tool that renders a skill's full prompt on demand (progressive
/// disclosure): the model calls Skill{name} and gets the rendered body back.
#[derive(Debug)]
pub struct SkillTool {
    registry: Arc<SkillRegistry>,
}

impl SkillTool {
    pub fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }
    fn description(&self) -> &str {
        "Load a skill's full instructions by name. Use the skills listed in the system prompt."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Skill name from the Available Skills list."},
                "params": {"type": "object", "description": "Optional template parameters."}
            },
            "required": ["name"]
        })
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::other("Skill requires `name`"))?;
        let skill = self
            .registry
            .get(name)
            .ok_or_else(|| AgentError::other(format!("no skill named '{name}'")))?;
        let params = input.get("params").cloned().unwrap_or_else(|| json!({}));
        let rendered = skill
            .render(&params)
            .map_err(|e| AgentError::other(format!("skill render: {e}")))?;
        Ok(json!({ "skill": name, "instructions": rendered }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(dir: &Path, name: &str, desc: &str, body: &str) {
        let sk = dir.join(name);
        std::fs::create_dir_all(&sk).unwrap();
        std::fs::write(
            sk.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: {desc}\n---\n{body}"),
        )
        .unwrap();
    }

    #[test]
    fn loads_skills_and_builds_index() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "code-review", "review code", "Do the review.");
        let reg = load_skills_from(&[dir.path().to_path_buf()]);
        let idx = skills_index(&reg);
        assert!(idx.contains("code-review"));
        assert!(idx.contains("review code"));
    }

    #[test]
    fn drops_non_portable_skill() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "portable", "ok", "Run `cargo build`.");
        write_skill(
            dir.path(),
            "claude-only",
            "broken",
            "Run ${CLAUDE_PLUGIN_ROOT}/scripts/go.sh now.",
        );
        let reg = load_skills_from(&[dir.path().to_path_buf()]);
        let idx = skills_index(&reg);
        assert!(idx.contains("portable"));
        assert!(
            !idx.contains("claude-only"),
            "skill referencing a foreign host var must be filtered out"
        );
    }

    #[tokio::test]
    async fn skill_tool_renders_prompt() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(
            dir.path(),
            "translate",
            "translate text",
            "Translate it well.",
        );
        let reg = Arc::new(load_skills_from(&[dir.path().to_path_buf()]));
        let tool = SkillTool::new(reg);
        let out = tool
            .call(
                &ToolUseContext::new(dir.path().to_path_buf()),
                json!({"name": "translate"}),
            )
            .await
            .unwrap();
        assert!(out["instructions"]
            .as_str()
            .unwrap()
            .contains("Translate it well."));
    }

    #[tokio::test]
    async fn unknown_skill_errors() {
        let dir = tempfile::tempdir().unwrap();
        let reg = Arc::new(load_skills_from(&[dir.path().to_path_buf()]));
        let tool = SkillTool::new(reg);
        let r = tool
            .call(
                &ToolUseContext::new(dir.path().to_path_buf()),
                json!({"name": "nope"}),
            )
            .await;
        assert!(r.is_err());
    }
}
