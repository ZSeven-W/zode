use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
};

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
    wake: Arc<CoalescedWake>,
}

#[derive(Debug, Default)]
pub struct AgentEventDrain {
    pub applied: usize,
    pub changed: bool,
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
        let wake = Arc::new(CoalescedWake::new(move || {
            let _ = proxy.send_event(AppWake::Redraw);
        }));
        let producer_wake = Arc::clone(&wake);
        tokio::spawn(async move {
            let Ok(mut events) = endpoint.subscribe().await else {
                enqueue(&sender, &producer_wake, BridgeItem::Unavailable);
                return;
            };
            while let Some(event) = events.next().await {
                match event {
                    Ok(event) => enqueue(&sender, &producer_wake, BridgeItem::Event(event)),
                    Err(_) => {
                        enqueue(&sender, &producer_wake, BridgeItem::Unavailable);
                        return;
                    }
                }
            }
            enqueue(&sender, &producer_wake, BridgeItem::Unavailable);
        });
        Self { receiver, wake }
    }

    pub fn drain_into(&mut self, state: &mut ZodeAppState) -> AgentEventDrain {
        let mut summary = AgentEventDrain::default();
        if !self.wake.begin_drain() {
            return summary;
        }
        loop {
            while let Ok(item) = self.receiver.try_recv() {
                apply_item(state, &mut summary, item);
            }
            if !self.wake.finish_drain() {
                break;
            }
        }
        summary
    }
}

fn apply_item(state: &mut ZodeAppState, summary: &mut AgentEventDrain, item: BridgeItem) {
    match item {
        BridgeItem::Event(event) => {
            let diff_session = matches!(event.kind, AgentEventKind::DiffInvalidated)
                .then(|| event.session.clone());
            let finished_session = matches!(event.kind, AgentEventKind::TurnFinished { .. })
                .then(|| event.session.clone());
            if reduce_agent_event(state, event) == ReduceOutcome::Applied {
                summary.applied += 1;
                summary.changed = true;
                if let Some(session) = diff_session {
                    summary.diff_invalidated.insert(session);
                }
                if let Some(session) = finished_session {
                    summary.finished_sessions.insert(session);
                }
            }
        }
        BridgeItem::Unavailable => {
            if state.host.connection != ConnectionState::Unavailable {
                state.host.connection = ConnectionState::Unavailable;
                summary.changed = true;
            }
        }
    }
}

fn enqueue(sender: &mpsc::UnboundedSender<BridgeItem>, wake: &CoalescedWake, item: BridgeItem) {
    if sender.send(item).is_ok() {
        wake.notify();
    }
}

const WAKE_IDLE: u8 = 0;
const WAKE_SCHEDULED: u8 = 1;
const WAKE_DRAINING: u8 = 2;
const WAKE_DIRTY: u8 = 3;

/// Coalesces producer bursts without losing work that arrives while the main
/// thread is draining. Only the idle-to-scheduled edge emits a winit event.
pub(crate) struct CoalescedWake {
    state: AtomicU8,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl CoalescedWake {
    pub(crate) fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self {
            state: AtomicU8::new(WAKE_IDLE),
            wake: Arc::new(wake),
        }
    }

    pub(crate) fn notify(&self) {
        loop {
            match self.state.load(Ordering::Acquire) {
                WAKE_IDLE => {
                    if self
                        .state
                        .compare_exchange(
                            WAKE_IDLE,
                            WAKE_SCHEDULED,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        (self.wake)();
                        return;
                    }
                }
                WAKE_DRAINING => {
                    if self
                        .state
                        .compare_exchange(
                            WAKE_DRAINING,
                            WAKE_DIRTY,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return;
                    }
                }
                WAKE_SCHEDULED | WAKE_DIRTY => return,
                _ => unreachable!("coalesced wake state is valid"),
            }
        }
    }

    pub(crate) fn begin_drain(&self) -> bool {
        self.state
            .compare_exchange(
                WAKE_SCHEDULED,
                WAKE_DRAINING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Returns true when a producer published more work during this drain.
    pub(crate) fn finish_drain(&self) -> bool {
        loop {
            match self.state.load(Ordering::Acquire) {
                WAKE_DRAINING => {
                    if self
                        .state
                        .compare_exchange(
                            WAKE_DRAINING,
                            WAKE_IDLE,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return false;
                    }
                }
                WAKE_DIRTY => {
                    if self
                        .state
                        .compare_exchange(
                            WAKE_DIRTY,
                            WAKE_DRAINING,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        )
                        .is_ok()
                    {
                        return true;
                    }
                }
                WAKE_IDLE | WAKE_SCHEDULED => return false,
                _ => unreachable!("coalesced wake state is valid"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };

    use tokio::sync::mpsc;
    use zode_app_model::{demo_state, AppCommand, ProjectState, TranscriptItem, TranscriptState};
    use zode_node_protocol::{
        AgentEvent, AgentEventKind, SessionLocator, ThreadStatus, ThreadSummary, TurnId,
        UserContent, WorkspaceUri, PROTOCOL_VERSION,
    };

    use super::{AgentEventBridge, BridgeItem, CoalescedWake};
    use crate::command_bridge::prepare_queued_start;

    fn test_bridge(receiver: mpsc::UnboundedReceiver<BridgeItem>) -> AgentEventBridge {
        let wake = Arc::new(CoalescedWake::new(|| {}));
        wake.notify();
        AgentEventBridge { receiver, wake }
    }

    #[test]
    fn producer_burst_schedules_only_one_wake() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let gate = CoalescedWake::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        });

        for _ in 0..128 {
            gate.notify();
        }

        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        assert!(gate.begin_drain());
        assert!(!gate.finish_drain());
    }

    #[test]
    fn arrival_during_drain_is_consumed_then_gate_rearms() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let wake_count = Arc::clone(&wakes);
        let gate = Arc::new(CoalescedWake::new(move || {
            wake_count.fetch_add(1, Ordering::Relaxed);
        }));
        gate.notify();
        assert!(gate.begin_drain());

        let barrier = Arc::new(Barrier::new(2));
        let producer_gate = Arc::clone(&gate);
        let producer_barrier = Arc::clone(&barrier);
        let producer = std::thread::spawn(move || {
            producer_barrier.wait();
            producer_gate.notify();
        });
        barrier.wait();
        producer.join().unwrap();

        assert_eq!(wakes.load(Ordering::Relaxed), 1);
        assert!(
            gate.finish_drain(),
            "late work keeps the current drain alive"
        );
        assert!(!gate.finish_drain());
        gate.notify();
        assert_eq!(wakes.load(Ordering::Relaxed), 2);
    }

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
        let mut bridge = test_bridge(receiver);

        let summary = bridge.drain_into(&mut state);

        assert_eq!(summary.applied, 1);
        assert!(summary.changed);
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
        let mut bridge = test_bridge(receiver);

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
        let mut bridge = test_bridge(receiver);

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
        assert!(!second_drain.changed);
        assert_eq!(second_drain.queue_dispatch_commands().count(), 0);
    }
}
