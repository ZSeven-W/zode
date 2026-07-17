use std::{collections::BTreeSet, sync::Arc};

use futures_util::StreamExt;
use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{
    reduce_agent_event, AppCommand, ConnectionState, ReduceOutcome, ZodeAppState,
};
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
    /// Sessions whose applied event stream completed a turn during this drain.
    /// The desktop controller consumes this edge once to start at most one
    /// queued follow-up per finished turn.
    pub finished_sessions: BTreeSet<SessionLocator>,
}

impl AgentEventDrain {
    /// Converts each applied completion edge into one session-owned queue
    /// dispatch. The desktop controller consumes these commands after the
    /// whole event batch drains, so a session switch cannot retarget them.
    pub fn queue_dispatch_commands(&self) -> impl Iterator<Item = AppCommand> + '_ {
        self.finished_sessions
            .iter()
            .cloned()
            .map(|session| AppCommand::DispatchNextQueuedMessage { session })
    }
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
                    let finished_session =
                        matches!(event.kind, AgentEventKind::TurnFinished { .. })
                            .then(|| event.session.clone());
                    if reduce_agent_event(state, event) == ReduceOutcome::Applied {
                        summary.applied += 1;
                        if let Some(session) = diff_session {
                            summary.diff_invalidated.insert(session);
                        }
                        if let Some(session) = finished_session {
                            summary.finished_sessions.insert(session);
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
    use zode_app_model::{demo_state, AppCommand, ProjectState, TranscriptItem, TranscriptState};
    use zode_node_protocol::{
        AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
        UserContent, WorkspaceUri, PROTOCOL_VERSION,
    };

    use super::{AgentEventBridge, BridgeItem};
    use crate::command_bridge::prepare_queued_start;

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

    #[test]
    fn drain_reports_each_applied_turn_completion_once() {
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
        let transcript = TranscriptState {
            busy: true,
            ..TranscriptState::default()
        };
        state.transcripts.insert(session.clone(), transcript);
        state.active_turns.insert(session.clone(), turn_id);
        let (sender, receiver) = mpsc::unbounded_channel();
        let finished = AgentEvent {
            version: PROTOCOL_VERSION,
            session: session.clone(),
            turn_id,
            sequence: 1,
            kind: AgentEventKind::TurnFinished { interrupted: false },
        };
        sender.send(BridgeItem::Event(finished.clone())).unwrap();
        sender.send(BridgeItem::Event(finished)).unwrap();
        let mut bridge = AgentEventBridge { receiver };

        let summary = bridge.drain_into(&mut state);

        assert_eq!(summary.applied, 1);
        assert_eq!(
            summary.finished_sessions.into_iter().collect::<Vec<_>>(),
            [session]
        );
    }

    #[test]
    fn completion_edges_dispatch_one_queue_head_for_each_owning_session() {
        let mut state = demo_state();
        let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
        state.projects.push(ProjectState {
            workspace_uri: workspace_uri.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        });
        let session_a = SessionLocator::new(state.host.node_id, "session-a");
        let session_b = SessionLocator::new(state.host.node_id, "session-b");
        let turn_a = TurnId::parse("00000000-0000-0000-0000-00000000000a").unwrap();
        let turn_b = TurnId::parse("00000000-0000-0000-0000-00000000000b").unwrap();
        for (session, turn) in [(&session_a, turn_a), (&session_b, turn_b)] {
            state.threads.push(ThreadSummary {
                session: session.clone(),
                workspace_uri: workspace_uri.clone(),
                title: session.session_id.clone(),
                updated_at_ms: 0,
                status: ThreadStatus::Running,
            });
            state.transcripts.insert(
                session.clone(),
                TranscriptState {
                    busy: true,
                    ..TranscriptState::default()
                },
            );
            state.active_turns.insert(session.clone(), turn);
        }
        state.current_session = Some(session_b.clone());
        state.active_workspace = Some(workspace_uri);
        for (session, text) in [(&session_a, "head-a"), (&session_b, "head-b")] {
            let queue = state.message_queues.entry(session.clone()).or_default();
            queue.enqueue(text.into(), Vec::new()).unwrap();
            queue.enqueue(format!("tail-{text}"), Vec::new()).unwrap();
        }

        let (sender, receiver) = mpsc::unbounded_channel();
        for (session, turn) in [(&session_a, turn_a), (&session_b, turn_b)] {
            let finished = AgentEvent {
                version: PROTOCOL_VERSION,
                session: session.clone(),
                turn_id: turn,
                sequence: 1,
                kind: AgentEventKind::TurnFinished { interrupted: false },
            };
            sender.send(BridgeItem::Event(finished.clone())).unwrap();
            sender.send(BridgeItem::Event(finished)).unwrap();
        }
        let mut bridge = AgentEventBridge { receiver };

        let summary = bridge.drain_into(&mut state);
        let commands = summary.queue_dispatch_commands().collect::<Vec<_>>();
        assert_eq!(summary.applied, 2);
        assert_eq!(
            commands.len(),
            2,
            "duplicates cannot create extra dispatches"
        );

        for command in commands {
            let AppCommand::DispatchNextQueuedMessage { session } = command else {
                panic!("completion edges must produce only queue dispatch commands");
            };
            let head = state.message_queues[&session]
                .peek_next(None)
                .expect("each completed session owns a queue head")
                .text
                .clone();
            prepare_queued_start(
                &mut state,
                session.clone(),
                vec![UserContent::Text { text: head.clone() }],
            )
            .expect("the completion made this session idle");
            assert!(state.active_turns.contains_key(&session));
            assert!(matches!(
                state.transcripts[&session].items.last(),
                Some(TranscriptItem::UserText(text)) if text == &head
            ));
        }

        let second_drain = bridge.drain_into(&mut state);
        assert_eq!(second_drain.applied, 0);
        assert_eq!(second_drain.queue_dispatch_commands().count(), 0);
    }
}
