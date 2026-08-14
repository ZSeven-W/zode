//! The real zode frontends as harness UI plugins — the DSH-style UI
//! layer. Each frontend runs inside its own fiber: mounting/disposing
//! fibers starts/tears the frontend down, and a frontend can hand over to
//! another at runtime via the 'ui/swap' event.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use zode_core::cordis_rs::{Context, CordisError};
use zode_core::sessions::DurableSessionMeta;
use zode_core::ui::{Ui, UiDeps};
use zode_core::ZodeEngine;

use crate::args::OutputFormat;

/// Process exit code shared with main().
pub type ExitCell = Arc<Mutex<i32>>;

/// The '-p/--print' single-turn frontend.
pub struct HeadlessUi {
    pub engine: Arc<ZodeEngine>,
    pub prompt: String,
    pub meta: Mutex<Option<DurableSessionMeta>>,
    pub output_format: OutputFormat,
    pub exit: ExitCell,
}

#[async_trait]
impl Ui for HeadlessUi {
    fn id(&self) -> &'static str {
        "headless"
    }

    async fn serve(&self, _ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
        let meta = self.meta.lock().unwrap().take().expect("meta present");
        let code =
            crate::headless::run_print(&self.engine, &self.prompt, meta, self.output_format).await;
        *self.exit.lock().unwrap() = code;
        Ok(())
    }
}

/// The '--no-tui' readline REPL frontend.
pub struct ReadlineUi {
    pub engine: Mutex<Option<ZodeEngine>>,
    pub resumed_id: Option<String>,
    pub exit: ExitCell,
}

#[async_trait]
impl Ui for ReadlineUi {
    fn id(&self) -> &'static str {
        "readline"
    }

    async fn serve(&self, _ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
        let engine = self.engine.lock().unwrap().take().expect("engine present");
        let code = crate::repl::run_repl(engine, self.resumed_id.clone()).await;
        *self.exit.lock().unwrap() = code;
        Ok(())
    }
}

/// Build inputs for the ratatui frontend. 'TuiApp' itself is not 'Sync'
/// (it owns terminal buffers), so it is constructed inside 'serve' — the
/// parts are plain 'Send' state that the plugin can hold.
pub struct TuiParts {
    pub engine: ZodeEngine,
    pub template: zode_core::EngineTemplate,
    pub ui: zode_tui::UiConfig,
    pub approval_rx: zode_core::approval::ApprovalReceiver,
    pub question_rx: zode_core::question::QuestionReceiver,
    pub op_question_queue: zode_core::question::QuestionQueue,
    pub resumed_id: Option<String>,
}

/// The full ratatui frontend (including the browser-native-host daemon
/// mode).
pub struct TuiUi {
    pub parts: Mutex<Option<TuiParts>>,
    pub browser_native_host: bool,
    pub exit: ExitCell,
}

#[async_trait]
impl Ui for TuiUi {
    fn id(&self) -> &'static str {
        "tui"
    }

    async fn serve(&self, _ctx: Context, _deps: Arc<UiDeps>) -> Result<(), CordisError> {
        let parts = self.parts.lock().unwrap().take().expect("parts present");
        let app = zode_tui::TuiApp::new(
            parts.engine,
            parts.template,
            parts.ui,
            parts.approval_rx,
            parts.question_rx,
            parts.op_question_queue,
            parts.resumed_id,
        );
        let browser_native_host = self.browser_native_host;

        // 'TuiApp' owns non-Sync terminal state (RefCell-backed views), so
        // its borrowed futures are not Send. Run the whole app on a
        // dedicated thread with its own single-thread runtime; this future
        // only waits for the exit code, so the plugin stays Send-safe.
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<i32>();
        std::thread::Builder::new()
            .name("zode-tui".to_string())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tui runtime");
                let code = runtime.block_on(async move {
                    if browser_native_host {
                        match app.ensure_extension_bridge_listening().await {
                            Ok(port) => {
                                if let Err(error) = crate::browser_native_host::write_ready(port) {
                                    eprintln!("zode native host: {error}");
                                    1
                                } else {
                                    match app
                                        .run_extension_daemon(
                                            crate::browser_native_host::spawn_disconnect_watcher(),
                                        )
                                        .await
                                    {
                                        Ok(()) => 0,
                                        Err(error) => {
                                            eprintln!("zode: {error}");
                                            1
                                        }
                                    }
                                }
                            }
                            Err(error) => {
                                let _ = crate::browser_native_host::write_error(&error.to_string());
                                1
                            }
                        }
                    } else {
                        match app.run().await {
                            Ok(()) => 0,
                            Err(error) => {
                                eprintln!("zode: {error}");
                                1
                            }
                        }
                    }
                });
                let _ = done_tx.send(code);
            })
            .map_err(|error| CordisError::PluginStartup("tui".to_string(), error.to_string()))?;
        let code = done_rx.await.unwrap_or(1);
        *self.exit.lock().unwrap() = code;
        Ok(())
    }
}
