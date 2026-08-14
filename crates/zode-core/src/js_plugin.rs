//! QuickJS gene layer: agent-generated JavaScript plugins with hard
//! memory and time bounds.
//!
//! Built-in tools stay Rust; the evolvable layer is JavaScript evaluated in
//! a dedicated QuickJS runtime — no compiler needed on the target machine,
//! hot replacement is just evaluating new source, and a misbehaving gene is
//! stopped by the runtime memory limit / interrupt deadline (which surfaces
//! as a Failed fiber and quarantines through the evolution layer).
//!
//! Guest protocol — the source evaluates to a factory returning
//! 'name?, apply(host)'. The host API is 'log(level, message)',
//! 'on(event, callback)' (the callback return value becomes the dispatch
//! bail value), 'emit(event, payload)', 'effect(cleanupFn)' (runs at fiber
//! dispose), and 'config' (the plugin config).

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cordis_rs::{Context, CordisError, Flow, Plugin, PluginResult};
use rquickjs::{
    CatchResultExt, Context as JsContext, Ctx, Function as JsFunction, Runtime as JsRuntime,
    Value as JsValue,
};
use serde_json::Value;

/// Default per-runtime JS memory cap (bytes).
pub const JS_GENE_MEMORY_LIMIT: usize = 16 * 1024 * 1024;
/// Default per-callback time cap.
pub const JS_GENE_CALL_TIMEOUT_MS: u64 = 1000;

/// A plugin whose body is generated JavaScript, evaluated in a dedicated
/// bounded QuickJS runtime.
pub struct JsPlugin {
    pub name: &'static str,
    pub source: String,
    pub memory_limit: usize,
    pub call_timeout_ms: u64,
}

impl JsPlugin {
    pub fn new(name: &'static str, source: impl Into<String>) -> Self {
        JsPlugin {
            name,
            source: source.into(),
            memory_limit: JS_GENE_MEMORY_LIMIT,
            call_timeout_ms: JS_GENE_CALL_TIMEOUT_MS,
        }
    }

    pub fn with_memory_limit(mut self, bytes: usize) -> Self {
        self.memory_limit = bytes;
        self
    }

    pub fn with_call_timeout(mut self, ms: u64) -> Self {
        self.call_timeout_ms = ms;
        self
    }
}

/// Host → worker thread commands.
enum Command {
    /// Invoke the guest callback registered for 'event' with a JSON payload.
    Call {
        event: String,
        payload: String,
        reply: Sender<Result<String, String>>,
    },
    /// Run the guest cleanup (if any) and tear the runtime down.
    Shutdown,
}

/// Worker → host pump messages.
enum WorkerMessage {
    /// The guest subscribed to an event: register a host-side listener.
    Listen { event: String },
    /// The guest dispatched an event on the host bus.
    Emit { event: String, payload: Value },
}

fn js_stringify<'js>(ctx: &Ctx<'js>, value: &JsValue<'js>) -> Result<String, String> {
    match ctx
        .json_stringify(value.clone())
        .map_err(|e| format!("stringify: {e}"))?
    {
        Some(s) => s.to_string().map_err(|e| format!("stringify: {e}")),
        None => Ok("null".to_string()),
    }
}

fn run_worker(
    source: String,
    config_json: String,
    memory_limit: usize,
    call_timeout_ms: u64,
    cmd_rx: Receiver<Command>,
    msg_tx: tokio::sync::mpsc::Sender<WorkerMessage>,
    ready_tx: Sender<Result<(), String>>,
) {
    // The success handshake fires from INSIDE the with-block right after
    // apply(), before the command loop starts — the outer send covers only
    // setup/apply failures (a receiver-less send after shutdown is a no-op).
    let ready_inner = ready_tx.clone();
    let result = (|| -> Result<(), String> {
        let runtime = JsRuntime::new().map_err(|e| format!("js runtime: {e}"))?;
        runtime.set_memory_limit(memory_limit);
        let deadline: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        {
            let deadline = deadline.clone();
            runtime.set_interrupt_handler(Some(Box::new(move || {
                deadline
                    .lock()
                    .map(|d| d.map(|d| Instant::now() >= d).unwrap_or(false))
                    .unwrap_or(false)
            })));
        }
        let context = JsContext::full(&runtime).map_err(|e| format!("js context: {e}"))?;
        context.with(|ctx| {
            // The guest's callbacks never cross into Rust: they stay in the
            // JS runtime under globalThis.__zode_handlers / __zode_cleanup.
            // The Rust bridge functions take plain strings only, so nothing
            // with the 'js lifetime escapes the runtime borrow.
            let globals = ctx.globals();
            globals
                .set(
                    "__zode_log",
                    rquickjs::function::Func::from(|level: String, message: String| {
                        match level.as_str() {
                            "error" => {
                                tracing::error!(target: "cordis", gene = "js", "{}", message)
                            }
                            "warn" => {
                                tracing::warn!(target: "cordis", gene = "js", "{}", message)
                            }
                            _ => tracing::info!(target: "cordis", gene = "js", "{}", message),
                        }
                    }),
                )
                .map_err(|e| format!("__zode_log: {e}"))?;
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_emit",
                        rquickjs::function::Func::from(
                            move |event: String, payload_json: String| {
                                let payload =
                                    serde_json::from_str(&payload_json).unwrap_or(Value::Null);
                                let _ =
                                    msg_tx.blocking_send(WorkerMessage::Emit { event, payload });
                            },
                        ),
                    )
                    .map_err(|e| format!("__zode_emit: {e}"))?;
            }
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_register",
                        rquickjs::function::Func::from(move |event: String| {
                            let _ = msg_tx.blocking_send(WorkerMessage::Listen { event });
                        }),
                    )
                    .map_err(|e| format!("__zode_register: {e}"))?;
            }
            let config = ctx
                .json_parse(config_json.as_bytes())
                .map_err(|e| format!("config parse: {e}"))?;
            globals
                .set("__zode_config", config)
                .map_err(|e| format!("__zode_config: {e}"))?;
            // Guest-side registry + host facade (callbacks stay in JS).
            ctx.eval::<(), _>(
                "globalThis.__zode_handlers = {}; globalThis.__zode_cleanup = null;"
                    .as_bytes(),
            )
            .catch(&ctx)
            .map_err(|e| format!("handler registry: {e}"))?;
            ctx.eval::<(), _>(
                r#"globalThis.host = {
  log: __zode_log,
  emit: __zode_emit,
  on: function (event, cb) { globalThis.__zode_handlers[event] = cb; __zode_register(event); },
  effect: function (cb) { globalThis.__zode_cleanup = cb; },
  config: __zode_config,
};"#
                    .as_bytes(),
            )
            .catch(&ctx)
            .map_err(|e| format!("host facade: {e}"))?;

            let factory: JsFunction = ctx
                .eval(source.as_bytes())
                .catch(&ctx)
                .map_err(|e| format!("eval: {e}"))?;
            let plugin_obj_value: JsValue = factory
                .call(())
                .catch(&ctx)
                .map_err(|e| format!("factory: {e}"))?;
            let plugin_obj = plugin_obj_value
                .as_object()
                .ok_or_else(|| "plugin factory must return an object".to_string())?
                .clone();
            let apply: JsFunction = plugin_obj
                .get("apply")
                .catch(&ctx)
                .map_err(|e| format!("missing apply(): {e}"))?;
            let host = globals
                .get::<_, rquickjs::Object>("host")
                .map_err(|e| format!("host: {e}"))?;

            // Run apply() under the same deadline budget.
            *deadline.lock().unwrap() =
                Some(Instant::now() + Duration::from_millis(call_timeout_ms));
            let applied: Result<JsValue, String> = apply
                .call((host,))
                .catch(&ctx)
                .map_err(|e| format!("apply(): {e}"));
            *deadline.lock().unwrap() = None;
            applied?;
            let _ = ready_inner.send(Ok(()));

            for command in cmd_rx.iter() {
                match command {
                    Command::Call { event, payload, reply } => {
                        *deadline.lock().unwrap() =
                            Some(Instant::now() + Duration::from_millis(call_timeout_ms));
                        let event_key = serde_json::to_string(&event).unwrap_or_default();
                        let code = format!(
                            "(globalThis.__zode_handlers[{event_key}] || function () {{ return null; }})({payload})"
                        );
                        let result = (|| -> Result<String, String> {
                            let ret: JsValue = ctx
                                .eval(code.as_bytes())
                                .catch(&ctx)
                                .map_err(|e| format!("handler: {e}"))?;
                            js_stringify(&ctx, &ret)
                        })();
                        *deadline.lock().unwrap() = None;
                        let _ = reply.send(result);
                    }
                    Command::Shutdown => {
                        *deadline.lock().unwrap() =
                            Some(Instant::now() + Duration::from_millis(call_timeout_ms));
                        let _: Result<JsValue, _> = ctx
                            .eval(
                                "globalThis.__zode_cleanup ? globalThis.__zode_cleanup() : null"
                                    .as_bytes(),
                            )
                            .catch(&ctx);
                        *deadline.lock().unwrap() = None;
                        break;
                    }
                }
            }
            Ok(())
        })
    })();
    let _ = ready_tx.send(result);
}

#[async_trait]
impl Plugin for JsPlugin {
    fn name(&self) -> &'static str {
        self.name
    }

    /// The JS source IS the content: two genes wrapping different source
    /// text must never dedupe into one.
    fn content_id(&self) -> Option<String> {
        Some(self.source.clone())
    }

    async fn apply(&self, ctx: Context, config: Arc<Value>) -> PluginResult {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<WorkerMessage>(32);
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();

        let source = self.source.clone();
        let memory_limit = self.memory_limit;
        let call_timeout_ms = self.call_timeout_ms;
        let config_json =
            serde_json::to_string(config.as_ref()).unwrap_or_else(|_| "null".to_string());
        let worker_name = format!("js-gene:{}", self.name);
        let worker = std::thread::Builder::new()
            .name(worker_name)
            .spawn(move || {
                run_worker(
                    source,
                    config_json,
                    memory_limit,
                    call_timeout_ms,
                    cmd_rx,
                    msg_tx,
                    ready_tx,
                )
            })
            .map_err(|e| CordisError::PluginStartup(self.name.to_string(), e.to_string()))?;

        // Wait (off the async pool) for eval + apply() to settle.
        let ready = tokio::task::spawn_blocking(move || ready_rx.recv().ok())
            .await
            .map_err(|e| CordisError::PluginStartup(self.name.to_string(), e.to_string()))?;
        match ready {
            Some(Ok(())) => {}
            Some(Err(err)) => {
                return Err(CordisError::PluginStartup(self.name.to_string(), err));
            }
            None => {
                return Err(CordisError::PluginStartup(
                    self.name.to_string(),
                    "js worker died before apply()".to_string(),
                ));
            }
        }

        // Fiber-owned pump: guest Listen calls become global listeners; each
        // dispatch invokes the JS callback on the worker thread.
        let fiber = ctx.current_fiber().ok_or(CordisError::InactiveEffect)?;
        let pump_ctx = ctx.clone();
        let pump_cmd_tx = cmd_tx.clone();
        tokio::spawn(async move {
            let mut msg_rx = msg_rx;
            while let Some(message) = msg_rx.recv().await {
                match message {
                    WorkerMessage::Listen { event } => {
                        let pump_ctx = pump_ctx.clone();
                        let cmd_tx = pump_cmd_tx.clone();
                        let fiber = fiber.clone();
                        if let Err(err) = fiber.on_dyn_global(&event, move |host_event| {
                            let cmd_tx = cmd_tx.clone();
                            let event = host_event.name.to_string();
                            let payload = serde_json::to_string(host_event.payload.as_ref())
                                .unwrap_or_else(|_| "null".to_string());
                            let pump_ctx = pump_ctx.clone();
                            async move {
                                let (reply_tx, reply_rx) = channel();
                                let _ = cmd_tx
                                    .send(Command::Call { event, payload, reply: reply_tx });
                                let result: Option<Result<String, String>> =
                                    tokio::task::spawn_blocking(move || reply_rx.recv().ok())
                                        .await
                                        .ok()
                                        .flatten();
                                match result {
                                    Some(Ok(json)) => {
                                        match serde_json::from_str::<Value>(&json) {
                                            Ok(Value::Null) | Err(_) => Flow::Continue,
                                            Ok(value) => Flow::Bail(value),
                                        }
                                    }
                                    Some(Err(err)) => {
                                        tracing::warn!(gene = "js", error = %err, "js gene handler failed");
                                        let _ = pump_ctx.emit_dyn(
                                            "gene/timeout",
                                            &serde_json::json!({ "error": err }),
                                        );
                                        Flow::Continue
                                    }
                                    None => Flow::Continue,
                                }
                            }
                        }) {
                            tracing::warn!(gene = "js", event = %event, error = %err, "failed to register js listener");
                        }
                    }
                    WorkerMessage::Emit { event, payload } => {
                        if let Err(err) = pump_ctx.emit_dyn(&event, &payload) {
                            tracing::warn!(gene = "js", event = %event, error = %err, "js emit failed");
                        }
                    }
                }
            }
        });

        // Fiber cleanup: run the guest cleanup, then join the worker so the
        // runtime and its memory are always reclaimed.
        ctx.effect_fn(
            "js-plugin:shutdown",
            cordis_rs::Cleanup::async_boxed(Box::pin(async move {
                let _ = cmd_tx.send(Command::Shutdown);
                let _ = tokio::time::timeout(
                    Duration::from_millis(call_timeout_ms + 500),
                    tokio::task::spawn_blocking(move || {
                        let _ = worker.join();
                    }),
                )
                .await;
            })),
        )?;

        Ok(())
    }
}
