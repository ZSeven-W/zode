//! Approval coordination between engine/direct callers and JSON-RPC clients.

use std::collections::{HashMap, HashSet};

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use zode_app_server_protocol::server_requests::{
    approval_request, ApprovalDecision, ApprovalKind, ApprovalRequestParams,
};
use zode_app_server_protocol::{JsonRpcMessage, RequestId};
use zode_core::approval::{Approval, ApprovalRequest};

pub enum BrokerMsg {
    /// Engine-tool approval drained from zode-core's ApprovalReceiver.
    Engine(ApprovalRequest),
    /// Direct-method approval; reply goes back through the oneshot.
    Direct {
        kind: ApprovalKind,
        summary: String,
        reply: oneshot::Sender<bool>,
    },
    /// A client response or error frame whose id matched server id space.
    ClientResponse {
        id: String,
        decision: Option<ApprovalDecision>,
    },
    Shutdown,
}

enum Pending {
    Engine {
        request: ApprovalRequest,
        tool: String,
    },
    Direct {
        kind: DirectKind,
        reply: oneshot::Sender<bool>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum DirectKind {
    Command,
    FsWrite,
    Tool,
}

impl From<ApprovalKind> for DirectKind {
    fn from(kind: ApprovalKind) -> Self {
        match kind {
            ApprovalKind::Command => Self::Command,
            ApprovalKind::FsWrite => Self::FsWrite,
            ApprovalKind::Tool => Self::Tool,
        }
    }
}

pub struct ApprovalBroker;

impl ApprovalBroker {
    /// Spawns the broker task. `timeout_ms` is injectable for tests.
    pub fn spawn(
        outbound: mpsc::Sender<JsonRpcMessage>,
        timeout_ms: u64,
    ) -> (mpsc::Sender<BrokerMsg>, JoinHandle<()>) {
        let (sender, receiver) = mpsc::channel(128);
        let task_sender = sender.downgrade();
        let task = tokio::spawn(run(receiver, task_sender, outbound, timeout_ms));
        (sender, task)
    }
}

async fn run(
    mut receiver: mpsc::Receiver<BrokerMsg>,
    sender: mpsc::WeakSender<BrokerMsg>,
    outbound: mpsc::Sender<JsonRpcMessage>,
    timeout_ms: u64,
) {
    let mut next_id = 1_u64;
    let mut pending = HashMap::new();
    let mut engine_always = HashSet::new();
    let mut direct_always = HashSet::new();

    while let Some(message) = receiver.recv().await {
        match message {
            BrokerMsg::Engine(request) => {
                let tool = request.tool.clone();
                if engine_always.contains(&tool) {
                    let _ = request.respond(Approval::AllowAlways);
                    continue;
                }
                let params = ApprovalRequestParams {
                    approval_id: format!("srv-{next_id}"),
                    kind: ApprovalKind::Tool,
                    summary: request.summary(),
                    thread_id: None,
                    turn_id: None,
                    tool: Some(tool.clone()),
                    input: Some(request.input.clone()),
                };
                let id = params.approval_id.clone();
                next_id += 1;
                let frame = approval_request(RequestId::String(id.clone()), &params);
                if outbound.send(JsonRpcMessage::Request(frame)).await.is_err() {
                    let _ = request.respond(Approval::Deny);
                    continue;
                }
                pending.insert(id.clone(), Pending::Engine { request, tool });
                spawn_timeout(&sender, id, timeout_ms);
            }
            BrokerMsg::Direct {
                kind,
                summary,
                reply,
            } => {
                let memory_key = DirectKind::from(kind);
                if direct_always.contains(&memory_key) {
                    let _ = reply.send(true);
                    continue;
                }
                let id = format!("srv-{next_id}");
                next_id += 1;
                let params = ApprovalRequestParams {
                    approval_id: id.clone(),
                    kind,
                    summary,
                    thread_id: None,
                    turn_id: None,
                    tool: None,
                    input: None,
                };
                let frame = approval_request(RequestId::String(id.clone()), &params);
                if outbound.send(JsonRpcMessage::Request(frame)).await.is_err() {
                    let _ = reply.send(false);
                    continue;
                }
                pending.insert(
                    id.clone(),
                    Pending::Direct {
                        kind: memory_key,
                        reply,
                    },
                );
                spawn_timeout(&sender, id, timeout_ms);
            }
            BrokerMsg::ClientResponse { id, decision } => {
                let Some(request) = pending.remove(&id) else {
                    tracing::debug!(approval_id = %id, "ignoring late or unknown approval response");
                    continue;
                };
                resolve(request, decision, &mut engine_always, &mut direct_always);
            }
            BrokerMsg::Shutdown => break,
        }
    }

    for (_, request) in pending {
        deny(request);
    }
}

fn spawn_timeout(sender: &mpsc::WeakSender<BrokerMsg>, id: String, timeout_ms: u64) {
    let sender = sender.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(timeout_ms)).await;
        if let Some(sender) = sender.upgrade() {
            let _ = sender
                .send(BrokerMsg::ClientResponse { id, decision: None })
                .await;
        }
    });
}

fn resolve(
    request: Pending,
    decision: Option<ApprovalDecision>,
    engine_always: &mut HashSet<String>,
    direct_always: &mut HashSet<DirectKind>,
) {
    match request {
        Pending::Engine { request, tool } => {
            let approval = match decision {
                Some(ApprovalDecision::Allow) => Approval::AllowOnce,
                Some(ApprovalDecision::AllowAlways) => {
                    engine_always.insert(tool);
                    Approval::AllowAlways
                }
                Some(ApprovalDecision::Deny) | None => Approval::Deny,
            };
            let _ = request.respond(approval);
        }
        Pending::Direct { kind, reply } => {
            let allowed = match decision {
                Some(ApprovalDecision::Allow) => true,
                Some(ApprovalDecision::AllowAlways) => {
                    direct_always.insert(kind);
                    true
                }
                Some(ApprovalDecision::Deny) | None => false,
            };
            let _ = reply.send(allowed);
        }
    }
}

fn deny(request: Pending) {
    match request {
        Pending::Engine { request, .. } => {
            let _ = request.respond(Approval::Deny);
        }
        Pending::Direct { reply, .. } => {
            let _ = reply.send(false);
        }
    }
}
