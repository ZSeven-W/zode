//! Turn execution adapters and the initialize-time host factory seam.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::approval_broker::BrokerMsg;
use agent::abort::AbortController;
use agent::stream::Event;
use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::sync::{mpsc, Mutex};
use zode_app_server_protocol::rpc::{ErrorObject, INVALID_PARAMS, TURN_ACTIVE};
use zode_app_server_protocol::types::{ApprovalPolicy, Thread};
use zode_core::approval::{approval_queue, Approval};
use zode_core::config::ZodeConfig;
use zode_core::engine::{EngineTemplate, ZodeEngine};
use zode_core::sandbox::SandboxConfig;

use crate::accumulator::{TurnAccumulator, TurnEndState};
use crate::error::error;
use crate::session::SessionMsg;

pub(crate) async fn drive_stream(
    mut stream: Box<dyn agent::stream::EventStream>,
    accumulator: &mut TurnAccumulator,
    msgs: &mpsc::Sender<SessionMsg>,
) -> TurnEndState {
    let mut last_error = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(event) => {
                for notification in accumulator.on_event(&event) {
                    let _ = msgs.send(SessionMsg::TurnEvent { notification }).await;
                }
            }
            Err(error) => {
                let message = error.to_string();
                for notification in accumulator.on_event(&Event::Error {
                    code: "engine_error".into(),
                    message: message.clone(),
                }) {
                    let _ = msgs.send(SessionMsg::TurnEvent { notification }).await;
                }
                last_error = Some(message);
            }
        }
    }
    last_error.map_or(TurnEndState::Completed, |error| TurnEndState::Failed {
        error,
    })
}

#[async_trait]
pub trait TurnHost: Send + 'static {
    /// Replaces the base config used only when assembling future engines.
    async fn apply_config(&mut self, _cfg: ZodeConfig) {}

    async fn set_model(&mut self, thread_id: &str, model: &str) -> Result<(), ErrorObject>;

    /// Restores a thread's persistent model after a turn-level override.
    async fn restore_model(&mut self, thread_id: &str);

    /// Starts a turn and arranges for exactly one `TurnFinished` message eventually.
    async fn start_turn(
        &mut self,
        thread: &Thread,
        input: String,
        model_override: Option<String>,
        abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    );
}

pub trait HostFactory: Send + 'static {
    fn base_config(&self) -> ZodeConfig {
        ZodeConfig::default()
    }

    fn build_host(
        &mut self,
        policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
        broker: Option<mpsc::Sender<BrokerMsg>>,
    ) -> Box<dyn TurnHost>;
}

pub struct EngineHost {
    template: EngineTemplate,
    engines: Arc<Mutex<HashMap<String, Arc<ZodeEngine>>>>,
    thread_templates: Arc<Mutex<HashMap<String, EngineTemplate>>>,
    pending_models: Arc<Mutex<HashMap<String, String>>>,
    restore_models: Arc<Mutex<HashMap<String, String>>>,
    turn_ids: mpsc::UnboundedReceiver<String>,
    approval_pump: Option<tokio::task::JoinHandle<()>>,
}

impl EngineHost {
    pub fn new(
        cfg: ZodeConfig,
        cwd: PathBuf,
        sandbox: Option<SandboxConfig>,
        date: String,
        policy: ApprovalPolicy,
        turn_ids: mpsc::UnboundedReceiver<String>,
        broker: Option<mpsc::Sender<BrokerMsg>>,
    ) -> Self {
        let (queue, approval_pump) = match policy {
            ApprovalPolicy::Auto => (None, None),
            ApprovalPolicy::ReadOnly => {
                let (queue, mut receiver) = approval_queue();
                let broker = tokio::spawn(async move {
                    while let Some(request) = receiver.next().await {
                        let _ = request.respond(Approval::Deny);
                    }
                });
                (Some(queue), Some(broker))
            }
            ApprovalPolicy::Prompt => {
                let (queue, mut receiver) = approval_queue();
                let broker = broker.expect("prompt host requires approval broker");
                let pump = tokio::spawn(async move {
                    while let Some(request) = receiver.next().await {
                        if broker.send(BrokerMsg::Engine(request)).await.is_err() {
                            break;
                        }
                    }
                });
                (Some(queue), Some(pump))
            }
        };
        let yolo = policy == ApprovalPolicy::Auto;
        Self {
            template: EngineTemplate::new(cfg, cwd, queue, yolo, sandbox, date),
            engines: Arc::new(Mutex::new(HashMap::new())),
            thread_templates: Arc::new(Mutex::new(HashMap::new())),
            pending_models: Arc::new(Mutex::new(HashMap::new())),
            restore_models: Arc::new(Mutex::new(HashMap::new())),
            turn_ids,
            approval_pump,
        }
    }

    #[cfg(test)]
    pub(crate) async fn pending_model(&self, thread_id: &str) -> Option<String> {
        self.pending_models.lock().await.get(thread_id).cloned()
    }

    #[cfg(test)]
    pub(crate) async fn restore_model_pending(&self, thread_id: &str) -> Option<String> {
        self.restore_models.lock().await.get(thread_id).cloned()
    }

    #[cfg(test)]
    pub(crate) async fn engine_arc(&self, thread_id: &str) -> Option<Arc<ZodeEngine>> {
        self.engines.lock().await.get(thread_id).cloned()
    }

    /// Test-only: assembles a real engine (no network I/O) and stores it for
    /// `thread_id`, without going through `start_turn`/`engine.turn()` — lets
    /// tests exercise `restore_model`'s busy/retry handling deterministically,
    /// without a real (and possibly retry-storming) provider round trip.
    #[cfg(test)]
    pub(crate) async fn assemble_engine_for_test(&self, thread_id: &str, cwd: &std::path::Path) {
        let engine = self
            .template
            .assemble_tab(Some(cwd.to_path_buf()), Some(thread_id.to_string()))
            .await
            .expect("test engine assembly should succeed");
        self.engines
            .lock()
            .await
            .insert(thread_id.to_string(), Arc::new(engine));
    }

    /// Test-only: directly seeds a `restore_models` entry, as if a prior
    /// turn-level model override had already recorded it.
    #[cfg(test)]
    pub(crate) async fn set_restore_pending_for_test(&self, thread_id: &str, model: &str) {
        self.restore_models
            .lock()
            .await
            .insert(thread_id.to_string(), model.to_string());
    }
}

impl Drop for EngineHost {
    fn drop(&mut self) {
        if let Some(broker) = self.approval_pump.take() {
            broker.abort();
        }
    }
}

#[async_trait]
impl TurnHost for EngineHost {
    async fn apply_config(&mut self, cfg: ZodeConfig) {
        self.template = self.template.with_config(cfg);
    }

    async fn set_model(&mut self, thread_id: &str, model: &str) -> Result<(), ErrorObject> {
        let template = self
            .thread_templates
            .lock()
            .await
            .get(thread_id)
            .cloned()
            .unwrap_or_else(|| self.template.clone());
        if !template.model_ids().iter().any(|id| id == model) {
            return Err(error(INVALID_PARAMS, format!("unknown model: {model}")));
        }

        let mut engines = self.engines.lock().await;
        let Some(engine) = engines.get_mut(thread_id) else {
            drop(engines);
            self.pending_models
                .lock()
                .await
                .insert(thread_id.to_string(), model.to_string());
            return Ok(());
        };
        let engine = Arc::get_mut(engine).ok_or_else(|| error(TURN_ACTIVE, "engine busy"))?;
        let updated = template
            .hot_swap_model(engine, model.to_string())
            .map_err(|e| error(INVALID_PARAMS, format!("model switch failed: {e}")))?;
        drop(engines);
        self.thread_templates
            .lock()
            .await
            .insert(thread_id.to_string(), updated);
        self.restore_models.lock().await.remove(thread_id);
        Ok(())
    }

    async fn restore_model(&mut self, thread_id: &str) {
        // Only clone the pending model out; do NOT remove the `restore_models`
        // entry yet. If the engine is busy (an Arc clone is alive elsewhere,
        // e.g. a post-turn extraction task) or the hot swap itself errors, the
        // entry must survive so a later retry (see `start_turn`'s self-heal,
        // or another `restore_model` call) can still apply it. Removing it
        // unconditionally here would leak the turn-level override model
        // forever, with no data left to recover from it short of a manual
        // `model/set`.
        let Some(model) = self.restore_models.lock().await.get(thread_id).cloned() else {
            return;
        };
        let template = self
            .thread_templates
            .lock()
            .await
            .get(thread_id)
            .cloned()
            .unwrap_or_else(|| self.template.clone());
        let mut engines = self.engines.lock().await;
        let Some(engine) = engines.get_mut(thread_id) else {
            return;
        };
        let Some(engine) = Arc::get_mut(engine) else {
            tracing::debug!(thread_id, "could not restore model because engine is busy");
            return;
        };
        match template.hot_swap_model(engine, model) {
            Ok(restored) => {
                drop(engines);
                self.thread_templates
                    .lock()
                    .await
                    .insert(thread_id.to_string(), restored);
                self.restore_models.lock().await.remove(thread_id);
            }
            Err(error) => {
                tracing::debug!(thread_id, %error, "could not restore thread model");
            }
        }
    }

    async fn start_turn(
        &mut self,
        thread: &Thread,
        input: String,
        model_override: Option<String>,
        abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    ) {
        let Some(turn_id) = self.turn_ids.recv().await else {
            return;
        };
        let thread_id = thread.id.clone();
        let cwd = PathBuf::from(&thread.cwd);
        let template = self.template.clone();
        let engines = self.engines.clone();
        let thread_templates = self.thread_templates.clone();
        let pending_models = self.pending_models.clone();
        let restore_models = self.restore_models.clone();
        tokio::spawn(async move {
            let mut effective_template = thread_templates
                .lock()
                .await
                .get(&thread_id)
                .cloned()
                .unwrap_or(template);
            let engine = {
                let mut stored = engines.lock().await;
                if !stored.contains_key(&thread_id) {
                    if let Some(model) = pending_models.lock().await.remove(&thread_id) {
                        effective_template = effective_template.with_model(model);
                        thread_templates
                            .lock()
                            .await
                            .insert(thread_id.clone(), effective_template.clone());
                    }
                    match effective_template
                        .assemble_tab(Some(cwd), Some(thread_id.clone()))
                        .await
                    {
                        Ok(engine) => {
                            let engine = Arc::new(engine);
                            stored.insert(thread_id.clone(), engine);
                        }
                        Err(error) => {
                            let outcome = TurnAccumulator::new(&thread_id, &turn_id).finish(
                                TurnEndState::Failed {
                                    error: error.to_string(),
                                },
                            );
                            let _ = msgs
                                .send(SessionMsg::TurnFinished {
                                    thread_id,
                                    turn_id,
                                    outcome,
                                })
                                .await;
                            return;
                        }
                    }
                } else if let Some(pending) = restore_models.lock().await.get(&thread_id).cloned() {
                    // Self-heal: a previous turn's restore may have found the
                    // engine busy (see `restore_model`) and left the entry in
                    // place. Opportunistically apply it now, before this turn
                    // reuses (or overrides) the engine, so the override model
                    // doesn't linger indefinitely on a thread that never gets
                    // another `restore_model` call.
                    if let Some(engine) = stored.get_mut(&thread_id).and_then(Arc::get_mut) {
                        match effective_template.hot_swap_model(engine, pending) {
                            Ok(restored) => {
                                effective_template = restored;
                                thread_templates
                                    .lock()
                                    .await
                                    .insert(thread_id.clone(), effective_template.clone());
                                restore_models.lock().await.remove(&thread_id);
                            }
                            Err(error) => {
                                tracing::debug!(
                                    thread_id = %thread_id,
                                    %error,
                                    "could not self-heal pending model restore"
                                );
                            }
                        }
                    }
                }
                if let Some(override_model) = model_override {
                    if !effective_template
                        .model_ids()
                        .iter()
                        .any(|id| id == &override_model)
                    {
                        let outcome = TurnAccumulator::new(&thread_id, &turn_id).finish(
                            TurnEndState::Failed {
                                error: format!("unknown model override: {override_model}"),
                            },
                        );
                        let _ = msgs
                            .send(SessionMsg::TurnFinished {
                                thread_id,
                                turn_id,
                                outcome,
                            })
                            .await;
                        return;
                    }
                    if effective_template.model() != Some(override_model.as_str()) {
                        let Some(engine) = stored.get_mut(&thread_id).and_then(Arc::get_mut) else {
                            let outcome = TurnAccumulator::new(&thread_id, &turn_id).finish(
                                TurnEndState::Failed {
                                    error: "engine busy while applying model override".into(),
                                },
                            );
                            let _ = msgs
                                .send(SessionMsg::TurnFinished {
                                    thread_id,
                                    turn_id,
                                    outcome,
                                })
                                .await;
                            return;
                        };
                        let previous = effective_template.model().unwrap_or_default().to_string();
                        if let Err(error) =
                            effective_template.hot_swap_model(engine, override_model)
                        {
                            let outcome = TurnAccumulator::new(&thread_id, &turn_id).finish(
                                TurnEndState::Failed {
                                    error: format!("model override failed: {error}"),
                                },
                            );
                            let _ = msgs
                                .send(SessionMsg::TurnFinished {
                                    thread_id,
                                    turn_id,
                                    outcome,
                                })
                                .await;
                            return;
                        }
                        restore_models
                            .lock()
                            .await
                            .insert(thread_id.clone(), previous);
                    }
                }
                stored
                    .get(&thread_id)
                    .expect("engine inserted or already present")
                    .clone()
            };

            let mut accumulator = TurnAccumulator::new(&thread_id, &turn_id);
            let state = match engine.turn(&input, abort.clone()).await {
                Ok(stream) => drive_stream(stream, &mut accumulator, &msgs).await,
                Err(error) => TurnEndState::Failed {
                    error: error.to_string(),
                },
            };
            drop(engine);
            let _ = msgs
                .send(SessionMsg::TurnFinished {
                    thread_id,
                    turn_id,
                    outcome: accumulator.finish(state),
                })
                .await;
        });
    }
}
