use std::{collections::BTreeSet, sync::Arc};

use futures_util::StreamExt;
use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{reduce_agent_event, ConnectionState, ReduceOutcome, ZodeAppState};
use zode_node_protocol::{AgentEndpoint, AgentEvent, AgentEventKind, SessionLocator};

use crate::window_state::AppWake;

enum BridgeItem {
    Event(AgentEvent),
    Unavailable,
}

/// Main-thread drain for the endpoint stream. The producer always wakes winit,
/// so streamed text progresses while the user is completely idle.
pub struct AgentEventBridge {
    receiver: mpsc::UnboundedReceiver<BridgeItem>,
}

#[derive(Debug, Default)]
pub struct AgentEventDrain {
    pub applied: usize,
    pub diff_invalidated: BTreeSet<SessionLocator>,
}

impl AgentEventBridge {
    /// Must be called while a Tokio runtime is entered.
    pub fn spawn(endpoint: Arc<dyn AgentEndpoint>, proxy: EventLoopProxy<AppWake>) -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let Ok(mut events) = endpoint.subscribe().await else {
                enqueue(&sender, &proxy, BridgeItem::Unavailable);
                return;
            };
            while let Some(event) = events.next().await {
                match event {
                    Ok(event) => enqueue(&sender, &proxy, BridgeItem::Event(event)),
                    Err(_) => {
                        enqueue(&sender, &proxy, BridgeItem::Unavailable);
                        return;
                    }
                }
            }
            enqueue(&sender, &proxy, BridgeItem::Unavailable);
        });
        Self { receiver }
    }

    pub fn drain_into(&mut self, state: &mut ZodeAppState) -> AgentEventDrain {
        let mut summary = AgentEventDrain::default();
        while let Ok(item) = self.receiver.try_recv() {
            match item {
                BridgeItem::Event(event) => {
                    let diff_session = matches!(event.kind, AgentEventKind::DiffInvalidated)
                        .then(|| event.session.clone());
                    if reduce_agent_event(state, event) == ReduceOutcome::Applied {
                        summary.applied += 1;
                        if let Some(session) = diff_session {
                            summary.diff_invalidated.insert(session);
                        }
                    }
                }
                BridgeItem::Unavailable => state.host.connection = ConnectionState::Unavailable,
            }
        }
        summary
    }
}

fn enqueue(
    sender: &mpsc::UnboundedSender<BridgeItem>,
    proxy: &EventLoopProxy<AppWake>,
    item: BridgeItem,
) {
    if sender.send(item).is_ok() {
        let _ = proxy.send_event(AppWake::Redraw);
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use zode_app_model::{demo_state, TranscriptState};
    use zode_node_protocol::{
        AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
        WorkspaceUri, PROTOCOL_VERSION,
    };

    use super::{AgentEventBridge, BridgeItem};

    #[test]
    fn drain_reports_only_applied_diff_invalidations() {
        let mut state = demo_state();
        let session = SessionLocator::new(state.host.node_id, "session");
        let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
        let turn_id = TurnId::parse("00000000-0000-0000-0000-000000000002").unwrap();
        state.threads.push(ThreadSummary {
            session: session.clone(),
            workspace_uri,
            title: "session".into(),
            updated_at_ms: 0,
            status: ThreadStatus::Running,
        });
        state
            .transcripts
            .insert(session.clone(), TranscriptState::default());
        state.active_turns.insert(session.clone(), turn_id);
        let (sender, receiver) = mpsc::unbounded_channel();
        sender
            .send(BridgeItem::Event(AgentEvent {
                version: PROTOCOL_VERSION,
                session: session.clone(),
                turn_id,
                sequence: 1,
                kind: AgentEventKind::DiffInvalidated,
            }))
            .unwrap();
        sender
            .send(BridgeItem::Event(AgentEvent {
                version: PROTOCOL_VERSION,
                session: session.clone(),
                turn_id,
                sequence: 1,
                kind: AgentEventKind::DiffInvalidated,
            }))
            .unwrap();
        let mut bridge = AgentEventBridge { receiver };

        let summary = bridge.drain_into(&mut state);

        assert_eq!(summary.applied, 1);
        assert_eq!(
            summary.diff_invalidated.into_iter().collect::<Vec<_>>(),
            [session]
        );
    }
}
