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

use futures::future::BoxFuture;
use rquickjs::{async_with, AsyncContext, AsyncRuntime, CatchResultExt};
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
/// Concurrency is capped at 2: Task's recursion guard counts depth in a
/// thread-local, so concurrent calls polled on one thread stack up against
/// `DEFAULT_MAX_DEPTH` (3) as if they were nested.
pub struct GatedTaskRunner {
    tools: Arc<agent::tool::ToolRegistry>,
    cwd: std::path::PathBuf,
    file_cache: Arc<agent::file_cache::FileStateCache>,
    permissions: Arc<agent::permission::PermissionManager>,
    hooks: Arc<agent::hook::HookRunner>,
    abort: agent::abort::AbortController,
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
    ) -> Self {
        Self {
            tools,
            cwd,
            file_cache,
            permissions,
            hooks,
            abort,
            sem: Arc::new(tokio::sync::Semaphore::new(2)),
        }
    }
}

impl JsAgentRunner for GatedTaskRunner {
    fn run(
        &self,
        prompt: String,
        agent_type: String,
        description: Option<String>,
    ) -> BoxFuture<'static, Result<String, String>> {
        let tool = self.tools.get("Task");
        let ctx = agent::tool::ToolUseContext {
            cwd: self.cwd.clone(),
            abort: self.abort.clone(),
            file_cache: self.file_cache.clone(),
            permissions: self.permissions.clone(),
            hooks: self.hooks.clone(),
        };
        let sem = self.sem.clone();
        Box::pin(async move {
            let _permit = sem.acquire_owned().await.map_err(|e| e.to_string())?;
            let tool = tool.ok_or_else(|| "Task tool unavailable".to_string())?;
            let input = serde_json::json!({
                "prompt": prompt,
                "agent_type": agent_type,
                "description": description,
            });
            let out = tool.call(&ctx, input).await.map_err(|e| e.to_string())?;
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
  const raw = await __agent(String(prompt), type, desc);
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

    // Everything captured by the async_with block must be owned (the macro
    // uplifts the future's lifetime).
    let out: Result<String, String> = async_with!(ctx => |ctx| {
        run_in_ctx(ctx, &script, &args_json, runner, log).await
    })
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

    // __agent(prompt, type, desc) -> JSON envelope string (see PRELUDE).
    let bridge = move |prompt: String, agent_type: String, description: String| {
        let runner = runner.clone();
        async move {
            let desc = (!description.is_empty()).then_some(description);
            let env = match runner.run(prompt, agent_type, desc).await {
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
    use std::sync::Mutex;

    /// Echoes `<type>:<prompt>` after a tick, so concurrency is observable.
    struct EchoRunner;
    impl JsAgentRunner for EchoRunner {
        fn run(
            &self,
            prompt: String,
            agent_type: String,
            _description: Option<String>,
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
