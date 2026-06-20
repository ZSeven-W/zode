//! `op_read` (ReadOnly, ungated) and `op_write` (Mutating, gated) agent tools.
//! `safety_class()` is static per tool, so read vs write must be two tools.

use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::connection::OpConnection;
use super::design::{load_guidance, DesignOrchestrator, DirectLlmContentGenerator};
use super::{is_read_tool, Consent};
use crate::config::OpenPencilConfig;
use crate::question::QuestionQueue;

/// Shared deps: enough to `ensure` a client per call.
#[derive(Debug, Clone)]
pub struct OpToolDeps {
    pub cfg: OpenPencilConfig,
    pub consent: Arc<dyn Consent>,
    pub tag: String,
}

/// Deps for `op_design` — SEPARATE from `OpToolDeps`. `op_read`/`op_write` are
/// registered before the skills registry exists, so their deps stay narrow
/// (cfg/consent/tag); only `op_design` (registered after skills) needs the
/// provider/model/skills to drive the design pipeline.
#[derive(Clone, Debug)]
pub struct OpDesignDeps {
    pub cfg: OpenPencilConfig,
    pub consent: Arc<dyn Consent>,
    pub tag: String,
    pub provider: Arc<dyn agent::provider::Provider>,
    pub model: String,
    pub skills: Arc<agent::skills::SkillRegistry>,
}

/// Consent backed by the question queue: a yes/no prompt showing the action.
#[derive(Debug)]
pub struct QueueConsent {
    queue: QuestionQueue,
    source: Option<String>,
}

impl QueueConsent {
    pub fn new(queue: QuestionQueue, source: Option<String>) -> Self {
        Self { queue, source }
    }
}

#[async_trait]
impl Consent for QueueConsent {
    async fn confirm(&self, prompt: &str) -> bool {
        let opts = vec!["Yes".to_string(), "No".to_string()];
        matches!(
            self.queue.ask(prompt, &opts, self.source.clone()).await,
            Some(0)
        )
    }
}

/// Route a tool call to the live OpenPencil instance. Validates read/write
/// routing (misrouted calls are rejected immediately, before any network I/O).
async fn dispatch(deps: &OpToolDeps, input: Value, writing: bool) -> Result<Value, AgentError> {
    let tool = input
        .get("tool")
        .and_then(|t| t.as_str())
        .ok_or_else(|| AgentError::other("op: missing string 'tool'"))?;
    if writing && is_read_tool(tool) {
        return Err(AgentError::other(format!(
            "op_write: '{tool}' is read-only; use op_read"
        )));
    }
    if !writing && !is_read_tool(tool) {
        return Err(AgentError::other(format!(
            "op_read: '{tool}' mutates; use op_write"
        )));
    }
    let args = input.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let client = OpConnection::ensure(&deps.cfg, deps.consent.as_ref(), &deps.tag)
        .await
        .map_err(|e| AgentError::other(e.to_string()))?;
    client
        .call(tool, args)
        .await
        .map_err(|e| AgentError::other(e.to_string()))
}

fn schema(kind: &str) -> Value {
    json!({
        "type": "object",
        "required": ["tool"],
        "properties": {
            "tool": {
                "type": "string",
                "description": format!("OpenPencil {kind} MCP tool name")
            },
            "arguments": {
                "type": "object",
                "additionalProperties": true
            }
        }
    })
}

/// Reads OpenPencil design state via read-only MCP tools.
#[derive(Debug)]
pub struct OpReadTool {
    deps: OpToolDeps,
}

/// Mutates an OpenPencil design via write MCP tools.
#[derive(Debug)]
pub struct OpWriteTool {
    deps: OpToolDeps,
}

impl OpReadTool {
    pub fn new(deps: OpToolDeps) -> Self {
        Self { deps }
    }
}

impl OpWriteTool {
    pub fn new(deps: OpToolDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for OpReadTool {
    fn name(&self) -> &str {
        "op_read"
    }

    fn description(&self) -> &str {
        "Read OpenPencil design state (get_*/list_*/snapshot_*/read_nodes/batch_get/...). \
         Args: {tool, arguments}."
    }

    fn input_schema(&self) -> Value {
        schema("read")
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        dispatch(&self.deps, input, false).await
    }
}

#[async_trait]
impl Tool for OpWriteTool {
    fn name(&self) -> &str {
        "op_write"
    }

    fn description(&self) -> &str {
        "Mutate an OpenPencil design (insert/update/delete/move/page/vars or batch_design DSL). \
         Args: {tool, arguments}."
    }

    fn input_schema(&self) -> Value {
        schema("write")
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        dispatch(&self.deps, input, true).await
    }
}

/// Generates a whole OpenPencil UI from a prompt via the design pipeline
/// (skeleton → content → refine). Mutating, so it is approval-gated.
#[derive(Debug)]
pub struct OpDesignTool {
    deps: OpDesignDeps,
}

impl OpDesignTool {
    pub fn new(deps: OpDesignDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Tool for OpDesignTool {
    fn name(&self) -> &str {
        "op_design"
    }

    fn description(&self) -> &str {
        "Generate a whole OpenPencil UI from a prompt via the design pipeline \
         (skeleton→content→refine). Use for full-page/section generation; use \
         op_write for single edits."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prompt"],
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What UI to generate (e.g. a SaaS pricing page)"
                }
            }
        })
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let prompt = input
            .get("prompt")
            .and_then(|p| p.as_str())
            .ok_or_else(|| AgentError::other("op_design: missing 'prompt'"))?;
        let client =
            OpConnection::ensure(&self.deps.cfg, self.deps.consent.as_ref(), &self.deps.tag)
                .await
                .map_err(|e| AgentError::other(e.to_string()))?;
        // `deps.skills` is an `Arc<SkillRegistry>`; pass it by reference.
        let guidance = load_guidance(
            self.deps.skills.as_ref(),
            &["frontend-design", "openpencil-design"],
        );
        let generator = DirectLlmContentGenerator {
            provider: self.deps.provider.clone(),
            model: self.deps.model.clone(),
        };
        let res = DesignOrchestrator
            .run(&client, &generator, &guidance, prompt)
            .await
            .map_err(|e| AgentError::other(e.to_string()))?;
        Ok(json!({
            "sections": res.section_ids,
            "failures": res.failures,
            "refine": res.refine,
        }))
    }
}

#[cfg(test)]
mod test_helpers {
    use super::*;

    #[derive(Debug)]
    struct NoConsent;

    #[async_trait]
    impl Consent for NoConsent {
        async fn confirm(&self, _p: &str) -> bool {
            false
        }
    }

    fn deps() -> OpToolDeps {
        OpToolDeps {
            cfg: OpenPencilConfig::default(),
            consent: Arc::new(NoConsent),
            tag: "0.8.0".into(),
        }
    }

    impl OpReadTool {
        pub(crate) fn new_for_test() -> Self {
            Self { deps: deps() }
        }
    }

    impl OpWriteTool {
        pub(crate) fn new_for_test() -> Self {
            Self { deps: deps() }
        }
    }

    impl OpDesignTool {
        pub(crate) fn new_for_test() -> Self {
            Self {
                deps: OpDesignDeps {
                    cfg: OpenPencilConfig::default(),
                    consent: Arc::new(NoConsent),
                    tag: "0.8.0".into(),
                    provider: Arc::new(agent::testing::MockProvider::new(Vec::new())),
                    model: "test-model".into(),
                    skills: Arc::new(agent::skills::SkillRegistry::new()),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::tool::{SafetyClass, Tool};

    #[test]
    fn classes_and_names() {
        assert_eq!(
            OpReadTool::new_for_test().safety_class(),
            SafetyClass::ReadOnly
        );
        assert_eq!(
            OpWriteTool::new_for_test().safety_class(),
            SafetyClass::Mutating
        );
        assert_eq!(OpReadTool::new_for_test().name(), "op_read");
        assert_eq!(OpWriteTool::new_for_test().name(), "op_write");
        assert_eq!(OpDesignTool::new_for_test().name(), "op_design");
        assert_eq!(
            OpDesignTool::new_for_test().safety_class(),
            SafetyClass::Mutating
        );
    }

    #[test]
    fn read_classification_covers_real_reads() {
        for t in [
            "get_document_info",
            "list_pages",
            "snapshot_layout",
            "read_nodes",
            "batch_get",
            "export_design_md",
            "search_all_unique_properties",
        ] {
            assert!(is_read_tool(t), "{t} should be read");
        }
        for t in [
            "insert_node",
            "batch_design",
            "delete_node",
            "set_node_fill_hex",
        ] {
            assert!(!is_read_tool(t), "{t} should be write");
        }
    }
}
