use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use zode_app_runtime::EventSink;
use zode_node_protocol::{
    AgentEvent, AgentEventKind, EndpointError, NodeId, SessionLocator, ToolCall, ToolStatus,
    TurnId, UsageSnapshot,
};

const DEADLINE: Duration = Duration::from_secs(2);

fn session(name: &str) -> SessionLocator {
    SessionLocator::new(NodeId::new(), name)
}

fn channel(capacity: usize) -> (EventSink, mpsc::Receiver<Result<AgentEvent, EndpointError>>) {
    let (tx, rx) = mpsc::channel(capacity);
    (EventSink::new(tx), rx)
}

async fn next_event(
    receiver: &mut mpsc::Receiver<Result<AgentEvent, EndpointError>>,
) -> AgentEvent {
    timeout(DEADLINE, receiver.recv())
        .await
        .expect("event timed out")
        .expect("event channel closed")
        .expect("event failed")
}

fn text(delta: &str) -> AgentEventKind {
    AgentEventKind::TextDelta {
        delta: delta.into(),
    }
}

#[tokio::test]
async fn slow_consumer_gets_all_text_and_the_finished_event() {
    let (sink, mut receiver) = channel(1);
    let session = session("slow");
    let turn = TurnId::new();
    let producer = tokio::spawn({
        let sink = sink.clone();
        let session = session.clone();
        async move {
            sink.send(session.clone(), turn, text("a")).await?;
            sink.send(session.clone(), turn, text("b")).await?;
            sink.send(
                session,
                turn,
                AgentEventKind::TurnFinished { interrupted: false },
            )
            .await
        }
    });

    let mut combined = String::new();
    let mut sequences = Vec::new();
    loop {
        let event = next_event(&mut receiver).await;
        sequences.push(event.sequence);
        match event.kind {
            AgentEventKind::TextDelta { delta } => combined.push_str(&delta),
            AgentEventKind::TurnFinished { interrupted: false } => break,
            other => panic!("unexpected event: {other:?}"),
        }
    }

    timeout(DEADLINE, producer)
        .await
        .expect("producer timed out")
        .expect("producer panicked")
        .expect("producer failed");
    assert_eq!(combined, "ab");
    assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
}

#[tokio::test]
async fn buffered_text_flushes_after_the_backend_goes_quiet() {
    let (sink, mut receiver) = channel(1);
    let session = session("quiet");
    let turn = TurnId::new();

    sink.send(session.clone(), turn, text("a")).await.unwrap();
    sink.send(session, turn, text("b")).await.unwrap();

    let mut combined = String::new();
    while combined.len() < 2 {
        match next_event(&mut receiver).await.kind {
            AgentEventKind::TextDelta { delta } => combined.push_str(&delta),
            other => panic!("unexpected event: {other:?}"),
        }
    }
    assert_eq!(combined, "ab");
}

#[tokio::test]
async fn coalescing_preserves_content_and_allocates_strict_sequences_on_enqueue() {
    let (sink, mut receiver) = channel(1);
    let session = session("coalesce");
    let turn = TurnId::new();
    let producer = tokio::spawn({
        let sink = sink.clone();
        let session = session.clone();
        async move {
            for delta in ["a", "b", "c", "d"] {
                sink.send(session.clone(), turn, text(delta)).await?;
            }
            sink.send(
                session,
                turn,
                AgentEventKind::TurnFinished { interrupted: false },
            )
            .await
        }
    });

    let mut combined = String::new();
    let mut sequences = Vec::new();
    loop {
        let event = next_event(&mut receiver).await;
        sequences.push(event.sequence);
        match event.kind {
            AgentEventKind::TextDelta { delta } => combined.push_str(&delta),
            AgentEventKind::TurnFinished { interrupted: false } => break,
            other => panic!("unexpected event: {other:?}"),
        }
    }
    timeout(DEADLINE, producer)
        .await
        .expect("producer timed out")
        .expect("producer panicked")
        .expect("producer failed");

    assert_eq!(combined, "abcd");
    assert_eq!(sequences.first(), Some(&1));
    assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
}

#[tokio::test]
async fn sequences_are_monotonic_per_session_across_turns() {
    let (sink, mut receiver) = channel(8);
    let first_session = session("first");
    let second_session = session("second");
    let first_turn = TurnId::new();
    let second_turn = TurnId::new();

    sink.send(first_session.clone(), first_turn, text("one"))
        .await
        .unwrap();
    sink.send(
        first_session.clone(),
        first_turn,
        AgentEventKind::TurnFinished { interrupted: false },
    )
    .await
    .unwrap();
    sink.send(first_session.clone(), second_turn, text("two"))
        .await
        .unwrap();
    sink.send(
        first_session.clone(),
        second_turn,
        AgentEventKind::TurnFinished { interrupted: false },
    )
    .await
    .unwrap();
    sink.send(second_session.clone(), TurnId::new(), text("other"))
        .await
        .unwrap();

    let mut first_sequences = Vec::new();
    let mut second_sequences = Vec::new();
    for _ in 0..5 {
        let event = next_event(&mut receiver).await;
        if event.session == first_session {
            first_sequences.push(event.sequence);
        } else if event.session == second_session {
            second_sequences.push(event.sequence);
        }
    }

    assert_eq!(first_sequences, [1, 2, 3, 4]);
    assert_eq!(second_sequences, [1]);
}

fn control_events() -> Vec<AgentEventKind> {
    let tool = || ToolCall {
        id: "tool-1".into(),
        name: "read_file".into(),
        status: ToolStatus::Running,
        summary: "read a file".into(),
        detail: None,
    };
    vec![
        AgentEventKind::ThinkingDelta {
            delta: "thought".into(),
        },
        AgentEventKind::ToolStarted { tool: tool() },
        AgentEventKind::ToolCompleted { tool: tool() },
        AgentEventKind::ApprovalRequested {
            approval_id: "approval-1".into(),
            tool: "shell".into(),
            summary: "run command".into(),
        },
        AgentEventKind::DiffInvalidated,
        AgentEventKind::Usage {
            usage: UsageSnapshot {
                input_tokens: 1,
                output_tokens: 2,
                context_used: None,
                cost_usd: None,
            },
        },
        AgentEventKind::StatusNotice {
            code: "working".into(),
            message: "still working".into(),
        },
        AgentEventKind::TurnFinished { interrupted: false },
        AgentEventKind::Error {
            message: "failed".into(),
            retryable: false,
        },
        AgentEventKind::Unknown,
    ]
}

#[tokio::test]
async fn every_non_text_event_is_a_lossless_ordering_barrier() {
    for control in control_events() {
        let (sink, mut receiver) = channel(1);
        let session = session("barrier");
        let turn = TurnId::new();
        sink.send(session.clone(), turn, text("before"))
            .await
            .unwrap();

        let producer = tokio::spawn({
            let sink = sink.clone();
            let session = session.clone();
            let control = control.clone();
            async move {
                sink.send(session.clone(), turn, control).await?;
                sink.send(session, turn, text("after")).await
            }
        });

        assert!(matches!(
            next_event(&mut receiver).await.kind,
            AgentEventKind::TextDelta { ref delta } if delta == "before"
        ));
        assert_eq!(next_event(&mut receiver).await.kind, control);
        assert!(matches!(
            next_event(&mut receiver).await.kind,
            AgentEventKind::TextDelta { ref delta } if delta == "after"
        ));
        timeout(DEADLINE, producer)
            .await
            .expect("producer timed out")
            .expect("producer panicked")
            .expect("producer failed");
    }
}
