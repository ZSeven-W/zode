//! User-defined workflows: reusable, named JS orchestration scripts the agent
//! can create (`DefineWorkflow`), list (`/workflows`), and RUN deterministically
//! (`RunWorkflow` tool, or Enter in the `/workflows` dialog). Execution happens
//! in zode's QuickJS runtime ([`crate::workflows_js`]) — the script drives real
//! sub-agents through `agent()` / `parallel()` / `pipeline()`; the model never
//! "follows steps" on its own. (The old Markdown step-list format is retired:
//! `.md` files in the workflow dirs are skipped with a warning.)
//!
//! A workflow file is frontmatter + a JS body:
//!
//! ```text
//! ---
//! name: review-and-fix
//! description: Review the diff in parallel, then fix the findings
//! ---
//! const findings = await parallel([
//!   () => agent("Review the working-tree diff for bugs", { type: "reviewer" }),
//!   () => agent("Review test coverage gaps", { type: "researcher" }),
//! ]);
//! return await agent(`Fix these findings:\n${findings.filter(Boolean).join("\n")}`);
//! ```
//!
//! Discovered from `~/.zode/workflows`, `<cwd>/.zode/workflows`, and
//! `<cwd>/.claude/workflows` (later dirs override same-named earlier ones).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::config::ConfigManager;
use crate::error::CoreError;
use crate::task_factory::ParentToolsCell;
use crate::workflows_js::{run_js_workflow, syntax_check, GatedTaskRunner};

/// A parsed workflow definition: frontmatter + the JS orchestration body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDef {
    pub name: String,
    pub description: String,
    pub script: String,
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
        script: body.to_string(),
    })
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
            match path.extension().and_then(|e| e.to_str()) {
                Some("js") => {}
                Some("md") => {
                    tracing::warn!(
                        "skip legacy Markdown workflow {} (rewrite it as a .js script)",
                        path.display()
                    );
                    continue;
                }
                _ => continue,
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

fn slug_of(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Write (create or overwrite) a global workflow at `~/.zode/workflows/<name>.js`.
pub fn write_workflow_def(
    name: &str,
    description: &str,
    script: &str,
) -> Result<PathBuf, CoreError> {
    let slug = slug_of(name);
    if slug.is_empty() {
        return Err(CoreError::Other("workflow name is empty".into()));
    }
    let dir = ConfigManager::config_dir()?.join("workflows");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{slug}.js"));
    let body = format!("---\nname: {name}\ndescription: {description}\n---\n{script}\n");
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Delete a global workflow (`~/.zode/workflows/<name>.js`, plus a legacy
/// same-named `.md` if present). Ok(false) if neither existed.
pub fn delete_workflow_def(name: &str) -> Result<bool, CoreError> {
    let slug = slug_of(name);
    let dir = ConfigManager::config_dir()?.join("workflows");
    let mut removed = false;
    for ext in ["js", "md"] {
        match std::fs::remove_file(dir.join(format!("{slug}.{ext}"))) {
            Ok(()) => removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(CoreError::Io(e)),
        }
    }
    Ok(removed)
}

/// The scripting surface documented for the model (and `/help`): kept in one
/// place so `DefineWorkflow`'s description and prompts stay in sync.
pub const WORKFLOW_JS_API: &str = "The script body runs in a sandboxed JS runtime \
    (no fs/net) with: `await agent(prompt, {type, description, mode})` → run one \
    sub-agent and get its final text (`mode` is forwarded unchanged to `Task`, which \
    validates it); `parallel([...thunks])` → run thunks concurrently (failed ones \
    resolve to null); `pipeline(items, ...stages)` → per-item stage chains; `log(msg)` \
    → progress line; `args` → the invocation's arguments value. Use top-level `await` \
    and `return` the final result.";

/// Tool that lets the agent create a reusable JS workflow (autonomous
/// orchestration). Registered only when `autonomous_orchestration` is on.
#[derive(Debug, Default)]
pub struct DefineWorkflowTool;

#[async_trait]
impl Tool for DefineWorkflowTool {
    fn name(&self) -> &str {
        "DefineWorkflow"
    }

    fn description(&self) -> &str {
        "Create a reusable, named JS workflow saved to ~/.zode/workflows/<name>.js. \
         The `script` is an orchestration body executed deterministically by zode \
         (run it later with the RunWorkflow tool). Do NOT include frontmatter in \
         `script` — it is added from `name`/`description`."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Short kebab-case identifier"},
                "description": {"type": "string", "description": "One-line summary"},
                "script": {"type": "string", "description": WORKFLOW_JS_API}
            },
            "required": ["name", "description", "script"]
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
        let script = input
            .get("script")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if name.is_empty() || script.is_empty() {
            return Err(AgentError::other(
                "DefineWorkflow requires a name and a non-empty script",
            ));
        }
        // Parse-only validation (the body is wrapped in an async fn, so
        // top-level await/return are legal). Catches typos before saving.
        syntax_check(script).map_err(|e| AgentError::other(format!("script error: {e}")))?;
        let path = write_workflow_def(name, description, script)
            .map_err(|e| AgentError::other(e.to_string()))?;
        Ok(json!({
            "ok": true,
            "path": path.display().to_string(),
            "note": "run it with the RunWorkflow tool (available after the next session rebuild for /workflows)"
        }))
    }
}

/// Tool that runs a saved JS workflow to completion: every `agent()` call in
/// the script dispatches through the parent's FINAL gated registry's `Task`
/// tool, so approvals / sandboxing / the recursion guard / sub-agent
/// visibility all match model-initiated tasks.
#[derive(Debug)]
pub struct RunWorkflowTool {
    /// The parent's final gated+sandboxed registry (set post-assembly).
    tools: ParentToolsCell,
}

impl RunWorkflowTool {
    pub fn new(tools: ParentToolsCell) -> Self {
        Self { tools }
    }
}

#[async_trait]
impl Tool for RunWorkflowTool {
    fn name(&self) -> &str {
        "RunWorkflow"
    }

    fn description(&self) -> &str {
        "Run a saved JS workflow to completion and return its result. `args` is \
         exposed to the script as the global `args`. Prefer this over manually \
         re-implementing a saved workflow's steps."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "The workflow's frontmatter name"},
                "args": {"description": "Optional value passed to the script as `args`"}
            },
            "required": ["name"]
        })
    }

    fn safety_class(&self) -> SafetyClass {
        // The script itself is sandboxed, but its agents can call mutating
        // tools (each individually gated).
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let name = input
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let args = input.get("args").cloned().unwrap_or(Value::Null);
        let def = load_workflow_defs(&ctx.cwd)
            .into_iter()
            .find(|w| w.name == name)
            .ok_or_else(|| AgentError::other(format!("RunWorkflow: no workflow named '{name}'")))?;
        let tools = self
            .tools
            .get()
            .cloned()
            .ok_or_else(|| AgentError::other("RunWorkflow: tool registry not ready"))?;
        let runner = Arc::new(GatedTaskRunner::new(
            tools,
            ctx.cwd.clone(),
            ctx.file_cache.clone(),
            ctx.permissions.clone(),
            ctx.hooks.clone(),
            ctx.abort.clone(),
            ctx.task_depth,
        ));
        let logs: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = logs.clone();
        let log: crate::workflows_js::LogSink = Arc::new(move |m| {
            if let Ok(mut l) = sink.lock() {
                l.push(m);
            }
        });
        let result = run_js_workflow(&def.script, args, runner, log)
            .await
            .map_err(AgentError::other)?;
        let logs = logs.lock().map(|l| l.clone()).unwrap_or_default();
        Ok(json!({ "name": def.name, "result": result, "logs": logs }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct DepthRecordingTask {
        seen: Arc<AtomicUsize>,
        input: Arc<std::sync::Mutex<Option<Value>>>,
    }

    #[async_trait]
    impl Tool for DepthRecordingTask {
        fn name(&self) -> &str {
            "Task"
        }

        fn description(&self) -> &str {
            "record Task depth"
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }

        async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
            self.seen.store(ctx.task_depth, Ordering::SeqCst);
            *self.input.lock().unwrap() = Some(input);
            Ok(json!({"output": "ok"}))
        }
    }

    #[test]
    fn parses_name_description_and_script() {
        let text = "---\nname: review-and-fix\ndescription: Review then fix\n---\nreturn await agent(\"go\");\n";
        let def = parse_workflow_def(text).expect("parses");
        assert_eq!(def.name, "review-and-fix");
        assert_eq!(def.description, "Review then fix");
        assert!(def.script.contains("agent(\"go\")"));
    }

    #[test]
    fn no_name_is_rejected() {
        assert!(parse_workflow_def("no frontmatter").is_none());
        assert!(parse_workflow_def("---\ndescription: d\n---\nreturn 1;").is_none());
    }

    #[test]
    fn loads_js_and_skips_legacy_md() {
        let dir = tempfile::tempdir().unwrap();
        let wf = dir.path().join(".zode").join("workflows");
        std::fs::create_dir_all(&wf).unwrap();
        std::fs::write(
            wf.join("a.js"),
            "---\nname: js-flow\ndescription: d\n---\nreturn 1;\n",
        )
        .unwrap();
        std::fs::write(
            wf.join("b.md"),
            "---\nname: md-flow\ndescription: d\n---\n1. [general] step\n",
        )
        .unwrap();
        let defs = load_workflow_defs(dir.path());
        assert!(defs.iter().any(|d| d.name == "js-flow"));
        assert!(!defs.iter().any(|d| d.name == "md-flow"));
    }

    #[test]
    fn slug_sanitizes_names() {
        assert_eq!(slug_of("review & fix!"), "review---fix-");
        assert_eq!(slug_of("ok-name_1"), "ok-name_1");
    }

    #[tokio::test]
    async fn run_workflow_preserves_the_callers_task_depth_for_agent_dispatch() {
        let dir = tempfile::tempdir().unwrap();
        let workflow_dir = dir.path().join(".zode").join("workflows");
        std::fs::create_dir_all(&workflow_dir).unwrap();
        std::fs::write(
            workflow_dir.join("depth-flow.js"),
            "---\nname: depth-flow\ndescription: depth test\n---\n\
             return await agent(\"nested\", { type: \"reviewer\", description: \"inspect\", \
             mode: \"future-mode\" });\n",
        )
        .unwrap();

        let seen = Arc::new(AtomicUsize::new(usize::MAX));
        let seen_input = Arc::new(std::sync::Mutex::new(None));
        let mut registry = agent::tool::ToolRegistry::new();
        registry.register(Arc::new(DepthRecordingTask {
            seen: seen.clone(),
            input: seen_input.clone(),
        }));
        let tools: ParentToolsCell = Arc::new(std::sync::OnceLock::new());
        tools.set(Arc::new(registry)).unwrap();
        let tool = RunWorkflowTool::new(tools);
        let mut ctx = ToolUseContext::new(dir.path());
        ctx.permissions = Arc::new(
            agent::permission::PermissionManager::new()
                .allow(agent::permission::RuleSource::User, "Task"),
        );
        ctx.task_depth = 2;

        let output = tool
            .call(&ctx, json!({"name": "depth-flow"}))
            .await
            .unwrap();

        assert_eq!(output["result"], "ok");
        assert_eq!(seen.load(Ordering::SeqCst), 2);
        let input = seen_input.lock().unwrap().clone().unwrap();
        assert_eq!(input["prompt"], "nested");
        assert_eq!(input["agent_type"], "reviewer");
        assert_eq!(input["description"], "inspect");
        assert_eq!(input["mode"], "future-mode");
    }
}
