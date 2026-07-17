use std::collections::{BTreeMap, VecDeque};

use tokio::sync::{mpsc, oneshot};
use zode_node_protocol::{
    AgentEvent, AgentEventKind, EndpointError, EndpointErrorKind, SessionLocator, TurnId,
    PROTOCOL_VERSION,
};

const INBOX_CAPACITY: usize = 64;
const PENDING_CAPACITY: usize = 64;
const MAX_COALESCED_TEXT_BYTES: usize = 64 * 1024;

/// A cloneable producer handle for the local endpoint's ordered event stream.
#[derive(Clone)]
pub struct EventSink {
    requests: mpsc::Sender<EmitRequest>,
}

impl EventSink {
    pub fn new(output: mpsc::Sender<Result<AgentEvent, EndpointError>>) -> Self {
        let (requests, receiver) = mpsc::channel(INBOX_CAPACITY);
        tokio::spawn(EventPump::new(output, receiver).run());
        Self { requests }
    }

    pub async fn send(
        &self,
        session: SessionLocator,
        turn_id: TurnId,
        kind: AgentEventKind,
    ) -> Result<(), EndpointError> {
        let (acknowledge, completion) = if matches!(&kind, AgentEventKind::TextDelta { .. }) {
            (None, None)
        } else {
            let (sender, receiver) = oneshot::channel();
            (Some(sender), Some(receiver))
        };

        self.requests
            .send(EmitRequest {
                session,
                turn_id,
                kind,
                acknowledge,
            })
            .await
            .map_err(|_| unavailable("event pump is unavailable"))?;

        if let Some(completion) = completion {
            completion
                .await
                .map_err(|_| unavailable("event pump stopped before enqueueing the event"))??;
        }

        Ok(())
    }
}

struct EmitRequest {
    session: SessionLocator,
    turn_id: TurnId,
    kind: AgentEventKind,
    acknowledge: Option<oneshot::Sender<Result<(), EndpointError>>>,
}

struct PendingEvent {
    session: SessionLocator,
    turn_id: TurnId,
    kind: AgentEventKind,
    acknowledge: Option<oneshot::Sender<Result<(), EndpointError>>>,
}

struct EventPump {
    output: mpsc::Sender<Result<AgentEvent, EndpointError>>,
    requests: mpsc::Receiver<EmitRequest>,
    pending: VecDeque<PendingEvent>,
    sequences: BTreeMap<SessionLocator, u64>,
    requests_closed: bool,
}

impl EventPump {
    fn new(
        output: mpsc::Sender<Result<AgentEvent, EndpointError>>,
        requests: mpsc::Receiver<EmitRequest>,
    ) -> Self {
        Self {
            output,
            requests,
            pending: VecDeque::new(),
            sequences: BTreeMap::new(),
            requests_closed: false,
        }
    }

    async fn run(mut self) {
        loop {
            if self.pending.is_empty() {
                if self.requests_closed {
                    return;
                }
                self.receive_one().await;
                continue;
            }

            if self.requests_closed || self.pending.len() >= PENDING_CAPACITY {
                if !self.enqueue_one().await {
                    return;
                }
                continue;
            }

            tokio::select! {
                biased;
                permit = self.output.clone().reserve_owned() => {
                    let Ok(permit) = permit else {
                        self.fail_pending(unavailable("event subscriber is unavailable"));
                        return;
                    };
                    if let Err(error) = self.enqueue_with_permit(permit) {
                        self.fail_pending(error);
                        return;
                    }
                }
                request = self.requests.recv() => {
                    match request {
                        Some(request) => self.accept(request),
                        None => self.requests_closed = true,
                    }
                }
            }
        }
    }

    async fn receive_one(&mut self) {
        match self.requests.recv().await {
            Some(request) => self.accept(request),
            None => self.requests_closed = true,
        }
    }

    fn accept(&mut self, request: EmitRequest) {
        if let AgentEventKind::TextDelta { delta } = request.kind {
            if let Some(PendingEvent {
                session,
                turn_id,
                kind: AgentEventKind::TextDelta { delta: pending },
                ..
            }) = self.pending.back_mut()
            {
                if *session == request.session
                    && *turn_id == request.turn_id
                    && pending.len() <= MAX_COALESCED_TEXT_BYTES.saturating_sub(delta.len())
                {
                    pending.push_str(&delta);
                    return;
                }
            }

            self.pending.push_back(PendingEvent {
                session: request.session,
                turn_id: request.turn_id,
                kind: AgentEventKind::TextDelta { delta },
                acknowledge: None,
            });
        } else {
            self.pending.push_back(PendingEvent {
                session: request.session,
                turn_id: request.turn_id,
                kind: request.kind,
                acknowledge: request.acknowledge,
            });
        }
    }

    async fn enqueue_one(&mut self) -> bool {
        match self.output.clone().reserve_owned().await {
            Ok(permit) => match self.enqueue_with_permit(permit) {
                Ok(()) => true,
                Err(error) => {
                    self.fail_pending(error);
                    false
                }
            },
            Err(_) => {
                self.fail_pending(unavailable("event subscriber is unavailable"));
                false
            }
        }
    }

    fn enqueue_with_permit(
        &mut self,
        permit: mpsc::OwnedPermit<Result<AgentEvent, EndpointError>>,
    ) -> Result<(), EndpointError> {
        let pending = self
            .pending
            .pop_front()
            .expect("an output permit is only requested for pending events");
        let sequence = self
            .sequences
            .get(&pending.session)
            .copied()
            .unwrap_or_default()
            .checked_add(1)
            .ok_or_else(|| internal("event sequence exhausted"));

        let sequence = match sequence {
            Ok(sequence) => sequence,
            Err(error) => {
                permit.send(Err(error.clone()));
                if let Some(acknowledge) = pending.acknowledge {
                    let _ = acknowledge.send(Err(error.clone()));
                }
                return Err(error);
            }
        };
        self.sequences.insert(pending.session.clone(), sequence);

        permit.send(Ok(AgentEvent {
            version: PROTOCOL_VERSION,
            session: pending.session,
            turn_id: pending.turn_id,
            sequence,
            kind: pending.kind,
        }));
        if let Some(acknowledge) = pending.acknowledge {
            let _ = acknowledge.send(Ok(()));
        }
        Ok(())
    }

    fn fail_pending(&mut self, error: EndpointError) {
        for pending in self.pending.drain(..) {
            if let Some(acknowledge) = pending.acknowledge {
                let _ = acknowledge.send(Err(error.clone()));
            }
        }
    }
}

fn unavailable(message: impl Into<String>) -> EndpointError {
    EndpointError {
        kind: EndpointErrorKind::Unavailable,
        message: message.into(),
    }
}

fn internal(message: impl Into<String>) -> EndpointError {
    EndpointError {
        kind: EndpointErrorKind::Internal,
        message: message.into(),
    }
}
