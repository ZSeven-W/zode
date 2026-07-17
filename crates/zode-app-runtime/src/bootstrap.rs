//! Composition root for the in-process desktop node.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use zode_core::approval::{Approval, ApprovalReceiver, ApprovalRequest};
use zode_core::bootstrap::ResolvedBootstrap;
use zode_core::question::QuestionReceiver;
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEventKind, AgentQuery, AgentSnapshot, ApprovalDecision,
    CapabilityManifest, EndpointError, EndpointErrorKind, NodeCapability, NodeId, SessionLocator,
    TurnId,
};

use crate::{
    persist_project_allow, workspace_uri_to_path, EngineBackend, EngineDriver, EventSink,
    LocalAgentEndpoint, LocalSessionRepository, NodeBackend, NodeIdentityStore, PersistedApproval,
    ZodeEngineDriver,
};

/// Fully composed local node plus the stable identity it advertises.
pub struct LocalAppRuntime {
    endpoint: Arc<LocalAgentEndpoint>,
    node_id: NodeId,
    capabilities: CapabilityManifest,
}

impl LocalAppRuntime {
    /// Compose the production driver from a resolved shared-core bootstrap.
    pub fn new(
        config_dir: impl AsRef<Path>,
        bootstrap: ResolvedBootstrap,
        event_capacity: usize,
    ) -> Result<Self, EndpointError> {
        let config_dir = config_dir.as_ref().to_path_buf();
        let node_id = load_node_id(&config_dir)?;
        let capabilities = capability_manifest(node_id, bootstrap.cfg.browser.enabled());
        let repository = LocalSessionRepository::new(&config_dir, node_id);
        let driver: Arc<dyn EngineDriver> = Arc::new(ZodeEngineDriver::new(
            node_id,
            bootstrap.template.clone(),
            repository,
            capabilities.clone(),
        ));
        Self::compose(bootstrap, driver, event_capacity, node_id, capabilities)
    }

    /// Injection seam used by lifecycle and composition contract tests.
    pub fn with_driver(
        config_dir: impl AsRef<Path>,
        bootstrap: ResolvedBootstrap,
        driver: Arc<dyn EngineDriver>,
        event_capacity: usize,
    ) -> Result<Self, EndpointError> {
        let config_dir = config_dir.as_ref().to_path_buf();
        let node_id = load_node_id(&config_dir)?;
        let capabilities = capability_manifest(node_id, bootstrap.cfg.browser.enabled());
        Self::compose(bootstrap, driver, event_capacity, node_id, capabilities)
    }

    fn compose(
        bootstrap: ResolvedBootstrap,
        driver: Arc<dyn EngineDriver>,
        event_capacity: usize,
        node_id: NodeId,
        capabilities: CapabilityManifest,
    ) -> Result<Self, EndpointError> {
        let engine = Arc::new(EngineBackend::new(node_id, driver));
        let active = Arc::new(Mutex::new(HashMap::new()));
        let approvals = Arc::new(Mutex::new(HashMap::new()));
        let backend = Arc::new(LocalRuntimeBackend {
            engine,
            capabilities: capabilities.clone(),
            active: active.clone(),
            approvals: approvals.clone(),
        });
        spawn_approval_pump(bootstrap.approval_rx, active, approvals);
        spawn_question_dismissal_pump(bootstrap.question_rx);
        let endpoint = Arc::new(LocalAgentEndpoint::spawn(backend, event_capacity));
        Ok(Self {
            endpoint,
            node_id,
            capabilities,
        })
    }

    pub fn endpoint(&self) -> Arc<LocalAgentEndpoint> {
        self.endpoint.clone()
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn capabilities(&self) -> &CapabilityManifest {
        &self.capabilities
    }
}

struct ActiveRoute {
    session: SessionLocator,
    turn_id: TurnId,
    events: EventSink,
}

struct PendingApproval {
    session: SessionLocator,
    tool: String,
    request: ApprovalRequest,
}

struct LocalRuntimeBackend {
    engine: Arc<EngineBackend>,
    capabilities: CapabilityManifest,
    active: Arc<Mutex<HashMap<String, ActiveRoute>>>,
    approvals: Arc<Mutex<HashMap<String, PendingApproval>>>,
}

#[async_trait]
impl NodeBackend for LocalRuntimeBackend {
    async fn command(&self, command: AgentCommand, events: EventSink) -> Result<(), EndpointError> {
        if command.session.node_id != self.capabilities.node_id {
            return Err(denied("session is not owned by the local node"));
        }
        match &command.kind {
            AgentCommandKind::StartTurn { .. } => {
                let turn_id = command.turn_id.ok_or_else(|| {
                    invalid("start turn requires a caller-allocated turn identity")
                })?;
                lock(&self.active).insert(
                    command.session.session_id.clone(),
                    ActiveRoute {
                        session: command.session.clone(),
                        turn_id,
                        events: events.clone(),
                    },
                );
            }
            AgentCommandKind::Approve {
                approval_id,
                decision,
            } => {
                return self
                    .resolve_approval(&command.session, approval_id, *decision)
                    .await;
            }
            AgentCommandKind::InterruptTurn => self.deny_pending(&command.session),
            _ => {}
        }
        let result = self.engine.command(command.clone(), events).await;
        if result.is_err() && matches!(command.kind, AgentCommandKind::StartTurn { .. }) {
            lock(&self.active).remove(&command.session.session_id);
        }
        result
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        if matches!(query, AgentQuery::Capabilities) {
            Ok(AgentSnapshot::Capabilities(self.capabilities.clone()))
        } else {
            self.engine.query(query).await
        }
    }
}

impl LocalRuntimeBackend {
    async fn resolve_approval(
        &self,
        session: &SessionLocator,
        approval_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), EndpointError> {
        let tool = pending_tool(&self.approvals, approval_id, session)?;
        let (approval, partial_success_message) = match decision {
            ApprovalDecision::AllowOnce => (Approval::AllowOnce, None),
            ApprovalDecision::AllowAlways => {
                let workspace = match self.engine.query(AgentQuery::Threads).await? {
                    AgentSnapshot::Threads(threads) => threads
                        .into_iter()
                        .find(|thread| &thread.session == session)
                        .map(|thread| thread.workspace_uri),
                    _ => None,
                }
                .ok_or_else(|| not_found("approval session workspace was not found"))?;
                let cwd = workspace_uri_to_path(&workspace)?;
                match persist_project_allow(&cwd, &tool) {
                    PersistedApproval::AllowAlways => (Approval::AllowAlways, None),
                    PersistedApproval::AllowOnceFallback { message } => {
                        (Approval::AllowOnce, Some(message))
                    }
                }
            }
            ApprovalDecision::Deny => (Approval::Deny, None),
        };

        // Fallible prerequisites above leave the request in the map so the UI
        // card remains genuinely retryable. Remove only once a response is
        // ready to be delivered.
        let pending = take_pending(&self.approvals, approval_id, session)?;
        respond_pending(pending, approval)?;
        if let Some(message) = partial_success_message {
            Err(partial_success(message))
        } else {
            Ok(())
        }
    }

    fn deny_pending(&self, session: &SessionLocator) {
        let mut approvals = lock(&self.approvals);
        let ids = approvals
            .iter()
            .filter(|(_, pending)| &pending.session == session)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        for id in ids {
            if let Some(pending) = approvals.remove(&id) {
                let _ = pending.request.respond(Approval::Deny);
            }
        }
    }
}

fn pending_tool(
    approvals: &Arc<Mutex<HashMap<String, PendingApproval>>>,
    approval_id: &str,
    session: &SessionLocator,
) -> Result<String, EndpointError> {
    let approvals = lock(approvals);
    let Some(current) = approvals.get(approval_id) else {
        return Err(request_expired("approval request is no longer pending"));
    };
    if &current.session != session {
        return Err(denied("approval request belongs to another session"));
    }
    Ok(current.tool.clone())
}

fn take_pending(
    approvals: &Arc<Mutex<HashMap<String, PendingApproval>>>,
    approval_id: &str,
    session: &SessionLocator,
) -> Result<PendingApproval, EndpointError> {
    let mut approvals = lock(approvals);
    let Some(current) = approvals.get(approval_id) else {
        return Err(request_expired("approval request is no longer pending"));
    };
    if &current.session != session {
        return Err(denied("approval request belongs to another session"));
    }
    Ok(approvals
        .remove(approval_id)
        .expect("approval was revalidated before removal"))
}

fn respond_pending(pending: PendingApproval, approval: Approval) -> Result<(), EndpointError> {
    pending
        .request
        .respond(approval)
        .map_err(|_| request_expired("approval requester is no longer available"))
}

fn spawn_approval_pump(
    mut receiver: ApprovalReceiver,
    active: Arc<Mutex<HashMap<String, ActiveRoute>>>,
    approvals: Arc<Mutex<HashMap<String, PendingApproval>>>,
) {
    tokio::spawn(async move {
        static NEXT_APPROVAL: AtomicU64 = AtomicU64::new(1);
        while let Some(request) = receiver.next().await {
            let Some(source) = request.source.clone() else {
                let _ = request.respond(Approval::Deny);
                continue;
            };
            let Some(route) = lock(&active).get(&source).map(|route| ActiveRoute {
                session: route.session.clone(),
                turn_id: route.turn_id,
                events: route.events.clone(),
            }) else {
                let _ = request.respond(Approval::Deny);
                continue;
            };
            let approval_id = format!(
                "local-approval-{}",
                NEXT_APPROVAL.fetch_add(1, Ordering::Relaxed)
            );
            let tool = request.tool.clone();
            let summary = request.summary();
            lock(&approvals).insert(
                approval_id.clone(),
                PendingApproval {
                    session: route.session.clone(),
                    tool: tool.clone(),
                    request,
                },
            );
            if route
                .events
                .send(
                    route.session,
                    route.turn_id,
                    AgentEventKind::ApprovalRequested {
                        approval_id: approval_id.clone(),
                        tool,
                        summary,
                    },
                )
                .await
                .is_err()
            {
                if let Some(pending) = lock(&approvals).remove(&approval_id) {
                    let _ = pending.request.respond(Approval::Deny);
                }
            }
        }
    });
}

fn spawn_question_dismissal_pump(mut receiver: QuestionReceiver) {
    // Protocol v1 has no question-answer command. Drain and dismiss requests so
    // the agent never hangs; a future protocol revision can expose the picker.
    tokio::spawn(async move {
        while let Some(request) = receiver.next().await {
            let _ = request.respond(None);
        }
    });
}

fn load_node_id(config_dir: &Path) -> Result<NodeId, EndpointError> {
    NodeIdentityStore::new(config_dir)
        .load_or_create()
        .map_err(|_| internal("local node identity could not be loaded"))
}

fn capability_manifest(node_id: NodeId, browser_enabled: bool) -> CapabilityManifest {
    let mut capabilities = BTreeSet::from([
        NodeCapability::Agent,
        NodeCapability::Workspace,
        NodeCapability::FileSystem,
        NodeCapability::Terminal,
        NodeCapability::Approval,
    ]);
    if browser_enabled {
        capabilities.insert(NodeCapability::Browser);
    }
    CapabilityManifest {
        node_id,
        capabilities,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn invalid(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::InvalidRequest, message)
}

fn not_found(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::NotFound, message)
}

fn denied(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::CapabilityDenied, message)
}

fn internal(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::Internal, message)
}

fn partial_success(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::PartialSuccess, message)
}

fn request_expired(message: impl Into<String>) -> EndpointError {
    endpoint_error(EndpointErrorKind::RequestExpired, message)
}

fn endpoint_error(kind: EndpointErrorKind, message: impl Into<String>) -> EndpointError {
    EndpointError {
        kind,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zode_core::approval::approval_queue;

    #[tokio::test]
    async fn approval_preflight_lookup_does_not_consume_the_pending_request() {
        let (queue, mut receiver) = approval_queue();
        let waiter = tokio::spawn(async move {
            queue
                .request("write_file", &serde_json::json!({}), Some("source".into()))
                .await
        });
        let request = receiver.next().await.unwrap();
        let session = SessionLocator::new(NodeId::new(), "session");
        let approvals = Arc::new(Mutex::new(HashMap::from([(
            "approval-1".into(),
            PendingApproval {
                session: session.clone(),
                tool: "write_file".into(),
                request,
            },
        )])));

        assert_eq!(
            pending_tool(&approvals, "approval-1", &session).unwrap(),
            "write_file"
        );
        assert!(lock(&approvals).contains_key("approval-1"));

        let pending = take_pending(&approvals, "approval-1", &session).unwrap();
        respond_pending(pending, Approval::Deny).unwrap();
        assert_eq!(waiter.await.unwrap(), Approval::Deny);
    }

    #[tokio::test]
    async fn dropped_approval_requester_has_a_structured_expired_error() {
        let (queue, mut receiver) = approval_queue();
        let waiter = tokio::spawn(async move {
            queue
                .request("write_file", &serde_json::json!({}), Some("source".into()))
                .await
        });
        let request = receiver.next().await.unwrap();
        waiter.abort();
        let _ = waiter.await;
        let pending = PendingApproval {
            session: SessionLocator::new(NodeId::new(), "session"),
            tool: "write_file".into(),
            request,
        };

        assert_eq!(
            respond_pending(pending, Approval::AllowOnce)
                .unwrap_err()
                .kind,
            EndpointErrorKind::RequestExpired
        );
    }
}
