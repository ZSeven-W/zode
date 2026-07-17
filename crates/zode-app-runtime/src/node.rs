use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};
use zode_node_protocol::{AgentCommand, AgentQuery, AgentSnapshot, EndpointError};

use crate::EventSink;

#[async_trait]
pub trait NodeBackend: Send + Sync + 'static {
    async fn command(&self, command: AgentCommand, events: EventSink) -> Result<(), EndpointError>;

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError>;
}

pub(crate) enum NodeRequest {
    Command {
        command: AgentCommand,
        response: oneshot::Sender<Result<(), EndpointError>>,
    },
    Query {
        query: AgentQuery,
        response: oneshot::Sender<Result<AgentSnapshot, EndpointError>>,
    },
}

pub(crate) async fn run(
    backend: Arc<dyn NodeBackend>,
    events: EventSink,
    mut requests: mpsc::Receiver<NodeRequest>,
) {
    while let Some(request) = requests.recv().await {
        match request {
            NodeRequest::Command { command, response } => {
                let result = backend.command(command, events.clone()).await;
                let _ = response.send(result);
            }
            NodeRequest::Query { query, response } => {
                let result = backend.query(query).await;
                let _ = response.send(result);
            }
        }
    }
}
