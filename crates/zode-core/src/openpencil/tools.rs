//! `OpRead` (ReadOnly, ungated) and `OpWrite` (Mutating, gated) agent tools.
//! `safety_class()` is static per tool, so read vs write must be two tools.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::connection::OpConnection;
use super::design::{load_guidance, DesignOrchestrator, DirectLlmContentGenerator};
use super::{is_read_tool, Consent, OpError};
use crate::config::OpenPencilConfig;
use crate::question::QuestionQueue;
use crate::task_factory::ModelRuntimeState;

/// Shared deps: enough to `ensure` a client per call.
#[derive(Debug, Clone)]
pub struct OpToolDeps {
    pub cfg: OpenPencilConfig,
    pub consent: Arc<dyn Consent>,
    pub tag: String,
}

/// Deps for `OpDesign` — SEPARATE from `OpToolDeps`. `OpRead`/`OpWrite` are
/// registered before the skills registry exists, so their deps stay narrow
/// (cfg/consent/tag); only `OpDesign` (registered after skills) needs the
/// provider/model/skills to drive the design pipeline.
#[derive(Clone, Debug)]
pub struct OpDesignDeps {
    pub cfg: OpenPencilConfig,
    pub consent: Arc<dyn Consent>,
    pub tag: String,
    pub model_runtime: ModelRuntimeState,
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
async fn dispatch(
    deps: &OpToolDeps,
    ctx: &ToolUseContext,
    input: Value,
    writing: bool,
) -> Result<Value, AgentError> {
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
    let client = OpConnection::ensure(&deps.cfg, deps.consent.as_ref(), &deps.tag, &ctx.abort)
        .await
        .map_err(map_op_error)?;
    call_remote(&client, tool, args, writing, &ctx.abort).await
}

async fn call_remote(
    client: &super::client::OpClient,
    tool: &str,
    args: Value,
    writing: bool,
    abort: &agent::abort::AbortController,
) -> Result<Value, AgentError> {
    if abort.is_aborted() {
        return Err(aborted(abort));
    }
    abort.pulse();
    let request = client.call(tool, args);
    tokio::pin!(request);
    tokio::select! {
        biased;
        _ = abort.cancelled() => {
            // Dropping the HTTP future stops local work, but once a mutating
            // request has reached the live editor its commit status cannot be
            // proven. Keep scheduler recovery fail-closed.
            if writing {
                abort.mark_unresolved_external_work();
            }
            Err(aborted(abort))
        }
        result = &mut request => {
            abort.pulse();
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    // A transport/RPC/parse failure after a write was sent is
                    // not proof that the editor rolled it back.
                    if writing {
                        abort.mark_unresolved_external_work();
                    }
                    Err(map_op_error(error))
                }
            }
        }
    }
}

fn map_op_error(error: OpError) -> AgentError {
    match error {
        OpError::Aborted(reason) => AgentError::Aborted(reason),
        other => AgentError::other(other.to_string()),
    }
}

fn aborted(abort: &agent::abort::AbortController) -> AgentError {
    AgentError::Aborted(abort.reason().unwrap_or_else(|| "aborted".to_string()))
}

fn map_design_error(
    error: OpError,
    abort: &agent::abort::AbortController,
    remote_started: bool,
) -> AgentError {
    if remote_started {
        // `Planned` is emitted immediately before `design_skeleton` dispatch.
        // Any later failure may therefore describe a partially committed page.
        abort.mark_unresolved_external_work();
    }
    if abort.is_aborted() {
        aborted(abort)
    } else {
        map_op_error(error)
    }
}

fn design_result_requires_review(result: &super::design::DesignResult) -> bool {
    !result.failures.is_empty()
        || result
            .refine
            .get("error")
            .is_some_and(|error| !error.is_null())
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
        "OpRead"
    }

    fn description(&self) -> &str {
        "Read OpenPencil design state (get_*/list_*/snapshot_*/read_nodes/batch_get/...). \
         Args: {tool, arguments}. Connection setup may install or launch OpenPencil; a detached \
         GUI launch is reported to scheduler safety state."
    }

    fn input_schema(&self) -> Value {
        schema("read")
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        dispatch(&self.deps, ctx, input, false).await
    }
}

#[async_trait]
impl Tool for OpWriteTool {
    fn name(&self) -> &str {
        "OpWrite"
    }

    fn description(&self) -> &str {
        "Mutate an OpenPencil design (insert/update/delete/move/page/vars or batch_design DSL). \
         Args: {tool, arguments}. Cancellation after dispatch is treated as an unverified remote \
         outcome and is never auto-replayed."
    }

    fn input_schema(&self) -> Value {
        schema("write")
    }

    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        dispatch(&self.deps, ctx, input, true).await
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
        "OpDesign"
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

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let prompt = input
            .get("prompt")
            .and_then(|p| p.as_str())
            .ok_or_else(|| AgentError::other("op_design: missing 'prompt'"))?;
        let abort = ctx.abort.child();
        let client = OpConnection::ensure(
            &self.deps.cfg,
            self.deps.consent.as_ref(),
            &self.deps.tag,
            &abort,
        )
        .await
        .map_err(map_op_error)?;
        // `deps.skills` is an `Arc<SkillRegistry>`; pass it by reference.
        let guidance = load_guidance(
            self.deps.skills.as_ref(),
            &["frontend-design", "openpencil-design"],
        );
        let runtime = self.deps.model_runtime.snapshot();
        let generator = DirectLlmContentGenerator {
            provider: runtime.provider,
            model: runtime.model,
        };
        let activity = abort.activity();
        let remote_started = Arc::new(AtomicBool::new(false));
        let progress_remote_started = remote_started.clone();
        let progress = move |event| {
            if matches!(event, super::design::DesignProgress::Planned { .. }) {
                progress_remote_started.store(true, Ordering::Release);
            }
            activity.pulse();
        };
        abort.mark_side_effect_risk();
        let run = DesignOrchestrator.run(&client, &generator, &guidance, prompt, &abort, &progress);
        tokio::pin!(run);
        let res = tokio::select! {
            biased;
            _ = abort.cancelled() => {
                // The pipeline may have committed skeleton/content/refine
                // remotely before cancellation reached this local future.
                if remote_started.load(Ordering::Acquire) {
                    abort.mark_unresolved_external_work();
                }
                return Err(aborted(&abort));
            }
            result = &mut run => result.map_err(|error| {
                map_design_error(
                    error,
                    &abort,
                    remote_started.load(Ordering::Acquire),
                )
            })?,
        };
        if design_result_requires_review(&res) {
            // The orchestrator intentionally returns section/refine failures
            // as a partial result. A skeleton or earlier sections may already
            // exist, so scheduled recurrence must pause for human review.
            abort.mark_unresolved_external_work();
        }
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
                    model_runtime: ModelRuntimeState::new(
                        Arc::new(agent::testing::MockProvider::new(Vec::new())),
                        "test-model".into(),
                    ),
                    skills: Arc::new(agent::skills::SkillRegistry::new()),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use agent::tool::{SafetyClass, Tool};
    use async_trait::async_trait;

    #[derive(Debug)]
    struct PendingTransport;

    #[async_trait]
    impl super::super::client::Transport for PendingTransport {
        async fn post_json(
            &self,
            _url: &str,
            _body: Value,
        ) -> Result<Value, super::super::OpError> {
            std::future::pending().await
        }
    }

    #[derive(Debug)]
    struct FailingTransport;

    #[async_trait]
    impl super::super::client::Transport for FailingTransport {
        async fn post_json(
            &self,
            _url: &str,
            _body: Value,
        ) -> Result<Value, super::super::OpError> {
            Err(super::super::OpError::Http("connection reset".to_string()))
        }
    }

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
        assert_eq!(OpReadTool::new_for_test().name(), "OpRead");
        assert_eq!(OpWriteTool::new_for_test().name(), "OpWrite");
        assert_eq!(OpDesignTool::new_for_test().name(), "OpDesign");
        assert_eq!(
            OpDesignTool::new_for_test().safety_class(),
            SafetyClass::Mutating
        );
    }

    #[test]
    fn read_classification_covers_real_reads() {
        for t in [
            "open_document",
            "get_document_info",
            "get_selection",
            "get_node",
            "get_node_children",
            "get_node_parent",
            "list_pages",
            "list_variables",
            "get_variables",
            "conversion_status",
            "lint_document",
            "list_theme_presets",
            "get_design_md",
            "export_design_md",
            "get_style_guide_tags",
            "get_style_guide",
            "get_guidelines",
            "ToolSearch",
            "get_screenshot",
            "get_active_theme",
            "list_components",
            "get_component",
            "snapshot_layout",
            "find_empty_space",
            "get_canvas_bounds",
            "find_node_by_name",
            "count_nodes",
            "list_node_kinds",
            "get_history_depth",
            "get_viewport",
            "get_selection_set",
            "get_editor_state",
            "read_nodes",
            "batch_get",
            "search_all_unique_properties",
        ] {
            assert!(is_read_tool(t), "{t} should be read");
        }
        for t in [
            "save_document",
            "upsert_variables",
            "upsert_component",
            "upsert_screen",
            "save_theme_preset",
            "load_theme_preset",
            "set_design_md",
            "spawn_agents",
            "export_nodes",
            "codegen_plan",
            "codegen_submit_chunk",
            "codegen_assemble",
            "codegen_clean",
            "replace_all_matching_properties",
            "batch_design",
            "design_skeleton",
            "design_content",
            "design_refine",
            "insert_node",
            "delete_node",
            "set_node_fill_hex",
        ] {
            assert!(!is_read_tool(t), "{t} should be write");
        }
    }

    #[tokio::test]
    async fn cancelled_remote_write_is_latched_as_unresolved() {
        let client = super::super::client::OpClient::new(
            "http://127.0.0.1:1".to_string(),
            Arc::new(PendingTransport),
        );
        let abort = agent::abort::AbortController::new();
        let cancel = abort.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancel.abort_with_reason("watchdog");
        });

        let result = call_remote(&client, "insert_node", json!({}), true, &abort).await;
        assert!(matches!(result, Err(AgentError::Aborted(reason)) if reason == "watchdog"));
        assert!(abort.activity().side_effect_risk());
        assert!(abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn cancelled_remote_read_does_not_invent_a_side_effect() {
        let client = super::super::client::OpClient::new(
            "http://127.0.0.1:1".to_string(),
            Arc::new(PendingTransport),
        );
        let abort = agent::abort::AbortController::new();
        let cancel = abort.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancel.abort_with_reason("watchdog");
        });

        let result = call_remote(&client, "get_node", json!({}), false, &abort).await;
        assert!(matches!(result, Err(AgentError::Aborted(reason)) if reason == "watchdog"));
        assert!(!abort.activity().side_effect_risk());
        assert!(!abort.activity().unresolved_external_work());
    }

    #[tokio::test]
    async fn failed_remote_write_is_latched_as_unresolved() {
        let client = super::super::client::OpClient::new(
            "http://127.0.0.1:1".to_string(),
            Arc::new(FailingTransport),
        );
        let abort = agent::abort::AbortController::new();
        let result = call_remote(&client, "insert_node", json!({}), true, &abort).await;
        assert!(result.is_err());
        assert!(abort.activity().unresolved_external_work());
    }

    #[test]
    fn design_error_after_remote_boundary_is_latched() {
        let abort = agent::abort::AbortController::new();
        let error = map_design_error(
            super::super::OpError::Parse("missing root id".to_string()),
            &abort,
            true,
        );
        assert!(matches!(error, AgentError::Other(_)));
        assert!(abort.activity().unresolved_external_work());
    }

    #[test]
    fn design_plan_error_before_remote_boundary_is_not_latched() {
        let abort = agent::abort::AbortController::new();
        let error = map_design_error(
            super::super::OpError::Parse("bad model plan".to_string()),
            &abort,
            false,
        );
        assert!(matches!(error, AgentError::Other(_)));
        assert!(!abort.activity().unresolved_external_work());
    }

    #[test]
    fn partial_design_result_requires_review() {
        let partial = super::super::design::DesignResult {
            section_ids: vec!["section-1".to_string()],
            refine: json!({"error": "connection reset"}),
            failures: Vec::new(),
        };
        assert!(design_result_requires_review(&partial));

        let complete = super::super::design::DesignResult {
            section_ids: vec!["section-1".to_string()],
            refine: json!({"ok": true}),
            failures: Vec::new(),
        };
        assert!(!design_result_requires_review(&complete));
    }
}
