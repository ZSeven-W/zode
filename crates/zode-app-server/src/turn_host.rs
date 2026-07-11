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
use zode_app_server_protocol::types::{ApprovalPolicy, Thread};
use zode_core::approval::{approval_queue, Approval};
use zode_core::config::ZodeConfig;
use zode_core::engine::{EngineTemplate, ZodeEngine};
use zode_core::sandbox::SandboxConfig;

use crate::accumulator::{TurnAccumulator, TurnEndState};
use crate::session::SessionMsg;

#[async_trait]
pub trait TurnHost: Send + 'static {
    /// Starts a turn and arranges for exactly one `TurnFinished` message eventually.
    async fn start_turn(
        &mut self,
        thread: &Thread,
        input: String,
        abort: AbortController,
        msgs: mpsc::Sender<SessionMsg>,
    );
}

pub trait HostFactory: Send + 'static {
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
            turn_ids,
            approval_pump,
        }
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
    async fn start_turn(
        &mut self,
        thread: &Thread,
        input: String,
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
        tokio::spawn(async move {
            let engine = {
                let existing = engines.lock().await.get(&thread_id).cloned();
                if let Some(engine) = existing {
                    engine
                } else {
                    match template
                        .assemble_tab(Some(cwd), Some(thread_id.clone()))
                        .await
                    {
                        Ok(engine) => {
                            let engine = Arc::new(engine);
                            engines
                                .lock()
                                .await
                                .insert(thread_id.clone(), engine.clone());
                            engine
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
                }
            };

            let mut accumulator = TurnAccumulator::new(&thread_id, &turn_id);
            let mut last_error = None;
            match engine.turn(&input, abort.clone()).await {
                Ok(mut stream) => {
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
                }
                Err(error) => last_error = Some(error.to_string()),
            }
            let state = last_error.map_or(TurnEndState::Completed, |error| TurnEndState::Failed {
                error,
            });
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
