//! JS frontends: a UI whose logic is generated JavaScript, evaluated in
//! a bounded QuickJS runtime — the agent writes its own frontend and swaps
//! it in at runtime with NO compiler on the user's machine.
//!
//! Guest protocol — the source evaluates to a factory returning
//! '{ serve(host) }'. 'serve' runs once to wire the frontend; the worker
//! then stays alive as an event loop until the guest calls 'host.exit()'
//! (or 'host.swapTo(id)', which hands over to another UI first).
//!
//! Host API:
//!   log(level, message)            tracing
//!   println(text)                  render a line to stdout
//!   emit(event, payloadJson)       bus event
//!   on(event, callback)            subscribe (callback return = bail)
//!   setSkin(json)                  install a runtime skin
//!   readLine(prompt, callback)     read one stdin line, then call
//!                                  callback(line | null on EOF)
//!   swapTo(id)                     hand over to another UI and exit
//!   exit()                         end this frontend session

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use cordis_rs::{Context, CordisError, Flow};
use rquickjs::{
    CatchResultExt, Context as JsContext, Ctx, Function as JsFunction, Runtime as JsRuntime,
    Value as JsValue,
};
use serde_json::Value;

use crate::js_plugin::{JS_GENE_CALL_TIMEOUT_MS, JS_GENE_MEMORY_LIMIT};
use crate::ui::{Ui, UiDeps};

/// A frontend whose logic is generated JavaScript.
pub struct JsUi {
    pub id: &'static str,
    pub source: String,
    pub memory_limit: usize,
    pub call_timeout_ms: u64,
}

impl JsUi {
    pub fn new(id: &'static str, source: impl Into<String>) -> Self {
        JsUi {
            id,
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
    /// Invoke a guest callback registered under 'event'.
    Call {
        event: String,
        payload: String,
        reply: Sender<Result<String, String>>,
    },
    /// Tear the runtime down.
    Shutdown,
}

/// Worker → host pump messages.
enum WorkerMessage {
    Listen { event: String },
    Emit { event: String, payload: Value },
    SetSkin { json: String },
    SwapTo { id: String },
    Exit,
    ReadLine { prompt: String, handler: String },
    PrintLn { text: String },
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
    memory_limit: usize,
    call_timeout_ms: u64,
    cmd_rx: Receiver<Command>,
    msg_tx: tokio::sync::mpsc::Sender<WorkerMessage>,
    ready_tx: Sender<Result<(), String>>,
) {
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
            let globals = ctx.globals();
            globals
                .set(
                    "__zode_log",
                    rquickjs::function::Func::from(|level: String, message: String| {
                        match level.as_str() {
                            "error" => {
                                tracing::error!(target: "cordis", ui = "js", "{}", message)
                            }
                            "warn" => {
                                tracing::warn!(target: "cordis", ui = "js", "{}", message)
                            }
                            _ => tracing::info!(target: "cordis", ui = "js", "{}", message),
                        }
                    }),
                )
                .map_err(|e| format!("__zode_log: {e}"))?;
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_println",
                        rquickjs::function::Func::from(move |text: String| {
                            let _ = msg_tx.blocking_send(WorkerMessage::PrintLn { text });
                        }),
                    )
                    .map_err(|e| format!("__zode_println: {e}"))?;
            }
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
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_set_skin",
                        rquickjs::function::Func::from(move |json: String| {
                            let _ = msg_tx.blocking_send(WorkerMessage::SetSkin { json });
                        }),
                    )
                    .map_err(|e| format!("__zode_set_skin: {e}"))?;
            }
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_read_line",
                        rquickjs::function::Func::from(
                            move |prompt: String, handler: String| {
                                let _ = msg_tx
                                    .blocking_send(WorkerMessage::ReadLine { prompt, handler });
                            },
                        ),
                    )
                    .map_err(|e| format!("__zode_read_line: {e}"))?;
            }
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_swap_to",
                        rquickjs::function::Func::from(move |id: String| {
                            let _ = msg_tx.blocking_send(WorkerMessage::SwapTo { id });
                        }),
                    )
                    .map_err(|e| format!("__zode_swap_to: {e}"))?;
            }
            {
                let msg_tx = msg_tx.clone();
                globals
                    .set(
                        "__zode_exit",
                        rquickjs::function::Func::from(move || {
                            let _ = msg_tx.blocking_send(WorkerMessage::Exit);
                        }),
                    )
                    .map_err(|e| format!("__zode_exit: {e}"))?;
            }
            ctx.eval::<(), _>(
                "globalThis.__zode_handlers = {};".as_bytes(),
            )
            .catch(&ctx)
            .map_err(|e| format!("handler registry: {e}"))?;
            ctx.eval::<(), _>(
                r#"globalThis.host = {
  log: __zode_log,
  println: __zode_println,
  emit: __zode_emit,
  on: function (event, cb) { globalThis.__zode_handlers[event] = cb; __zode_register(event); },
  setSkin: __zode_set_skin,
  readLine: function (prompt, cb) { var name = "__line_" + Math.random().toString(36).slice(2); globalThis.__zode_handlers[name] = cb; __zode_read_line(prompt, name); },
  swapTo: __zode_swap_to,
  exit: __zode_exit,
};"#
                    .as_bytes(),
            )
            .catch(&ctx)
            .map_err(|e| format!("host facade: {e}"))?;

            let factory: JsFunction = ctx
                .eval(source.as_bytes())
                .catch(&ctx)
                .map_err(|e| format!("eval: {e}"))?;
            let ui_obj_value: JsValue = factory
                .call(())
                .catch(&ctx)
                .map_err(|e| format!("factory: {e}"))?;
            let ui_obj = ui_obj_value
                .as_object()
                .ok_or_else(|| "ui factory must return an object".to_string())?
                .clone();
            let serve: JsFunction = ui_obj
                .get("serve")
                .catch(&ctx)
                .map_err(|e| format!("missing serve(): {e}"))?;
            let host = globals
                .get::<_, rquickjs::Object>("host")
                .map_err(|e| format!("host: {e}"))?;

            *deadline.lock().unwrap() =
                Some(Instant::now() + Duration::from_millis(call_timeout_ms));
            let served: Result<JsValue, String> = serve
                .call((host,))
                .catch(&ctx)
                .map_err(|e| format!("serve(): {e}"));
            *deadline.lock().unwrap() = None;
            served?;
            let _ = ready_inner.send(Ok(()));

            for command in cmd_rx.iter() {
                match command {
                    Command::Call { event, payload, reply } => {
                        *deadline.lock().unwrap() =
                            Some(Instant::now() + Duration::from_millis(call_timeout_ms));
                        let result = (|| -> Result<String, String> {
                            let event_key = serde_json::to_string(&event).unwrap_or_default();
                            let code = format!(
                                "(globalThis.__zode_handlers[{event_key}] || function () {{ return null; }})({payload})"
                            );
                            let ret: JsValue = ctx
                                .eval(code.as_bytes())
                                .catch(&ctx)
                                .map_err(|e| format!("handler: {e}"))?;
                            js_stringify(&ctx, &ret)
                        })();
                        *deadline.lock().unwrap() = None;
                        let _ = reply.send(result);
                    }
                    Command::Shutdown => break,
                }
            }
            Ok(())
        })
    })();
    let _ = ready_tx.send(result);
}

#[async_trait]
impl Ui for JsUi {
    fn id(&self) -> &'static str {
        self.id
    }

    async fn serve(&self, ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
        let (cmd_tx, cmd_rx) = channel::<Command>();
        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<WorkerMessage>(32);
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();

        let source = self.source.clone();
        let memory_limit = self.memory_limit;
        let call_timeout_ms = self.call_timeout_ms;
        let worker_name = format!("js-ui:{}", self.id);
        let worker = std::thread::Builder::new()
            .name(worker_name)
            .spawn(move || {
                run_worker(
                    source,
                    memory_limit,
                    call_timeout_ms,
                    cmd_rx,
                    msg_tx,
                    ready_tx,
                )
            })
            .map_err(|e| CordisError::PluginStartup(self.id.to_string(), e.to_string()))?;

        let ready = tokio::task::spawn_blocking(move || ready_rx.recv().ok())
            .await
            .map_err(|e| CordisError::PluginStartup(self.id.to_string(), e.to_string()))?;
        match ready {
            Some(Ok(())) => {}
            Some(Err(err)) => {
                return Err(CordisError::PluginStartup(self.id.to_string(), err));
            }
            None => {
                return Err(CordisError::PluginStartup(
                    self.id.to_string(),
                    "js ui worker died before serve()".to_string(),
                ));
            }
        }

        // Pump: bridge worker messages to the harness until the guest exits.
        let fiber = ctx.current_fiber().ok_or(CordisError::InactiveEffect)?;
        let pump_ctx = ctx.clone();
        let pump_cmd_tx = cmd_tx.clone();
        let (exit_tx, exit_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let mut msg_rx = msg_rx;
            while let Some(message) = msg_rx.recv().await {
                match message {
                    WorkerMessage::Listen { event } => {
                        let cmd_tx = pump_cmd_tx.clone();
                        let fiber = fiber.clone();
                        if let Err(err) = fiber.on_dyn_global(&event, move |host_event| {
                            let cmd_tx = cmd_tx.clone();
                            let event = host_event.name.to_string();
                            let payload = serde_json::to_string(host_event.payload.as_ref())
                                .unwrap_or_else(|_| "null".to_string());
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
                                        tracing::warn!(ui = "js", error = %err, "js ui handler failed");
                                        Flow::Continue
                                    }
                                    None => Flow::Continue,
                                }
                            }
                        }) {
                            tracing::warn!(ui = "js", event = %event, error = %err, "failed to register js ui listener");
                        }
                    }
                    WorkerMessage::Emit { event, payload } => {
                        if let Err(err) = pump_ctx.emit_dyn(&event, &payload) {
                            tracing::warn!(ui = "js", event = %event, error = %err, "js ui emit failed");
                        }
                    }
                    WorkerMessage::SetSkin { json } => {
                        if let Ok(state) =
                            pump_ctx.use_service::<Arc<crate::skin::SkinState>>("ui/skin")
                        {
                            if let Err(err) = state.install(&json) {
                                tracing::warn!(ui = "js", error = %err, "skin install failed");
                            }
                        }
                    }
                    WorkerMessage::PrintLn { text } => {
                        use std::io::Write;
                        let mut stdout = std::io::stdout().lock();
                        let _ = writeln!(stdout, "{text}");
                        let _ = stdout.flush();
                    }
                    WorkerMessage::ReadLine { prompt, handler } => {
                        use std::io::Write;
                        {
                            let mut stdout = std::io::stdout().lock();
                            let _ = write!(stdout, "{prompt}");
                            let _ = stdout.flush();
                        }
                        let cmd_tx = pump_cmd_tx.clone();
                        tokio::spawn(async move {
                            let line = tokio::task::spawn_blocking(|| {
                                let mut line = String::new();
                                match std::io::stdin().read_line(&mut line) {
                                    Ok(0) | Err(_) => None,
                                    Ok(_) => Some(line.trim_end().to_string()),
                                }
                            })
                            .await
                            .unwrap_or(None);
                            let payload =
                                serde_json::to_string(&line).unwrap_or_else(|_| "null".into());
                            let (reply_tx, reply_rx) = channel();
                            let _ = cmd_tx.send(Command::Call {
                                event: handler,
                                payload,
                                reply: reply_tx,
                            });
                            // Discard the callback's return value (the guest
                            // drives control flow itself).
                            let _ = tokio::task::spawn_blocking(move || reply_rx.recv().ok()).await;
                        });
                    }
                    WorkerMessage::SwapTo { id } => {
                        // Hand over to the next frontend, then end this one.
                        let _ = pump_ctx
                            .parallel_dyn("ui/swap", &serde_json::json!({ "to": id }))
                            .await;
                        let _ = exit_tx.send(());
                        break;
                    }
                    WorkerMessage::Exit => {
                        let _ = exit_tx.send(());
                        break;
                    }
                }
            }
        });

        // Wait for the guest to end the frontend session.
        let _ = exit_rx.await;

        // Teardown: stop the worker and reclaim the runtime.
        let _ = cmd_tx.send(Command::Shutdown);
        let _ = tokio::time::timeout(
            Duration::from_millis(call_timeout_ms + 500),
            tokio::task::spawn_blocking(move || {
                let _ = worker.join();
            }),
        )
        .await;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use cordis_rs::prelude::*;
    use serde_json::json;

    use crate::config::ZodeConfig;
    use crate::ui::{UiDeps, UiHost};

    use super::*;

    /// A rust probe UI that logs its own run and hands over via ui/swap.
    struct ProbeUi(
        &'static str,
        Arc<Mutex<Vec<&'static str>>>,
        Option<&'static str>,
    );

    #[async_trait]
    impl Ui for ProbeUi {
        fn id(&self) -> &'static str {
            self.0
        }

        async fn serve(&self, ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
            self.1.lock().unwrap().push(self.0);
            if let Some(next) = self.2 {
                let _ = ctx.parallel_dyn("ui/swap", &json!({ "to": next })).await;
            }
            Ok(())
        }
    }

    #[tokio::test]
    async fn js_ui_prints_emits_and_hands_over() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(
            &root,
            Arc::new(UiDeps {
                cwd: std::path::PathBuf::from("/tmp"),
                cfg: ZodeConfig::default(),
            }),
        )?;
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        let probes: Arc<AtomicUsize> = Arc::new(AtomicUsize::new(0));
        root.on_dyn("js-ui/hello", {
            let probes = probes.clone();
            move |_| {
                probes.fetch_add(1, Ordering::SeqCst);
                async { Flow::Continue }
            }
        })?;

        host.register(Arc::new(ProbeUi(
            "rust-headless",
            log.clone(),
            Some("js-v1"),
        )));
        host.register(Arc::new(JsUi::new(
            "js-v1",
            r#"(function () {
  return {
    serve: function (host) {
      host.println("js-v1 mounted");
      host.emit("js-ui/hello", JSON.stringify({ from: "js-v1" }));
      host.swapTo("rust-done");
    },
  };
})"#,
        )));
        host.register(Arc::new(ProbeUi("rust-done", log.clone(), None)));

        host.run("rust-headless").await?;
        assert_eq!(
            *log.lock().unwrap(),
            vec!["rust-headless", "rust-done"],
            "rust -> js-v1 (prints/emits) -> rust-done handover"
        );
        assert_eq!(probes.load(Ordering::SeqCst), 1, "js ui must emit");
        Ok(())
    }

    #[tokio::test]
    async fn js_ui_installs_a_skin_at_runtime() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(
            &root,
            Arc::new(UiDeps {
                cwd: std::path::PathBuf::from("/tmp"),
                cfg: ZodeConfig::default(),
            }),
        )?;
        let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
        assert!(
            root.has_service("ui/skin"),
            "skin service missing after UiHost::new"
        );
        host.register(Arc::new(ProbeUi(
            "rust-headless",
            log.clone(),
            Some("js-skin"),
        )));
        host.register(Arc::new(JsUi::new(
            "js-skin",
            r#"(function () {
  return {
    serve: function (host) {
      host.setSkin(JSON.stringify({
        name: "js-skin",
        colors: { accent: "141", bg_primary: "234" },
      }));
      host.swapTo("rust-done");
    },
  };
})"#,
        )));
        host.register(Arc::new(ProbeUi("rust-done", log.clone(), None)));
        host.run("rust-headless").await?;

        let skin = root
            .use_service::<Arc<crate::skin::SkinState>>("ui/skin")
            .unwrap();
        assert!(skin.version() >= 1, "js ui must install its skin");
        assert!(skin.current().unwrap().contains("js-skin"));
        Ok(())
    }

    #[tokio::test]
    async fn js_ui_serves_until_exit() -> Result<(), CordisError> {
        let root = Context::root();
        let host = UiHost::new(
            &root,
            Arc::new(UiDeps {
                cwd: std::path::PathBuf::from("/tmp"),
                cfg: ZodeConfig::default(),
            }),
        )?;
        let exited = Arc::new(AtomicUsize::new(0));
        root.on_dyn("js-ui/exited", {
            let exited = exited.clone();
            move |_| {
                exited.fetch_add(1, Ordering::SeqCst);
                async { Flow::Continue }
            }
        })?;
        host.register(Arc::new(JsUi::new(
            "js-linger",
            r#"(function () {
  return {
    serve: function (host) {
      host.on("tick", function () {
        host.emit("js-ui/exited", JSON.stringify({}));
        host.exit();
        return null;
      });
    },
  };
})"#,
        )));

        // serve() returns after wiring; the UI stays mounted until a tick
        // event drives it to exit().
        let fiber = host.mount("js-linger").await?;
        assert!(host.active_id().is_some());
        for _ in 0..100 {
            if exited.load(Ordering::SeqCst) >= 1 {
                break;
            }
            let _ = root.parallel_dyn("tick", &json!({})).await;
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(exited.load(Ordering::SeqCst), 1);
        // The fiber settles when the guest exited.
        fiber.await_ready().await?;
        assert!(host.active_id().is_some());
        host.unmount().await;
        Ok(())
    }
}
