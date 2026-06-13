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

/// Standard skills dirs (low → high precedence): global ~/.zode/skills,
/// project .zode/skills, project .claude/skills (compat).
pub fn skills_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(global) = ConfigManager::config_dir() {
        dirs.push(global.join("skills"));
    }
    dirs.push(cwd.join(".zode").join("skills"));
    dirs.push(cwd.join(".claude").join("skills"));
    dirs
}

/// Load all skills from the given dirs into one registry. Missing dirs are
/// skipped; same-name skills from later dirs override earlier ones (insert
/// replaces). Load warnings are logged via tracing.
pub fn load_skills_from(dirs: &[PathBuf]) -> SkillRegistry {
    let registry = SkillRegistry::new();
    for dir in dirs {
        if !dir.exists() {
            continue;
        }
        match load_dir(dir) {
            Ok(outcome) => {
                for w in &outcome.warnings {
                    tracing::warn!("skill load warning in {}: {w:?}", dir.display());
                }
                for skill in outcome.skills {
                    registry.insert(skill);
                }
            }
            Err(e) => tracing::warn!("skip skills dir {}: {e}", dir.display()),
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
