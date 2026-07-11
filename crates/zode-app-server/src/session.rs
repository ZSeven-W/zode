use std::collections::BTreeMap;

use tokio::sync::{mpsc, oneshot};
use zode_app_server_protocol::notify;
use zode_app_server_protocol::rpc::{
    JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, RequestId,
    METHOD_NOT_FOUND, NOT_INITIALIZED, TURN_ACTIVE,
};
use zode_app_server_protocol::server_requests::{ApprovalDecision, ApprovalKind};
use zode_app_server_protocol::types::{
    ApprovalPolicy, EmptyResponse, InitializeParams, ModelSetParams, ThreadListResponse,
    ThreadNameSetParams, ThreadRefParams, ThreadResponse, ThreadStartParams, TurnInterruptParams,
    TurnResponse, TurnStartParams,
};
use zode_core::sandbox::SandboxConfig;

use crate::accumulator::{TurnEndState, TurnOutcome};
use crate::approval_broker::{ApprovalBroker, BrokerMsg};
use crate::error::error;
use crate::initialize::{handle_initialize, ConnectionState};
use crate::policy::{direct_gate, DirectGate, DirectKind};
use crate::router::{dispatch_stateless, method_kind, parse_params};
use crate::threads::ThreadRegistry;
use crate::turn_host::{HostFactory, TurnHost};
use crate::turns::TurnRegistry;

pub enum SessionMsg {
    Rpc(JsonRpcRequest),
    TurnEvent {
        notification: JsonRpcNotification,
    },
    TurnFinished {
        thread_id: String,
        turn_id: String,
        outcome: TurnOutcome,
    },
    ClientResponse {
        id: String,
        decision: Option<ApprovalDecision>,
    },
    Shutdown,
}

pub struct SessionActor {
    state: ConnectionState,
    threads: ThreadRegistry,
    turns: TurnRegistry,
    policy: ApprovalPolicy,
    host_factory: Box<dyn HostFactory>,
    host: Option<Box<dyn TurnHost>>,
    turn_ids: Option<mpsc::UnboundedSender<String>>,
    outbound: mpsc::Sender<JsonRpcMessage>,
    zode_home: String,
    sandbox: Option<SandboxConfig>,
    self_tx: mpsc::Sender<SessionMsg>,
    pending_deletes: BTreeMap<String, Vec<RequestId>>,
    shutting_down: bool,
    approval_timeout_ms: u64,
    broker: Option<mpsc::Sender<BrokerMsg>>,
}

impl SessionActor {
    pub fn spawn(
        host_factory: Box<dyn HostFactory>,
        outbound: mpsc::Sender<JsonRpcMessage>,
        zode_home: String,
        sandbox: Option<SandboxConfig>,
        approval_timeout_ms: u64,
    ) -> (mpsc::Sender<SessionMsg>, tokio::task::JoinHandle<()>) {
        let (tx, rx) = mpsc::channel(1024);
        let actor = Self {
            state: ConnectionState::default(),
            threads: ThreadRegistry::default(),
            turns: TurnRegistry::default(),
            policy: ApprovalPolicy::default(),
            host_factory,
            host: None,
            turn_ids: None,
            outbound,
            zode_home,
            sandbox,
            self_tx: tx.clone(),
            pending_deletes: BTreeMap::new(),
            shutting_down: false,
            approval_timeout_ms,
            broker: None,
        };
        let handle = tokio::spawn(actor.run(rx));
        (tx, handle)
    }

    async fn run(mut self, mut rx: mpsc::Receiver<SessionMsg>) {
        while let Some(msg) = rx.recv().await {
            match msg {
                SessionMsg::Rpc(request) if !self.shutting_down => self.rpc(request).await,
                SessionMsg::TurnEvent { notification } => {
                    let _ = self
                        .outbound
                        .send(JsonRpcMessage::Notification(notification))
                        .await;
                }
                SessionMsg::TurnFinished {
                    thread_id,
                    turn_id,
                    outcome,
                } => self.finished(thread_id, turn_id, outcome).await,
                SessionMsg::ClientResponse { id, decision } => {
                    if let Some(broker) = &self.broker {
                        let _ = broker
                            .send(BrokerMsg::ClientResponse { id, decision })
                            .await;
                    }
                }
                SessionMsg::Shutdown => {
                    self.shutting_down = true;
                    self.turns.abort_all();
                    if let Some(broker) = &self.broker {
                        let _ = broker.send(BrokerMsg::Shutdown).await;
                    }
                }
                SessionMsg::Rpc(_) => {}
            }
            if self.shutting_down && !self.turns.has_active() {
                break;
            }
        }
    }

    async fn rpc(&mut self, request: JsonRpcRequest) {
        if request.method == "initialize" {
            return self.initialize(request).await;
        }
        if !self.state.initialized {
            return self
                .send_error(request.id, error(NOT_INITIALIZED, "Not initialized"))
                .await;
        }
        match request.method.as_str() {
            "thread/start" => self.thread_start(request).await,
            "thread/list" => {
                self.send_value(
                    request.id,
                    ThreadListResponse {
                        threads: self.threads.list(),
                    },
                )
                .await
            }
            "thread/read" | "thread/resume" => self.thread_read(request).await,
            "thread/name/set" => self.thread_name(request).await,
            "model/set" => self.model_set(request).await,
            "thread/delete" => self.thread_delete(request).await,
            "turn/start" => self.turn_start(request).await,
            "turn/interrupt" => self.turn_interrupt(request).await,
            method => {
                let Some(kind) = method_kind(method) else {
                    return self
                        .send_error(
                            request.id,
                            error(METHOD_NOT_FOUND, format!("Method not found: {method}")),
                        )
                        .await;
                };
                if let Some(direct) = kind {
                    match direct_gate(self.policy, direct) {
                        DirectGate::Deny(err) => return self.send_error(request.id, err).await,
                        DirectGate::Allow => {}
                        DirectGate::Prompt => {
                            let outbound = self.outbound.clone();
                            let sandbox = self.sandbox.clone();
                            let broker =
                                self.broker.clone().expect("prompt policy requires broker");
                            tokio::spawn(async move {
                                let id = request.id;
                                let method = request.method;
                                let params = request.params;
                                let summary = format!(
                                    "{method} {}",
                                    params
                                        .as_ref()
                                        .map_or_else(|| "{}".into(), ToString::to_string)
                                );
                                let (reply, decision) = oneshot::channel();
                                let allowed = broker
                                    .send(BrokerMsg::Direct {
                                        kind: approval_kind(direct),
                                        summary,
                                        reply,
                                    })
                                    .await
                                    .is_ok()
                                    && decision.await.unwrap_or(false);
                                let message = if allowed {
                                    match dispatch_stateless(&method, params, sandbox.as_ref())
                                        .await
                                    {
                                        Ok(value) => JsonRpcMessage::Response(
                                            JsonRpcResponse::new(id, value),
                                        ),
                                        Err(err) => {
                                            JsonRpcMessage::Error(JsonRpcError::new(id, err))
                                        }
                                    }
                                } else {
                                    JsonRpcMessage::Error(JsonRpcError::new(
                                        id,
                                        error(
                                            zode_app_server_protocol::rpc::POLICY_DENIED,
                                            "denied by user",
                                        ),
                                    ))
                                };
                                let _ = outbound.send(message).await;
                            });
                            return;
                        }
                    }
                }
                let outbound = self.outbound.clone();
                let sandbox = self.sandbox.clone();
                tokio::spawn(async move {
                    let id = request.id;
                    let message =
                        match dispatch_stateless(&request.method, request.params, sandbox.as_ref())
                            .await
                        {
                            Ok(value) => JsonRpcMessage::Response(JsonRpcResponse::new(id, value)),
                            Err(err) => JsonRpcMessage::Error(JsonRpcError::new(id, err)),
                        };
                    let _ = outbound.send(message).await;
                });
            }
        }
    }

    async fn initialize(&mut self, request: JsonRpcRequest) {
        let params: InitializeParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(request.id, e).await,
        };
        let policy = params.approval_policy;
        match handle_initialize(&mut self.state, params, self.zode_home.clone()) {
            Ok(response) => {
                self.policy = policy;
                if policy == ApprovalPolicy::Prompt {
                    let (broker, _task) =
                        ApprovalBroker::spawn(self.outbound.clone(), self.approval_timeout_ms);
                    self.broker = Some(broker);
                }
                let (turn_ids, turn_ids_rx) = mpsc::unbounded_channel();
                self.host = Some(self.host_factory.build_host(
                    policy,
                    turn_ids_rx,
                    self.broker.clone(),
                ));
                self.turn_ids = Some(turn_ids);
                self.send_value(request.id, response).await;
            }
            Err(err) => self.send_error(request.id, err).await,
        }
    }
    async fn thread_start(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: ThreadStartParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        match self.threads.start_metadata_only(p, "(untitled)".into()) {
            Ok(thread) => self.send_value(id, ThreadResponse { thread }).await,
            Err(e) => self.send_error(id, e).await,
        }
    }
    async fn thread_read(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: ThreadRefParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        match self.threads.read(&p.thread_id) {
            Ok(thread) => self.send_value(id, ThreadResponse { thread }).await,
            Err(e) => self.send_error(id, e).await,
        }
    }
    async fn thread_name(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: ThreadNameSetParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        match self.threads.set_name(&p.thread_id, &p.name) {
            Ok(()) => self.send_value(id, EmptyResponse {}).await,
            Err(e) => self.send_error(id, e).await,
        }
    }
    async fn model_set(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: ModelSetParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        if self.turns.get(&p.thread_id).is_some() {
            return self
                .send_error(id, error(TURN_ACTIVE, "turn active for thread"))
                .await;
        }
        if let Err(e) = self.threads.read(&p.thread_id) {
            return self.send_error(id, e).await;
        }
        let result = self
            .host
            .as_mut()
            .expect("initialized session has a turn host")
            .set_model(&p.thread_id, &p.model)
            .await;
        match result {
            Ok(()) => match self.threads.set_model(&p.thread_id, &p.model) {
                Ok(()) => self.send_value(id, EmptyResponse {}).await,
                Err(e) => self.send_error(id, e).await,
            },
            Err(e) => self.send_error(id, e).await,
        }
    }
    async fn thread_delete(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: ThreadRefParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        if let Err(e) = self.threads.read(&p.thread_id) {
            return self.send_error(id, e).await;
        }
        if self.turns.abort_thread(&p.thread_id) {
            self.pending_deletes
                .entry(p.thread_id)
                .or_default()
                .push(id);
        } else {
            match self.threads.delete(&p.thread_id) {
                Ok(()) => self.send_value(id, EmptyResponse {}).await,
                Err(e) => self.send_error(id, e).await,
            }
        }
    }
    async fn turn_start(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: TurnStartParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        let thread = match self.threads.read(&p.thread_id) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        let turn_id = TurnRegistry::generate_id();
        let (turn, abort) = match self.turns.start(&p.thread_id, turn_id.clone()) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, error(TURN_ACTIVE, e)).await,
        };
        if p.model.is_some() {
            self.turns.mark_model_override(&p.thread_id);
        }
        self.send_value(id, TurnResponse { turn }).await;
        let _ = self
            .outbound
            .send(JsonRpcMessage::Notification(notify::turn_started(
                &p.thread_id,
                &turn_id,
            )))
            .await;
        if let (Some(turn_ids), Some(host)) = (&self.turn_ids, &mut self.host) {
            let _ = turn_ids.send(turn_id);
            host.start_turn(&thread, p.input, p.model, abort, self.self_tx.clone())
                .await;
        }
    }
    async fn turn_interrupt(&mut self, request: JsonRpcRequest) {
        let id = request.id;
        let p: TurnInterruptParams = match parse_params(request.params) {
            Ok(v) => v,
            Err(e) => return self.send_error(id, e).await,
        };
        if self.turns.interrupt(&p.thread_id, &p.turn_id) {
            self.send_value(id, EmptyResponse {}).await
        } else {
            self.send_value(id, serde_json::json!({"status":"finished"}))
                .await
        }
    }
    async fn finished(&mut self, thread_id: String, turn_id: String, outcome: TurnOutcome) {
        let Some(active) = self.turns.finish(&thread_id, &turn_id) else {
            return;
        };
        let notification = if active.interrupted {
            notify::turn_interrupted(&thread_id, &turn_id)
        } else {
            match outcome.state {
                TurnEndState::Completed => notify::turn_completed(
                    &thread_id,
                    &turn_id,
                    &outcome.final_text,
                    &outcome.usage,
                ),
                TurnEndState::Interrupted => notify::turn_interrupted(&thread_id, &turn_id),
                TurnEndState::Failed { error } => notify::turn_failed(&thread_id, &turn_id, &error),
            }
        };
        let _ = self
            .outbound
            .send(JsonRpcMessage::Notification(notification))
            .await;
        if active.has_model_override {
            if let Some(host) = &mut self.host {
                host.restore_model(&thread_id).await;
            }
        }
        if let Some(ids) = self.pending_deletes.remove(&thread_id) {
            let result = self.threads.delete(&thread_id);
            for id in ids {
                match &result {
                    Ok(()) => self.send_value(id, EmptyResponse {}).await,
                    Err(e) => self.send_error(id, e.clone()).await,
                }
            }
        }
    }
    async fn send_value<T: serde::Serialize>(&mut self, id: RequestId, value: T) {
        let value = serde_json::to_value(value).unwrap_or(serde_json::Value::Null);
        let _ = self
            .outbound
            .send(JsonRpcMessage::Response(JsonRpcResponse::new(id, value)))
            .await;
    }
    async fn send_error(&mut self, id: RequestId, err: zode_app_server_protocol::rpc::ErrorObject) {
        let _ = self
            .outbound
            .send(JsonRpcMessage::Error(JsonRpcError::new(id, err)))
            .await;
    }
}

fn approval_kind(kind: DirectKind) -> ApprovalKind {
    match kind {
        DirectKind::Command => ApprovalKind::Command,
        DirectKind::FsWrite => ApprovalKind::FsWrite,
    }
}
