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

/// Skill directories, low → high precedence.
///
/// Direct skill directories belonging to other agents are portable user
/// configuration and are scanned. Their plugin caches are deliberately not
/// scanned; install a plugin explicitly through Zode to use it here.
pub fn skills_dirs(cwd: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".config").join("opencode").join("skills"));
        dirs.push(home.join(".claude").join("skills"));
        dirs.push(home.join(".agents").join("skills"));
        dirs.push(home.join(".codex").join("skills"));
        dirs.push(home.join(".gemini").join("antigravity-cli").join("skills"));
        dirs.push(home.join(".pi").join("agent").join("skills"));
        dirs.push(home.join(".kilo").join("skills"));
        dirs.push(home.join(".cursor").join("skills"));
    }
    dirs.push(cwd.join(".opencode").join("skills"));
    dirs.push(cwd.join(".claude").join("skills"));
    dirs.push(cwd.join(".agents").join("skills"));
    dirs.push(cwd.join(".codex").join("skills"));

    // Zode global sources, including plugins installed by Zode itself.
    if let Ok(global) = ConfigManager::config_dir() {
        collect_plugin_skill_dirs(&global.join("plugins"), &mut dirs);
        dirs.push(global.join("skills"));
    }
    dirs.extend(crate::plugin_package::installed_package_dirs(
        crate::plugin_package::PackageDirectoryKind::Skills,
    ));
    // Project-local Zode sources have highest precedence.
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

/// Scan the dir tree ONCE and return both `(name, description)` for every
/// discovered skill AND a registry containing only those for which
/// `keep(name)` is true. Avoids walking + YAML-parsing the whole (large,
/// plugin) skills tree twice per engine assembly — the previous pattern of a
/// full `load_skills_from` for the meta list plus a
/// separate `load_skills_filtered` for the live registry.
pub fn load_skills_meta_and_registry(
    dirs: &[PathBuf],
    keep: impl Fn(&str) -> bool,
) -> (Vec<(String, String)>, SkillRegistry) {
    let all = load_skills_from(dirs);
    let mut meta: Vec<(String, String)> = all
        .list()
        .iter()
        .map(|s| (s.name.clone(), s.description.clone()))
        .collect();
    meta.sort();
    // Derive the enabled registry in-memory (drop disabled names) rather
    // than re-reading and re-parsing every SKILL.md from disk.
    let enabled = SkillRegistry::new();
    for skill in all.list() {
        if keep(&skill.name) {
            enabled.insert(skill);
        }
    }
    (meta, enabled)
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

const SKILL_DISCIPLINE: &str = "\n### Using skills\n\
Before starting non-trivial work, scan the Available Skills above; if one plausibly \
applies, invoke it with the Skill tool FIRST and state which you're using. For \
multi-step features or changes, prefer a plan-first flow — use any available \
planning/brainstorming skill before writing code, and follow test-driven development \
if a testing skill applies.\n";

/// Provider-ready skills section shared by root and Task child prompts.
/// Keeping one renderer prevents child agents from seeing a different set of
/// registered skill names or different invocation discipline than the caller.
pub fn skills_prompt_from_index(index: &str, discipline: bool) -> String {
    if index.is_empty() {
        return String::new();
    }
    let mut prompt = String::from(
        "\n\n## Available Skills\n\
Invoke a skill by name with the Skill tool to load its full instructions:\n",
    );
    prompt.push_str(index);
    prompt.push('\n');
    if discipline {
        prompt.push_str(SKILL_DISCIPLINE);
    }
    prompt
}

pub fn skills_prompt(registry: &SkillRegistry, discipline: bool) -> String {
    skills_prompt_from_index(&skills_index(registry), discipline)
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
    #[serial_test::serial]
    fn skills_dirs_import_direct_sources_but_not_plugin_caches() {
        let config = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("ZODE_CONFIG_DIR");
        std::env::set_var("ZODE_CONFIG_DIR", config.path());

        for relative in [
            ".claude/skills",
            ".codex/skills",
            ".agents/skills",
            ".opencode/skills",
            ".claude/plugins/cache/demo/skills",
            ".codex/plugins/cache/demo/skills",
        ] {
            std::fs::create_dir_all(project.path().join(relative)).unwrap();
        }

        let dirs = skills_dirs(project.path());
        match previous {
            Some(value) => std::env::set_var("ZODE_CONFIG_DIR", value),
            None => std::env::remove_var("ZODE_CONFIG_DIR"),
        }

        assert!(dirs.contains(&project.path().join(".claude/skills")));
        assert!(dirs.contains(&project.path().join(".codex/skills")));
        assert!(dirs.contains(&project.path().join(".agents/skills")));
        assert!(dirs.contains(&project.path().join(".opencode/skills")));
        assert!(!dirs
            .iter()
            .any(|path| path.to_string_lossy().contains("/plugins/")));
        assert!(dirs.contains(&config.path().join("skills")));
        assert!(dirs.contains(&project.path().join(".zode/skills")));
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

    #[test]
    fn meta_and_registry_from_one_scan_apply_keep_filter() {
        let dir = tempfile::tempdir().unwrap();
        write_skill(dir.path(), "keep-me", "enabled", "Body.");
        write_skill(dir.path(), "drop-me", "disabled", "Body.");
        let (meta, reg) =
            load_skills_meta_and_registry(&[dir.path().to_path_buf()], |n| n != "drop-me");
        // Meta lists BOTH (the /plugin picker shows disabled skills too)...
        let names: Vec<&str> = meta.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"keep-me") && names.contains(&"drop-me"));
        assert_eq!(meta, {
            let mut m = meta.clone();
            m.sort();
            m
        }); // sorted
            // ...but the live registry only holds the enabled one.
        assert!(reg.get("keep-me").is_some());
        assert!(reg.get("drop-me").is_none());
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
