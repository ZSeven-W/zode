//! Process plugins: capability units that run as separate executables.
//!
//! A compiled Rust (or any-language) program becomes a plugin simply by
//! speaking a small JSON-lines protocol over stdin/stdout. **Replacing a
//! compiled plugin = dispose the fiber (which kills the child process) and
//! spawn a new fiber pointing at the new binary — the harness process
//! itself never restarts.** This is the AOT answer to hot-swapping: the
//! lifecycle machinery (spawn/dispose/restart/selection) stays in the
//! harness, and the code medium becomes just another transport.
//!
//! Protocol (one JSON object per line, UTF-8):
//!
//! - host → child: \`{"event": name, "payload": value}\` for every
//!   dispatch of an event the child subscribed to;
//! - child → host:
//!   - \`{"op":"listen","event":name}\` — subscribe to an event;
//!   - \`{"op":"emit","event":name,"payload":value}\` — dispatch on the host bus;
//!   - \`{"op":"log","level":"info|warn|error","message":"..."}\` — log via tracing.
//!
//! When the fiber disposes, the child's stdin is closed and it is killed
//! (graceful term, then kill after \`shutdown_grace_secs\`) — a replaced
//! plugin can never leak a live process.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::future::BoxFuture;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{ChildStdin, Command};
use tokio::sync::Mutex;

use crate::context::Context;
use crate::error::CordisError;
use crate::events::Flow;
use crate::plugin::{Plugin, PluginResult};
use crate::types::Cleanup;

/// A plugin whose body is a separate executable.
pub struct ProcessPlugin {
    pub name: &'static str,
    pub program: PathBuf,
    pub args: Vec<String>,
    /// Seconds of grace between SIGTERM and SIGKILL when the fiber disposes.
    pub shutdown_grace_secs: u64,
}

impl ProcessPlugin {
    pub fn new(name: &'static str, program: impl Into<PathBuf>) -> Self {
        ProcessPlugin {
            name,
            program: program.into(),
            args: Vec::new(),
            shutdown_grace_secs: 3,
        }
    }

    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    pub fn with_shutdown_grace(mut self, secs: u64) -> Self {
        self.shutdown_grace_secs = secs;
        self
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum ChildMessage {
    Listen { event: String },
    Emit { event: String, payload: Value },
    Log { level: String, message: String },
}

type ChildSender = Arc<Mutex<Option<BufWriter<ChildStdin>>>>;

async fn send_line(sender: &ChildSender, line: String) {
    let mut guard = sender.lock().await;
    let Some(writer) = guard.as_mut() else {
        return;
    };
    if writer.write_all(line.as_bytes()).await.is_err() {
        *guard = None;
        return;
    }
    let _ = writer.write_all(b"\n").await;
    let _ = writer.flush().await;
}

fn spawn_missing() -> CordisError {
    CordisError::PluginStartup(
        "process-plugin".to_string(),
        "child stdin/stdout pipes are unavailable".to_string(),
    )
}

#[async_trait]
impl Plugin for ProcessPlugin {
    fn name(&self) -> &'static str {
        self.name
    }

    async fn apply(&self, ctx: Context, _config: Arc<Value>) -> PluginResult {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(false)
            .spawn()
            .map_err(|e| CordisError::PluginStartup(self.name.to_string(), e.to_string()))?;
        let child_stdin = child.stdin.take().ok_or_else(spawn_missing)?;
        let child_stdout = child.stdout.take().ok_or_else(spawn_missing)?;

        let sender: ChildSender = Arc::new(Mutex::new(Some(BufWriter::new(child_stdin))));

        // The plugin's own fiber owns every listener the child subscribes to,
        // even though registration happens from this spawned helper task.
        let fiber = ctx.current_fiber().ok_or(CordisError::InactiveEffect)?;

        // Reader task: child → host messages.
        let plugin_name = self.name;
        let ctx_reader = ctx.clone();
        let fiber_reader = fiber.clone();
        let sender_reader = sender.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(child_stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                match serde_json::from_str::<ChildMessage>(&line) {
                    Ok(ChildMessage::Listen { event }) => {
                        let sender = sender_reader.clone();
                        let event_name = event.clone();
                        if let Err(err) = fiber_reader.on_dyn_global(&event, move |host_event| {
                            let sender = sender.clone();
                            let line = serde_json::to_string(&json!({
                                "event": host_event.name.as_ref(),
                                "payload": host_event.payload.as_ref(),
                            }))
                            .unwrap_or_default();
                            async move {
                                send_line(&sender, line).await;
                                Flow::Continue
                            }
                        }) {
                            tracing::warn!(
                                plugin = plugin_name,
                                event = %event_name,
                                error = %err,
                                "failed to register child listener",
                            );
                        }
                    }
                    Ok(ChildMessage::Emit { event, payload }) => {
                        if let Err(err) = ctx_reader.emit_dyn(&event, &payload) {
                            tracing::warn!(plugin = plugin_name, event = %event, error = %err, "child emit failed");
                        }
                    }
                    Ok(ChildMessage::Log { level, message }) => match level.as_str() {
                        "error" => {
                            tracing::error!(target: "cordis", plugin = plugin_name, "{}", message)
                        }
                        "warn" => {
                            tracing::warn!(target: "cordis", plugin = plugin_name, "{}", message)
                        }
                        _ => tracing::info!(target: "cordis", plugin = plugin_name, "{}", message),
                    },
                    Err(err) => {
                        tracing::warn!(plugin = plugin_name, line = %line, error = %err, "bad child message");
                    }
                }
            }
        });

        // Fiber cleanup: close stdin, graceful term, then kill — so a
        // replaced plugin can never leave its process behind.
        let child = Arc::new(Mutex::new(child));
        let grace = self.shutdown_grace_secs;
        ctx.effect_fn(
            "process-plugin:kill",
            Cleanup::async_boxed(Box::pin({
                let child = child.clone();
                let sender = sender.clone();
                async move {
                    // Close stdin first: well-behaved children exit on EOF.
                    *sender.lock().await = None;
                    let mut child = child.lock().await;
                    if child.try_wait().ok().flatten().is_some() {
                        return;
                    }
                    let _ = child.start_kill();
                    let deadline = tokio::time::sleep(Duration::from_secs(grace));
                    tokio::pin!(deadline);
                    tokio::select! {
                        _ = &mut deadline => {
                            let _ = child.kill().await;
                        }
                        result = child.wait() => {
                            let _ = result;
                        }
                    }
                }
            }) as BoxFuture<'static, ()>),
        )?;

        Ok(())
    }
}
