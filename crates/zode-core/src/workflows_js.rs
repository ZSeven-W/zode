//! JS-scripted workflows: a QuickJS runtime that runs a user's orchestration
//! script with three bridges — `agent()` (dispatch a sub-agent and await its
//! final text), `log()` (progress line to the UI), and a JS prelude providing
//! `parallel()` / `pipeline()` sugar on top of `agent()`.
//!
//! A JS workflow file uses the same frontmatter as the Markdown kind, with a
//! script body instead of a step list:
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
//! The body is wrapped in an async IIFE, so top-level `await` and `return`
//! both work. The script is pure orchestration: no filesystem, no network —
//! the only capabilities are the bridges the host registers.

use std::sync::Arc;

use futures::{future::BoxFuture, StreamExt};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt};
use serde_json::Value;

/// Dispatches one `agent()` call. Implemented by the engine on top of the
/// gated `Task` tool so approvals, sandboxing, the recursion guard, and the
/// sidebar's sub-agent section all behave exactly like model-initiated tasks.
pub trait JsAgentRunner: Send + Sync {
    fn run(
        &self,
        prompt: String,
        agent_type: String,
        description: Option<String>,
        mode: Option<String>,
    ) -> BoxFuture<'static, Result<String, String>>;
}

/// Progress sink for `log()` lines (the TUI routes them into the transcript).
pub type LogSink = Arc<dyn Fn(String) + Send + Sync>;

/// Parse-only validation of a script body: build (but never call) the same
/// async wrapper `run_js_workflow` uses, so top-level `await`/`return` are
/// legal and syntax errors surface before the file is saved.
pub fn syntax_check(body: &str) -> Result<(), String> {
    let rt = rquickjs::Runtime::new().map_err(|e| format!("js runtime: {e}"))?;
    let ctx = rquickjs::Context::full(&rt).map_err(|e| format!("js context: {e}"))?;
    ctx.with(|ctx| {
        ctx.eval::<(), _>(format!("void (async () => {{\n{body}\n}});").as_bytes())
            .catch(&ctx)
            .map_err(|e| e.to_string())
    })
}

/// [`JsAgentRunner`] over a tool registry's gated `Task` tool: every `agent()`
/// call behaves exactly like a model-initiated Task (approvals, sandboxing,
/// recursion guard, sidebar sub-agent visibility).
///
/// Concurrency is capped at 2 to bound workflow fan-out. Task depth is copied
/// from the caller's `ToolUseContext`, so entering a workflow cannot reset the
/// explicit parent → child recursion chain.
pub struct GatedTaskRunner {
    tools: Arc<agent::tool::ToolRegistry>,
    cwd: std::path::PathBuf,
    file_cache: Arc<agent::file_cache::FileStateCache>,
    permissions: Arc<agent::permission::PermissionManager>,
    hooks: Arc<agent::hook::HookRunner>,
    abort: agent::abort::AbortController,
    task_depth: usize,
    sem: Arc<tokio::sync::Semaphore>,
}

impl GatedTaskRunner {
    pub fn new(
        tools: Arc<agent::tool::ToolRegistry>,
        cwd: std::path::PathBuf,
        file_cache: Arc<agent::file_cache::FileStateCache>,
        permissions: Arc<agent::permission::PermissionManager>,
        hooks: Arc<agent::hook::HookRunner>,
        abort: agent::abort::AbortController,
        task_depth: usize,
    ) -> Self {
        Self {
            tools,
            cwd,
            file_cache,
            permissions,
            hooks,
            abort,
            task_depth,
            sem: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}

/// Dispatch one workflow-originated Task through the same permission, hook,
/// abort, and side-effect tracking path used by the regular query loop.
async fn dispatch_task(
    tools: Arc<agent::tool::ToolRegistry>,
    ctx: agent::tool::ToolUseContext,
    input: Value,
) -> Result<Value, String> {
    use agent::hook::{HookEvent, HookOutcome};
    use agent::permission::PermissionDecision;
    use agent::stream::{Event, RequestedToolUse, ToolExecutor};

    const TOOL_NAME: &str = "Task";

    match ctx.permissions.evaluate(TOOL_NAME, &input, None) {
        PermissionDecision::Allow(_) => {}
        PermissionDecision::Ask(ask) => {
            let _ = ctx
                .hooks
                .run_with_abort(
                    &HookEvent::OnPermissionRequest {
                        tool: TOOL_NAME.to_string(),
                        input,
                    },
                    &ctx.abort,
                )
                .await;
            let _ = ctx
                .hooks
                .run_with_abort(
                    &HookEvent::OnPermissionDenied {
                        tool: TOOL_NAME.to_string(),
                        reason: ask.message_text.clone(),
                    },
                    &ctx.abort,
                )
                .await;
            return Err(format!(
                "Tool '{TOOL_NAME}' requires manual approval and no external queue is wired \
                 (ask: {}).",
                ask.message_text
            ));
        }
        PermissionDecision::Deny(deny) => {
            let _ = ctx
                .hooks
                .run_with_abort(
                    &HookEvent::OnPermissionDenied {
                        tool: TOOL_NAME.to_string(),
                        reason: deny.message_text.clone(),
                    },
                    &ctx.abort,
                )
                .await;
            return Err(format!("Tool '{TOOL_NAME}' denied: {}", deny.message_text));
        }
    }

    let before = HookEvent::BeforeToolUse {
        tool: TOOL_NAME.to_string(),
        input: input.clone(),
    };
    if matches!(
        ctx.hooks.run_with_abort(&before, &ctx.abort).await,
        HookOutcome::Block
    ) {
        return Err(format!("Tool '{TOOL_NAME}' blocked by BeforeToolUse hook"));
    }
    let _ = ctx
        .hooks
        .run_with_abort(
            &HookEvent::OnPermissionAllowed {
                tool: TOOL_NAME.to_string(),
            },
            &ctx.abort,
        )
        .await;

    let request = RequestedToolUse {
        id: "workflow-task".to_string(),
        name: TOOL_NAME.to_string(),
        input: input.clone(),
    };
    let mut stream = ToolExecutor::dispatch(vec![request], tools, ctx.clone(), 1);
    let event = stream
        .next()
        .await
        .ok_or_else(|| "Task tool dispatch ended without a result".to_string())?
        .map_err(|error| error.to_string())?;

    match event {
        Event::ToolResult {
            ok: true, output, ..
        } => {
            let _ = ctx
                .hooks
                .run_with_abort(
                    &HookEvent::AfterToolUse {
                        tool: TOOL_NAME.to_string(),
                        input,
                        output: output.clone(),
                        ok: true,
                    },
                    &ctx.abort,
                )
                .await;
            Ok(output)
        }
        Event::ToolResult {
            ok: false, output, ..
        } => {
            let error = output
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let _ = ctx
                .hooks
                .run_with_abort(
                    &HookEvent::PostToolUseFailure {
                        tool: TOOL_NAME.to_string(),
                        input,
                        error: error.clone(),
                    },
                    &ctx.abort,
                )
                .await;
            Err(error)
        }
        _ => Err("Task tool dispatch returned an unexpected event".to_string()),
    }
}

impl JsAgentRunner for GatedTaskRunner {
    fn run(
        &self,
        prompt: String,
        agent_type: String,
        description: Option<String>,
        mode: Option<String>,
    ) -> BoxFuture<'static, Result<String, String>> {
        let tools = self.tools.clone();
        let ctx = agent::tool::ToolUseContext {
            cwd: self.cwd.clone(),
            abort: self.abort.clone(),
            file_cache: self.file_cache.clone(),
            permissions: self.permissions.clone(),
            hooks: self.hooks.clone(),
            task_depth: self.task_depth,
        };
        let sem = self.sem.clone();
        Box::pin(async move {
            let _permit = sem.acquire_owned().await.map_err(|e| e.to_string())?;
            let mut input = serde_json::json!({
                "prompt": prompt,
                "agent_type": agent_type,
                "description": description,
            });
            if let Some(mode) = mode {
                input["mode"] = Value::String(mode);
            }
            let out = dispatch_task(tools, ctx, input).await?;
            Ok(out
                .get("output")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| out.to_string()))
        })
    }
}

/// `agent()` / `log()` / `parallel()` / `pipeline()` on top of the raw
/// bridges. `__agent` returns a JSON envelope instead of throwing across the
/// FFI boundary; `agent()` unwraps it and throws a real JS Error on failure.
const PRELUDE: &str = r#"
globalThis.log = (m) => __log(typeof m === "string" ? m : JSON.stringify(m));
globalThis.agent = async (prompt, opts = {}) => {
  const type = String(opts.type ?? opts.agent_type ?? "general");
  const desc = opts.description == null ? "" : String(opts.description);
  if (opts.mode != null && typeof opts.mode !== "string") {
    throw new TypeError("agent option 'mode' must be a string");
  }
  const hasMode = opts.mode != null;
  const mode = hasMode ? opts.mode : "";
  const raw = await __agent(String(prompt), type, desc, mode, hasMode);
  const res = JSON.parse(raw);
  if (!res.ok) throw new Error(res.error);
  return res.text;
};
globalThis.parallel = (thunks) =>
  Promise.all(
    thunks.map((t) =>
      Promise.resolve()
        .then(t)
        .catch((e) => {
          log(`parallel: ${e && e.message ? e.message : e}`);
          return null;
        })
    )
  );
globalThis.pipeline = (items, ...stages) =>
  Promise.all(
    items.map(async (item, i) => {
      let v = item;
      for (const s of stages) {
        v = await s(v, item, i);
      }
      return v;
    })
  );
"#;

/// Run one JS workflow body to completion. `args` is exposed to the script as
/// the global `args`; the script's `return` value comes back JSON-serialized
/// (`null` for `undefined`). Errors — syntax, thrown exceptions, bridge
/// failures — come back as `Err(message)`.
pub async fn run_js_workflow(
    body: &str,
    args: Value,
    runner: Arc<dyn JsAgentRunner>,
    log: LogSink,
) -> Result<Value, String> {
    let rt = AsyncRuntime::new().map_err(|e| format!("js runtime: {e}"))?;
    let ctx = AsyncContext::full(&rt)
        .await
        .map_err(|e| format!("js context: {e}"))?;
    // Wrap the body so top-level `await` and `return` both work.
    let script = format!("(async () => {{\n{body}\n}})()");
    let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "null".into());

    let out: Result<String, String> = ctx
        .async_with(async |ctx| run_in_ctx(ctx, &script, &args_json, runner, log).await)
        .await;
    let json = out?;
    serde_json::from_str(&json).map_err(|e| format!("workflow result not JSON: {e}"))
}

/// Everything that needs the `'js` lifetime, factored out of the macro block.
async fn run_in_ctx(
    ctx: rquickjs::Ctx<'_>,
    script: &str,
    args_json: &str,
    runner: Arc<dyn JsAgentRunner>,
    log: LogSink,
) -> Result<String, String> {
    use rquickjs::function::{Async, Func};

    let fmt = |e: rquickjs::CaughtError<'_>| e.to_string();

    // __agent(prompt, type, desc, mode, hasMode) -> JSON envelope string (see PRELUDE).
    let bridge = move |prompt: String,
                       agent_type: String,
                       description: String,
                       mode: String,
                       has_mode: bool| {
        let runner = runner.clone();
        async move {
            let desc = (!description.is_empty()).then_some(description);
            let mode = has_mode.then_some(mode);
            let env = match runner.run(prompt, agent_type, desc, mode).await {
                Ok(text) => serde_json::json!({ "ok": true, "text": text }),
                Err(e) => serde_json::json!({ "ok": false, "error": e }),
            };
            env.to_string()
        }
    };
    ctx.globals()
        .set("__agent", Func::from(Async(bridge)))
        .map_err(|e| format!("js bridge: {e}"))?;
    let sink = log.clone();
    ctx.globals()
        .set(
            "__log",
            Func::from(move |msg: String| {
                sink(msg);
            }),
        )
        .map_err(|e| format!("js bridge: {e}"))?;

    ctx.eval::<(), _>(format!("globalThis.args = {args_json};"))
        .catch(&ctx)
        .map_err(fmt)?;
    ctx.eval::<(), _>(PRELUDE).catch(&ctx).map_err(fmt)?;

    let promise = ctx
        .eval::<rquickjs::Promise, _>(script.as_bytes())
        .catch(&ctx)
        .map_err(fmt)?;
    let value: rquickjs::Value = promise.into_future().await.catch(&ctx).map_err(fmt)?;
    if value.is_undefined() {
        return Ok("null".to_string());
    }
    match ctx.json_stringify(value).catch(&ctx).map_err(fmt)? {
        Some(s) => s.to_string().map_err(|e| format!("js stringify: {e}")),
        None => Ok("null".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent::error::AgentError;
    use agent::hook::{HookEvent, HookOutcome, HookRunner, RustHookHandler};
    use agent::permission::{PermissionManager, RuleSource};
    use agent::tool::{Tool, ToolRegistry, ToolUseContext};
    use async_trait::async_trait;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    #[derive(Debug)]
    struct CountingTask {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl Tool for CountingTask {
        fn name(&self) -> &str {
            "Task"
        }

        fn description(&self) -> &str {
            "Test Task tool."
        }

        fn input_schema(&self) -> Value {
            serde_json::json!({"type": "object"})
        }

        async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(serde_json::json!({
                "output": input["prompt"].as_str().unwrap_or_default()
            }))
        }
    }

    fn gated_runner(
        calls: Arc<AtomicUsize>,
        permissions: PermissionManager,
        hooks: HookRunner,
    ) -> GatedTaskRunner {
        let mut tools = ToolRegistry::new();
        tools.register(Arc::new(CountingTask { calls }));
        GatedTaskRunner::new(
            Arc::new(tools),
            std::env::temp_dir(),
            Arc::new(agent::file_cache::FileStateCache::new(
                NonZeroUsize::new(8).unwrap(),
                1 << 20,
            )),
            Arc::new(permissions),
            Arc::new(hooks),
            agent::abort::AbortController::new(),
            0,
        )
    }

    /// Echoes `<type>:<prompt>` after a tick, so concurrency is observable.
    struct EchoRunner;
    impl JsAgentRunner for EchoRunner {
        fn run(
            &self,
            prompt: String,
            agent_type: String,
            _description: Option<String>,
            _mode: Option<String>,
        ) -> BoxFuture<'static, Result<String, String>> {
            Box::pin(async move {
                tokio::task::yield_now().await;
                if prompt.contains("boom") {
                    return Err("boom".to_string());
                }
                Ok(format!("{agent_type}:{prompt}"))
            })
        }
    }

    fn no_log() -> LogSink {
        Arc::new(|_| {})
    }

    fn capture_log() -> (LogSink, Arc<Mutex<Vec<String>>>) {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let sink = lines.clone();
        (Arc::new(move |m| sink.lock().unwrap().push(m)), lines)
    }

    async fn run(body: &str) -> Result<Value, String> {
        run_js_workflow(body, Value::Null, Arc::new(EchoRunner), no_log()).await
    }

    #[tokio::test]
    async fn returns_agent_result() {
        let v = run(r#"return await agent("hi", { type: "researcher" });"#)
            .await
            .unwrap();
        assert_eq!(v, serde_json::json!("researcher:hi"));
    }

    #[tokio::test]
    async fn forwards_string_agent_mode_without_validating_supported_values() {
        struct ModeRunner;
        impl JsAgentRunner for ModeRunner {
            fn run(
                &self,
                _prompt: String,
                _agent_type: String,
                _description: Option<String>,
                mode: Option<String>,
            ) -> BoxFuture<'static, Result<String, String>> {
                Box::pin(async move { Ok(mode.unwrap_or_else(|| "missing".to_string())) })
            }
        }

        let v = run_js_workflow(
            r#"return await agent("hi", { mode: "future-mode" });"#,
            Value::Null,
            Arc::new(ModeRunner),
            no_log(),
        )
        .await
        .unwrap();

        assert_eq!(v, serde_json::json!("future-mode"));
    }

    #[tokio::test]
    async fn rejects_non_string_agent_modes_before_task_dispatch() {
        for value in ["1", "[\"plan\"]", "{ toString() { return \"plan\"; } }"] {
            let script = format!("return await agent(\"hi\", {{ mode: {value} }});");
            let error = run_js_workflow(&script, Value::Null, Arc::new(EchoRunner), no_log())
                .await
                .unwrap_err();
            assert!(error.contains("mode"), "{error}");
            assert!(error.contains("string"), "{error}");
        }
    }

    #[tokio::test]
    async fn workflow_agent_obeys_hard_task_denies_before_dispatch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let runner = gated_runner(
            calls.clone(),
            PermissionManager::new()
                .allow(RuleSource::User, "Task")
                .deny(RuleSource::Policy, "Task"),
            HookRunner::new(),
        );

        let error = runner
            .run(
                "must not run".into(),
                "general".into(),
                None,
                Some("plan".into()),
            )
            .await
            .unwrap_err();

        assert!(error.contains("denied"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn workflow_agent_obeys_before_task_hook_before_dispatch() {
        let calls = Arc::new(AtomicUsize::new(0));
        let hooks = HookRunner::new().with(Arc::new(RustHookHandler::new(
            "block-workflow-task",
            |event| match event {
                HookEvent::BeforeToolUse { tool, .. } if tool == "Task" => HookOutcome::Block,
                _ => HookOutcome::Ok,
            },
        )));
        let runner = gated_runner(
            calls.clone(),
            PermissionManager::new().allow(RuleSource::User, "Task"),
            hooks,
        );

        let error = runner
            .run("must not run".into(), "general".into(), None, None)
            .await
            .unwrap_err();

        assert!(error.contains("BeforeToolUse"), "{error}");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn parallel_collects_and_nulls_failures() {
        let v = run(r#"
            const r = await parallel([
              () => agent("a"),
              () => agent("boom"),
              () => agent("c", { type: "reviewer" }),
            ]);
            return r;
            "#)
        .await
        .unwrap();
        assert_eq!(v, serde_json::json!(["general:a", null, "reviewer:c"]));
    }

    #[tokio::test]
    async fn pipeline_threads_items_through_stages() {
        let v = run(r#"
            return await pipeline(
              ["x", "y"],
              (item) => agent(item),
              (prev, item, i) => `${i}:${prev}|${item}`
            );
            "#)
        .await
        .unwrap();
        assert_eq!(v, serde_json::json!(["0:general:x|x", "1:general:y|y"]));
    }

    #[tokio::test]
    async fn args_are_exposed_and_log_reaches_the_sink() {
        let (log, lines) = capture_log();
        let v = run_js_workflow(
            r#"log(`got ${args.n}`); return args.n * 2;"#,
            serde_json::json!({ "n": 21 }),
            Arc::new(EchoRunner),
            log,
        )
        .await
        .unwrap();
        assert_eq!(v, serde_json::json!(42));
        assert_eq!(lines.lock().unwrap().as_slice(), ["got 21"]);
    }

    #[tokio::test]
    async fn thrown_errors_and_syntax_errors_surface() {
        let e = run(r#"throw new Error("nope");"#).await.unwrap_err();
        assert!(e.contains("nope"), "{e}");
        let e = run(r#"const = broken"#).await.unwrap_err();
        assert!(!e.is_empty());
    }

    #[tokio::test]
    async fn undefined_result_becomes_null() {
        let v = run(r#"log("side effect only");"#).await.unwrap();
        assert_eq!(v, Value::Null);
    }

    #[test]
    fn syntax_check_accepts_await_return_and_rejects_typos() {
        assert!(syntax_check("return await agent(\"x\");").is_ok());
        assert!(syntax_check("const = broken").is_err());
    }
}
