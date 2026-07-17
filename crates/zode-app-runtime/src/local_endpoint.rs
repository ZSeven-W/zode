use std::sync::Arc;

use async_trait::async_trait;
use futures::stream;
use tokio::sync::{mpsc, oneshot, Mutex};
use zode_node_protocol::{
    AgentCommand, AgentEndpoint, AgentEvent, AgentEventStream, AgentQuery, AgentSnapshot,
    EndpointError, EndpointErrorKind,
};

use crate::event_sink::EventSink;
use crate::node::{self, NodeBackend, NodeRequest};

const NODE_MAILBOX_CAPACITY: usize = 32;

pub struct LocalAgentEndpoint {
    requests: mpsc::Sender<NodeRequest>,
    events: Mutex<Option<mpsc::Receiver<Result<AgentEvent, EndpointError>>>>,
}

impl LocalAgentEndpoint {
    pub fn spawn(backend: Arc<dyn NodeBackend>, event_capacity: usize) -> Self {
        let (event_sender, event_receiver) = mpsc::channel(event_capacity.max(1));
        let events = EventSink::new(event_sender);
        let (requests, request_receiver) = mpsc::channel(NODE_MAILBOX_CAPACITY);
        tokio::spawn(node::run(backend, events, request_receiver));

        Self {
            requests,
            events: Mutex::new(Some(event_receiver)),
        }
    }

    async fn request<T>(
        &self,
        request: NodeRequest,
        response: oneshot::Receiver<Result<T, EndpointError>>,
    ) -> Result<T, EndpointError> {
        self.requests
            .send(request)
            .await
            .map_err(|_| unavailable("local node actor is unavailable"))?;
        response
            .await
            .map_err(|_| unavailable("local node actor dropped its response"))?
    }
}

#[async_trait]
impl AgentEndpoint for LocalAgentEndpoint {
    async fn command(&self, command: AgentCommand) -> Result<(), EndpointError> {
        command.validate().map_err(|error| EndpointError {
            kind: EndpointErrorKind::InvalidRequest,
            message: error.to_string(),
        })?;

        let (sender, receiver) = oneshot::channel();
        self.request(
            NodeRequest::Command {
                command,
                response: sender,
            },
            receiver,
        )
        .await
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        let (sender, receiver) = oneshot::channel();
        self.request(
            NodeRequest::Query {
                query,
                response: sender,
            },
            receiver,
        )
        .await
    }

    async fn subscribe(&self) -> Result<AgentEventStream, EndpointError> {
        let receiver = self
            .events
            .lock()
            .await
            .take()
            .ok_or_else(|| EndpointError {
                kind: EndpointErrorKind::Busy,
                message: "local event stream has already been subscribed".into(),
            })?;

        Ok(Box::pin(stream::unfold(receiver, |mut receiver| async {
            receiver.recv().await.map(|event| (event, receiver))
        })))
    }
}

fn unavailable(message: impl Into<String>) -> EndpointError {
    EndpointError {
        kind: EndpointErrorKind::Unavailable,
        message: message.into(),
    }
}
