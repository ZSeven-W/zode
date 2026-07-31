//! Host-side mode routing for the Task tool.
//!
//! The vendored Task runtime intentionally knows only about `agent_type`.
//! Zode keeps execution mode orthogonal by routing one Task call to a factory
//! configured for that mode. A mode applies for exactly the lifetime of the
//! spawned child loop; returning the tool result restores the caller's
//! unchanged tool registry and system prompt.

use std::collections::BTreeMap;
use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

pub const INHERIT_MODE: &str = "inherit";
pub const DEFAULT_MODE_ALIAS: &str = "default";

/// Read the optional Task mode. Missing/null means `inherit`; malformed input
/// fails closed instead of silently running with broader default capabilities.
pub fn requested_task_mode(input: &Value) -> Result<&str, AgentError> {
    match input.get("mode") {
        None | Some(Value::Null) => Ok(INHERIT_MODE),
        Some(Value::String(mode)) if mode.is_empty() => {
            Err(AgentError::other("Task mode must not be empty"))
        }
        Some(Value::String(mode)) if mode.trim() != mode => Err(AgentError::other(
            "Task mode must not contain leading or trailing whitespace",
        )),
        Some(Value::String(mode)) => Ok(mode),
        Some(_) => Err(AgentError::other("Task mode must be a string")),
    }
}

pub fn is_inherit_mode(mode: &str) -> bool {
    matches!(mode, INHERIT_MODE | DEFAULT_MODE_ALIAS)
}

/// Runs cheap Task validation before any outer permission/trust decorator.
/// The upstream TaskTool repeats the depth check as the authoritative guard;
/// this host wrapper prevents an impossible nested call from prompting first.
#[derive(Debug)]
pub struct TaskPreflightTool {
    inner: Arc<dyn Tool>,
    max_depth: usize,
    supported_modes: Option<Vec<String>>,
}

impl TaskPreflightTool {
    pub fn new(inner: Arc<dyn Tool>, max_depth: usize) -> Self {
        let supported_modes = inner
            .input_schema()
            .pointer("/properties/mode/enum")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .filter(|modes| !modes.is_empty());
        Self {
            inner,
            max_depth: max_depth.max(1),
            supported_modes,
        }
    }
}

#[async_trait]
impl Tool for TaskPreflightTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn input_schema(&self) -> Value {
        self.inner.input_schema()
    }

    fn safety_class(&self) -> SafetyClass {
        self.inner.safety_class()
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let requested = requested_task_mode(&input)?;
        if let Some(supported) = &self.supported_modes {
            if !supported.iter().any(|mode| mode == requested) {
                return Err(AgentError::other(format!(
                    "Task mode '{requested}' is not supported (try: {})",
                    supported.join(", ")
                )));
            }
        }
        if ctx.task_depth >= self.max_depth {
            return Err(AgentError::other(format!(
                "Task: max recursion depth {} reached (current depth {})",
                self.max_depth, ctx.task_depth
            )));
        }
        self.inner.call(ctx, input).await
    }
}

/// Routes Task calls to mode-specific TaskTool instances while presenting one
/// stable provider-facing `Task` definition.
#[derive(Debug)]
pub struct TaskModeRouter {
    inherit: Arc<dyn Tool>,
    modes: BTreeMap<String, Arc<dyn Tool>>,
    description: String,
}

impl TaskModeRouter {
    pub fn new(inherit: Arc<dyn Tool>) -> Self {
        let description = inherit.description().to_string();
        Self {
            inherit,
            modes: BTreeMap::new(),
            description,
        }
    }

    /// Register another immutable child-loop mode. The routed tool must expose
    /// the same Task name/schema contract as the inherited route.
    pub fn with_mode(mut self, mode: impl Into<String>, tool: Arc<dyn Tool>) -> Self {
        let mode = mode.into();
        assert!(
            !mode.is_empty() && mode.trim() == mode,
            "Task mode names must be non-empty and canonical"
        );
        assert!(
            !is_inherit_mode(&mode),
            "inherit/default are reserved Task modes"
        );
        assert_eq!(
            tool.name(),
            self.inherit.name(),
            "mode routes must expose the same tool name"
        );
        self.modes.insert(mode, tool);
        let supported = self.supported_modes().join(", ");
        self.description = format!(
            "{} Optional `mode` selects an immutable child execution mode \
             ({supported}); it never changes the caller's mode.",
            self.inherit.description()
        );
        self
    }

    fn supported_modes(&self) -> Vec<String> {
        let mut modes = vec![INHERIT_MODE.to_string(), DEFAULT_MODE_ALIAS.to_string()];
        modes.extend(self.modes.keys().cloned());
        modes
    }
}

#[async_trait]
impl Tool for TaskModeRouter {
    fn name(&self) -> &str {
        self.inherit.name()
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> Value {
        let mut schema = self.inherit.input_schema();
        let modes = self.supported_modes();
        if !schema.is_object() {
            schema = json!({"type": "object", "properties": {}});
        }
        let object = schema.as_object_mut().expect("schema normalized to object");
        let properties = object.entry("properties").or_insert_with(|| json!({}));
        if !properties.is_object() {
            *properties = json!({});
        }
        properties
            .as_object_mut()
            .expect("properties normalized to object")
            .insert(
                "mode".to_string(),
                json!({
                    "type": "string",
                    "enum": modes,
                    "default": INHERIT_MODE,
                    "description": "Child execution mode. `plan` is read-only and returns a plan; `read-only` completes normally with read-only tools; `inherit`/`default` preserve the caller's capability ceiling."
                }),
            );
        schema
    }

    fn safety_class(&self) -> SafetyClass {
        self.inherit.safety_class()
    }

    async fn call(&self, ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let requested = requested_task_mode(&input)?.to_string();
        let (effective, tool) = if is_inherit_mode(&requested) {
            (INHERIT_MODE, self.inherit.clone())
        } else {
            let tool = self.modes.get(&requested).cloned().ok_or_else(|| {
                AgentError::other(format!(
                    "Task mode '{requested}' is not supported (try: {})",
                    self.supported_modes().join(", ")
                ))
            })?;
            (requested.as_str(), tool)
        };

        let mut output = tool.call(ctx, input).await?;
        if let Some(object) = output.as_object_mut() {
            object.insert("requested_mode".to_string(), json!(&requested));
            object.insert("mode".to_string(), json!(effective));
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval::{Approval, ApprovalGate};
    use crate::gated_tool::PermissionGatedTool;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug)]
    struct RouteTool {
        calls: Arc<AtomicUsize>,
        output: &'static str,
    }

    #[async_trait]
    impl Tool for RouteTool {
        fn name(&self) -> &str {
            "Task"
        }

        fn description(&self) -> &str {
            "delegate work"
        }

        fn input_schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"prompt": {"type": "string"}},
                "required": ["prompt"]
            })
        }

        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }

        async fn call(&self, _ctx: &ToolUseContext, _input: Value) -> Result<Value, AgentError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(json!({"output": self.output}))
        }
    }

    fn route_tool(output: &'static str) -> (Arc<dyn Tool>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        (
            Arc::new(RouteTool {
                calls: calls.clone(),
                output,
            }),
            calls,
        )
    }

    #[derive(Debug)]
    struct CountingGate(Arc<AtomicUsize>);

    #[async_trait]
    impl ApprovalGate for CountingGate {
        async fn approve(&self, _tool: &str, _input: &Value) -> Approval {
            self.0.fetch_add(1, Ordering::SeqCst);
            Approval::AllowOnce
        }
    }

    #[tokio::test]
    async fn preflight_rejects_depth_and_malformed_modes_before_permission_gate() {
        let (inherit, inherit_calls) = route_tool("unexpected");
        let (plan, plan_calls) = route_tool("unexpected");
        let inner: Arc<dyn Tool> = Arc::new(TaskModeRouter::new(inherit).with_mode("plan", plan));
        let approvals = Arc::new(AtomicUsize::new(0));
        let gated: Arc<dyn Tool> = Arc::new(PermissionGatedTool::new(
            inner,
            Arc::new(CountingGate(approvals.clone())),
        ));
        let tool = TaskPreflightTool::new(gated, 3);
        let mut max_depth_ctx = ToolUseContext::new(std::env::temp_dir());
        max_depth_ctx.task_depth = 3;

        let depth_error = tool
            .call(&max_depth_ctx, json!({"prompt": "x", "mode": "plan"}))
            .await
            .unwrap_err();
        assert!(depth_error.to_string().contains("max recursion depth 3"));

        let malformed_error = tool
            .call(
                &ToolUseContext::new(std::env::temp_dir()),
                json!({"prompt": "x", "mode": 1}),
            )
            .await
            .unwrap_err();
        assert!(malformed_error.to_string().contains("must be a string"));
        let unknown_error = tool
            .call(
                &ToolUseContext::new(std::env::temp_dir()),
                json!({"prompt": "x", "mode": "future"}),
            )
            .await
            .unwrap_err();
        assert!(unknown_error.to_string().contains("not supported"));
        let whitespace_error = tool
            .call(
                &ToolUseContext::new(std::env::temp_dir()),
                json!({"prompt": "x", "mode": " plan "}),
            )
            .await
            .unwrap_err();
        assert!(whitespace_error.to_string().contains("whitespace"));
        assert_eq!(approvals.load(Ordering::SeqCst), 0);
        assert_eq!(inherit_calls.load(Ordering::SeqCst), 0);
        assert_eq!(plan_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn routes_registered_modes_and_reports_effective_mode() {
        let (inherit, inherit_calls) = route_tool("inherit");
        let (plan, plan_calls) = route_tool("plan");
        let (read_only, read_only_calls) = route_tool("read-only");
        let router = TaskModeRouter::new(inherit)
            .with_mode("plan", plan)
            .with_mode("read-only", read_only);
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let inherited = router.call(&ctx, json!({"prompt": "a"})).await.unwrap();
        let defaulted = router
            .call(&ctx, json!({"prompt": "b", "mode": "default"}))
            .await
            .unwrap();
        let planned = router
            .call(&ctx, json!({"prompt": "c", "mode": "plan"}))
            .await
            .unwrap();
        let read_only = router
            .call(&ctx, json!({"prompt": "d", "mode": "read-only"}))
            .await
            .unwrap();

        assert_eq!(inherit_calls.load(Ordering::SeqCst), 2);
        assert_eq!(plan_calls.load(Ordering::SeqCst), 1);
        assert_eq!(read_only_calls.load(Ordering::SeqCst), 1);
        assert_eq!(inherited["mode"], "inherit");
        assert_eq!(defaulted["requested_mode"], "default");
        assert_eq!(defaulted["mode"], "inherit");
        assert_eq!(planned["output"], "plan");
        assert_eq!(planned["mode"], "plan");
        assert_eq!(read_only["output"], "read-only");
        assert_eq!(read_only["mode"], "read-only");
    }

    #[tokio::test]
    async fn unknown_or_malformed_mode_fails_closed() {
        let (inherit, _) = route_tool("inherit");
        let router = TaskModeRouter::new(inherit);
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let unknown = router
            .call(&ctx, json!({"prompt": "x", "mode": "yolo"}))
            .await
            .unwrap_err();
        assert!(unknown.to_string().contains("not supported"));
        assert!(router
            .call(&ctx, json!({"prompt": "x", "mode": 1}))
            .await
            .unwrap_err()
            .to_string()
            .contains("string"));
        assert!(router
            .call(&ctx, json!({"prompt": "x", "mode": " plan "}))
            .await
            .unwrap_err()
            .to_string()
            .contains("whitespace"));
    }

    #[test]
    fn schema_exposes_registered_modes_without_making_mode_required() {
        let (inherit, _) = route_tool("inherit");
        let (plan, _) = route_tool("plan");
        let (read_only, _) = route_tool("read-only");
        let router = TaskModeRouter::new(inherit)
            .with_mode("plan", plan)
            .with_mode("read-only", read_only);
        let schema = router.input_schema();

        assert_eq!(
            schema["properties"]["mode"]["enum"],
            json!(["inherit", "default", "plan", "read-only"])
        );
        assert_eq!(schema["properties"]["mode"]["default"], "inherit");
        assert_eq!(schema["required"], json!(["prompt"]));
    }
}
