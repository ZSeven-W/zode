//! User-defined workflows: reusable, named multi-step recipes the agent can
//! create (`define_workflow`), list (`/workflows`), and follow by dispatching
//! each step to a sub-agent with the Task tool.
//!
//! A workflow is a Markdown file with frontmatter and a numbered step list,
//! each step prefixed with the sub-agent type to run it:
//!
//! ```text
//! ---
//! name: review-and-fix
//! description: Review the working-tree diff, then fix the findings
//! ---
//! 1. [reviewer] Review the working-tree git diff and list concrete issues.
//! 2. [general] Fix each issue the reviewer found, then run the tests.
//! ```
//!
//! Discovered from `~/.zode/workflows`, `<cwd>/.zode/workflows`, and
//! `<cwd>/.claude/workflows` (later dirs override same-named earlier ones).
//! This is the definition/orchestration layer — steps are executed by the agent
//! via Task (advertised in the orchestration system-prompt directive), not by a
//! separate parallel runtime.

use std::path::{Path, PathBuf};

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::ConfigManager;
use crate::error::CoreError;

/// One step: which sub-agent type runs it, and the instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowStep {
    pub agent_type: String,
    pub prompt: String,
}

/// A parsed workflow definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDef {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

/// Workflow dirs, low → high precedence: global, project, .claude compat.
pub fn workflows_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(global) = ConfigManager::config_dir() {
        dirs.push(global.join("workflows"));
    }
    dirs.push(cwd.join(".zode").join("workflows"));
    dirs.push(cwd.join(".claude").join("workflows"));
    dirs
}

/// Parse one workflow file. `None` if there's no frontmatter `name`.
pub fn parse_workflow_def(text: &str) -> Option<WorkflowDef> {
    let (front, body) = crate::agents::split_frontmatter_pub(text)?;
    let mut name = None;
    let mut description = String::new();
    for line in front.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').to_string();
            match k.trim() {
                "name" => name = Some(v),
                "description" => description = v,
                _ => {}
            }
        }
    }
    let name = name.filter(|n| !n.is_empty())?;
    Some(WorkflowDef {
        name,
        description,
        steps: parse_steps(body),
    })
}

/// Parse step lines into [`WorkflowStep`]s — public for the workflow create
/// dialog, which collects steps as free text (one `[type] prompt` per line).
pub fn parse_step_lines(body: &str) -> Vec<WorkflowStep> {
    parse_steps(body)
}

/// Parse the body's step lines: `N. [agent_type] prompt` (the number and the
/// `[type]` are both optional; a missing type defaults to "general").
fn parse_steps(body: &str) -> Vec<WorkflowStep> {
    let mut steps = Vec::new();
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // Drop a leading "N." / "N)" ordinal.
        let rest = match line.find(['.', ')']) {
            Some(i) if line[..i].chars().all(|c| c.is_ascii_digit()) && i > 0 => {
                line[i + 1..].trim()
            }
            _ => line,
        };
        let rest = rest.trim_start_matches(['-', '*', ' ']).trim();
        let (agent_type, prompt) = match (rest.strip_prefix('['), rest.find(']')) {
            (Some(_), Some(close)) => (
                rest[1..close].trim().to_string(),
                rest[close + 1..].trim().to_string(),
            ),
            _ => ("general".to_string(), rest.to_string()),
        };
        if prompt.is_empty() {
            continue;
        }
        steps.push(WorkflowStep {
            agent_type: if agent_type.is_empty() {
                "general".into()
            } else {
                agent_type
            },
            prompt,
        });
    }
    steps
}

/// Load all workflow definitions from the standard dirs (precedence-merged).
pub fn load_workflow_defs(cwd: &Path) -> Vec<WorkflowDef> {
    let mut by_name: std::collections::BTreeMap<String, WorkflowDef> = Default::default();
    for dir in workflows_dirs(cwd) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(text) => {
                    if let Some(def) = parse_workflow_def(&text) {
                        by_name.insert(def.name.clone(), def);
                    } else {
                        tracing::warn!("skip workflow {} (no frontmatter name)", path.display());
                    }
                }
                Err(e) => tracing::warn!("skip workflow {}: {e}", path.display()),
            }
        }
    }
    by_name.into_values().collect()
}

/// Write (create or overwrite) a global workflow at `~/.zode/workflows/<name>.md`.
pub fn write_workflow_def(
    name: &str,
    description: &str,
    steps: &[WorkflowStep],
) -> Result<PathBuf, CoreError> {
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
        return Err(CoreError::Other("workflow name is empty".into()));
    }
    let dir = ConfigManager::config_dir()?.join("workflows");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{slug}.md"));
    let mut body = format!("---\nname: {name}\ndescription: {description}\n---\n");
    for (i, s) in steps.iter().enumerate() {
        body.push_str(&format!("{}. [{}] {}\n", i + 1, s.agent_type, s.prompt));
    }
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Delete a global workflow (`~/.zode/workflows/<name>.md`). Ok(false) if absent.
pub fn delete_workflow_def(name: &str) -> Result<bool, CoreError> {
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
        .join("workflows")
        .join(format!("{slug}.md"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(CoreError::Io(e)),
    }
}

/// Tool that lets the agent create a reusable workflow (autonomous orchestration).
/// Registered only when `autonomous_orchestration` is on.
#[derive(Debug, Default)]
pub struct DefineWorkflowTool;

#[async_trait]
impl Tool for DefineWorkflowTool {
    fn name(&self) -> &str {
        "define_workflow"
    }

    fn description(&self) -> &str {
        "Create a reusable, named workflow (ordered steps, each with a sub-agent \
         type + instruction) saved to ~/.zode/workflows. Define a workflow once, \
         then follow its steps by dispatching each to the Task tool. Takes effect \
         on the next session rebuild."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Short kebab-case identifier"},
                "description": {"type": "string", "description": "One-line summary"},
                "steps": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "agent_type": {"type": "string", "description": "Sub-agent type to run this step"},
                            "prompt": {"type": "string", "description": "Instruction for this step"}
                        },
                        "required": ["prompt"]
                    }
                }
            },
            "required": ["name", "description", "steps"]
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
        let steps: Vec<WorkflowStep> = input
            .get("steps")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|s| {
                        let prompt = s.get("prompt").and_then(|v| v.as_str())?.trim().to_string();
                        if prompt.is_empty() {
                            return None;
                        }
                        let agent_type = s
                            .get("agent_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("general")
                            .trim()
                            .to_string();
                        Some(WorkflowStep { agent_type, prompt })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if name.is_empty() || steps.is_empty() {
            return Err(AgentError::other(
                "define_workflow requires a name and at least one step",
            ));
        }
        let path = write_workflow_def(name, description, &steps)
            .map_err(|e| AgentError::other(e.to_string()))?;
        Ok(json!({
            "ok": true,
            "path": path.display().to_string(),
            "steps": steps.len(),
            "note": "available after the next session rebuild (/reload-plugins or next turn)"
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_and_typed_steps() {
        let text = "---\nname: review-and-fix\ndescription: Review then fix\n---\n1. [reviewer] Review the diff and list issues.\n2. [general] Fix each issue.\n";
        let def = parse_workflow_def(text).expect("parses");
        assert_eq!(def.name, "review-and-fix");
        assert_eq!(def.description, "Review then fix");
        assert_eq!(def.steps.len(), 2);
        assert_eq!(def.steps[0].agent_type, "reviewer");
        assert_eq!(def.steps[0].prompt, "Review the diff and list issues.");
        assert_eq!(def.steps[1].agent_type, "general");
    }

    #[test]
    fn step_without_type_defaults_to_general() {
        let def =
            parse_workflow_def("---\nname: w\ndescription: d\n---\n- just do the thing\n").unwrap();
        assert_eq!(def.steps.len(), 1);
        assert_eq!(def.steps[0].agent_type, "general");
        assert_eq!(def.steps[0].prompt, "just do the thing");
    }

    #[test]
    fn no_name_is_rejected() {
        assert!(parse_workflow_def("no frontmatter").is_none());
    }
}
