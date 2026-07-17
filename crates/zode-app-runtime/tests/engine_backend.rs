use agent::abort::AbortController;
use agent::error::AgentError;
use agent::stream::{Event, ResultData};
use async_trait::async_trait;
use futures::{stream, StreamExt};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tokio::time::timeout;
use zode_app_runtime::{
    DriverEventStream, EngineBackend, EngineDriver, EventNormalizer, EventSink, NodeBackend,
};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEvent, AgentEventKind, AgentQuery, AgentSnapshot,
    EndpointError, EndpointErrorKind, NodeId, RuntimeOptions, SandboxMode, SessionLocator,
    ToolStatus, TurnId, UserContent, WorkspaceUri, PROTOCOL_VERSION,
};

fn normalize(normalizer: &mut EventNormalizer, event: Event) -> Option<AgentEventKind> {
    normalizer.normalize(event)
}

#[test]
fn text_delta_maps_without_rewriting_content() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::TextDelta {
            delta: "hello".into(),
        },
    );

    assert_eq!(
        event,
        Some(AgentEventKind::TextDelta {
            delta: "hello".into()
        })
    );
}

#[test]
fn thinking_maps_to_thinking_delta() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Thinking {
            delta: "considering".into(),
        },
    );

    assert_eq!(
        event,
        Some(AgentEventKind::ThinkingDelta {
            delta: "considering".into()
        })
    );
}

#[test]
fn tool_use_emits_a_safe_summary_without_raw_arguments() {
    let mut normalizer = EventNormalizer::new();
    let secret = "raw-api-token-should-never-reach-ui";

    let event = normalize(
        &mut normalizer,
        Event::ToolUse {
            id: "tool-1".into(),
            name: "FileWrite".into(),
            input: serde_json::json!({
                "path": "/tmp/note.md",
                "api_token": secret,
                "content": "large raw contents",
            }),
        },
    )
    .unwrap();

    let AgentEventKind::ToolStarted { tool } = event else {
        panic!("expected ToolStarted");
    };
    assert_eq!(tool.id, "tool-1");
    assert_eq!(tool.name, "FileWrite");
    assert_eq!(tool.status, ToolStatus::Running);
    assert!(tool.summary.contains("/tmp/note.md"));
    assert_eq!(tool.detail, None);
    assert!(!format!("{tool:?}").contains(secret));
    assert!(!tool.summary.contains("large raw contents"));
}

#[test]
fn successful_tool_result_uses_the_cached_tool_name() {
    let mut normalizer = EventNormalizer::new();
    normalize(
        &mut normalizer,
        Event::ToolUse {
            id: "tool-ok".into(),
            name: "Bash".into(),
            input: serde_json::json!({"command": "pwd"}),
        },
    );

    let event = normalize(
        &mut normalizer,
        Event::ToolResult {
            id: "tool-ok".into(),
            ok: true,
            output: serde_json::json!({"stdout": "/tmp"}),
        },
    )
    .unwrap();

    let AgentEventKind::ToolCompleted { tool } = event else {
        panic!("expected ToolCompleted");
    };
    assert_eq!(tool.id, "tool-ok");
    assert_eq!(tool.name, "Bash");
    assert_eq!(tool.status, ToolStatus::Completed);
}

#[test]
fn failed_tool_result_maps_to_failed_status() {
    let mut normalizer = EventNormalizer::new();
    normalize(
        &mut normalizer,
        Event::ToolUse {
            id: "tool-failed".into(),
            name: "FileRead".into(),
            input: serde_json::json!({"path": "/missing"}),
        },
    );

    let event = normalize(
        &mut normalizer,
        Event::ToolResult {
            id: "tool-failed".into(),
            ok: false,
            output: serde_json::json!({"error": "not found"}),
        },
    )
    .unwrap();

    let AgentEventKind::ToolCompleted { tool } = event else {
        panic!("expected ToolCompleted");
    };
    assert_eq!(tool.name, "FileRead");
    assert_eq!(tool.status, ToolStatus::Failed);
}

#[test]
fn usage_frames_remain_cumulative_instead_of_being_double_counted() {
    let mut normalizer = EventNormalizer::new();

    let first = normalize(
        &mut normalizer,
        Event::Usage {
            input_tokens: 10,
            output_tokens: 2,
            cache_read: 0,
            cache_create: 0,
        },
    )
    .unwrap();
    let second = normalize(
        &mut normalizer,
        Event::Usage {
            input_tokens: 15,
            output_tokens: 4,
            cache_read: 0,
            cache_create: 0,
        },
    )
    .unwrap();

    let AgentEventKind::Usage { usage: first } = first else {
        panic!("expected Usage");
    };
    let AgentEventKind::Usage { usage: second } = second else {
        panic!("expected Usage");
    };
    assert_eq!((first.input_tokens, first.output_tokens), (10, 2));
    assert_eq!((second.input_tokens, second.output_tokens), (15, 4));
}

#[test]
fn notice_preserves_the_diagnostic_code_and_message() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Notice {
            code: "api_retry".into(),
            message: "retrying request".into(),
        },
    );

    assert_eq!(
        event,
        Some(AgentEventKind::StatusNotice {
            code: "api_retry".into(),
            message: "retrying request".into(),
        })
    );
}

#[test]
fn recoverable_agent_error_keeps_the_stream_retryable() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Error {
            code: "provider_error".into(),
            message: "request failed".into(),
        },
    )
    .unwrap();

    let AgentEventKind::Error { message, retryable } = event else {
        panic!("expected Error");
    };
    assert!(message.contains("request failed"));
    assert!(retryable);
}

#[test]
fn unknown_event_becomes_a_diagnostic_notice() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(&mut normalizer, Event::Unknown);

    assert!(matches!(
        event,
        Some(AgentEventKind::StatusNotice { code, message })
            if code == "agent.event.unknown" && !message.is_empty()
    ));
}

#[test]
fn result_metadata_does_not_finish_the_turn_early() {
    let mut normalizer = EventNormalizer::new();

    let event = normalize(
        &mut normalizer,
        Event::Result {
            data: ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("test-model".into()),
                metadata: Default::default(),
            },
        },
    );

    assert_eq!(event, None);
}

const ASYNC_DEADLINE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
struct FinishCall {
    session: SessionLocator,
    turn_id: TurnId,
    model: Option<String>,
    interrupted: bool,
}

struct FakeDriver {
    commands: Mutex<Vec<AgentCommand>>,
    starts: Mutex<Vec<AgentCommand>>,
    aborts: Mutex<Vec<(SessionLocator, TurnId, AbortController)>>,
    streams: Mutex<VecDeque<DriverEventStream>>,
    finishes: Mutex<Vec<FinishCall>>,
    start_seen: Semaphore,
    finish_seen: Semaphore,
    finish_gate: Option<Arc<Semaphore>>,
}

impl FakeDriver {
    fn new(streams: Vec<DriverEventStream>) -> Self {
        Self::with_finish_gate(streams, None)
    }

    fn with_finish_gate(
        streams: Vec<DriverEventStream>,
        finish_gate: Option<Arc<Semaphore>>,
    ) -> Self {
        Self {
            commands: Mutex::new(Vec::new()),
            starts: Mutex::new(Vec::new()),
            aborts: Mutex::new(Vec::new()),
            streams: Mutex::new(streams.into()),
            finishes: Mutex::new(Vec::new()),
            start_seen: Semaphore::new(0),
            finish_seen: Semaphore::new(0),
            finish_gate,
        }
    }

    fn abort_for(&self, session: &SessionLocator, turn_id: TurnId) -> AbortController {
        self.aborts
            .lock()
            .unwrap()
            .iter()
            .find(|(candidate, candidate_turn, _)| {
                candidate == session && *candidate_turn == turn_id
            })
            .expect("turn abort controller was recorded")
            .2
            .clone()
    }
}

#[async_trait]
impl EngineDriver for FakeDriver {
    async fn command(&self, command: AgentCommand) -> Result<(), EndpointError> {
        self.commands.lock().unwrap().push(command);
        Ok(())
    }

    async fn start_turn(&self, command: AgentCommand, abort: AbortController) -> DriverEventStream {
        let turn_id = command.turn_id.expect("start turn has an identity");
        self.aborts
            .lock()
            .unwrap()
            .push((command.session.clone(), turn_id, abort));
        self.starts.lock().unwrap().push(command);
        self.start_seen.add_permits(1);
        self.streams
            .lock()
            .unwrap()
            .pop_front()
            .expect("a stream was prepared for every successful start")
    }

    async fn finish_turn(
        &self,
        session: &SessionLocator,
        turn_id: TurnId,
        model: Option<String>,
        interrupted: bool,
    ) -> Result<(), EndpointError> {
        self.finishes.lock().unwrap().push(FinishCall {
            session: session.clone(),
            turn_id,
            model,
            interrupted,
        });
        self.finish_seen.add_permits(1);
        if let Some(gate) = self.finish_gate.clone() {
            gate.acquire_owned()
                .await
                .expect("finish gate stays open")
                .forget();
        }
        Ok(())
    }

    async fn query(&self, _query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        Ok(AgentSnapshot::RuntimeOptions(RuntimeOptions {
            models: vec!["test-model".into()],
            active_model: Some("test-model".into()),
            effort: None,
            sandbox_mode: SandboxMode::WorkspaceWrite,
            sandbox_network: false,
        }))
    }
}

fn test_session(node_id: NodeId, name: &str) -> SessionLocator {
    SessionLocator::new(node_id, name)
}

fn start_command(session: SessionLocator, turn_id: TurnId) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id: Some(turn_id),
        kind: AgentCommandKind::StartTurn {
            input: vec![UserContent::Text { text: "run".into() }],
        },
    }
}

fn interrupt_command(session: SessionLocator, turn_id: TurnId) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id: Some(turn_id),
        kind: AgentCommandKind::InterruptTurn,
    }
}

fn steer_command(session: SessionLocator, turn_id: TurnId) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id: Some(turn_id),
        kind: AgentCommandKind::SteerTurn {
            input: vec![UserContent::Text {
                text: "follow up".into(),
            }],
        },
    }
}

fn create_command(session: SessionLocator) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id: None,
        kind: AgentCommandKind::CreateSession {
            workspace_uri: WorkspaceUri::new("file:///tmp/zode-lifecycle").unwrap(),
            model: Some("test-model".into()),
        },
    }
}

fn driver_stream() -> (
    mpsc::UnboundedSender<Result<Event, AgentError>>,
    DriverEventStream,
) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let stream = stream::unfold(receiver, |mut receiver| async move {
        receiver.recv().await.map(|event| (event, receiver))
    });
    (sender, Box::pin(stream))
}

fn gated_late_stream() -> (oneshot::Sender<()>, DriverEventStream) {
    let (release, wait) = oneshot::channel();
    let late = stream::once(async move {
        wait.await.expect("late-event gate stays open");
        Ok(Event::TextDelta {
            delta: "late".into(),
        })
    });
    let aborted = stream::iter([Err(AgentError::Aborted("interrupted".into()))]);
    (release, Box::pin(late.chain(aborted)))
}

fn event_sink() -> (EventSink, mpsc::Receiver<Result<AgentEvent, EndpointError>>) {
    let (sender, receiver) = mpsc::channel(32);
    (EventSink::new(sender), receiver)
}

async fn consume_permit(semaphore: &Semaphore, label: &str) {
    timeout(ASYNC_DEADLINE, semaphore.acquire())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
        .expect("semaphore stays open")
        .forget();
}

async fn receive_event(
    receiver: &mut mpsc::Receiver<Result<AgentEvent, EndpointError>>,
) -> AgentEvent {
    timeout(ASYNC_DEADLINE, receiver.recv())
        .await
        .expect("timed out waiting for an agent event")
        .expect("agent event channel closed")
        .expect("agent event carried an endpoint error")
}

async fn collect_until_finished(
    receiver: &mut mpsc::Receiver<Result<AgentEvent, EndpointError>>,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    loop {
        let event = receive_event(receiver).await;
        let finished = matches!(event.kind, AgentEventKind::TurnFinished { .. });
        events.push(event);
        if finished {
            return events;
        }
    }
}

#[tokio::test]
async fn normal_turn_persists_before_diff_and_finishes_exactly_once() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "normal");
    let turn_id = TurnId::new();
    let (stream_sender, stream) = driver_stream();
    let finish_gate = Arc::new(Semaphore::new(0));
    let driver = Arc::new(FakeDriver::with_finish_gate(
        vec![stream],
        Some(finish_gate.clone()),
    ));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    timeout(
        ASYNC_DEADLINE,
        backend.command(start_command(session.clone(), turn_id), events),
    )
    .await
    .expect("start command waited for stream completion")
    .unwrap();
    consume_permit(&driver.start_seen, "turn start").await;

    stream_sender
        .send(Ok(Event::TextDelta {
            delta: "hello".into(),
        }))
        .unwrap();
    stream_sender
        .send(Ok(Event::Usage {
            input_tokens: 8,
            output_tokens: 3,
            cache_read: 0,
            cache_create: 0,
        }))
        .unwrap();
    stream_sender
        .send(Ok(Event::Result {
            data: ResultData {
                stop_reason: Some("end_turn".into()),
                model: Some("resolved-model".into()),
                metadata: Default::default(),
            },
        }))
        .unwrap();
    drop(stream_sender);

    consume_permit(&driver.finish_seen, "finish persistence barrier").await;
    let text = receive_event(&mut receiver).await;
    let usage = receive_event(&mut receiver).await;
    assert!(matches!(text.kind, AgentEventKind::TextDelta { .. }));
    assert!(matches!(usage.kind, AgentEventKind::Usage { .. }));
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));

    finish_gate.add_permits(1);
    let diff = receive_event(&mut receiver).await;
    let finished = receive_event(&mut receiver).await;
    assert!(matches!(diff.kind, AgentEventKind::DiffInvalidated));
    assert!(matches!(
        finished.kind,
        AgentEventKind::TurnFinished { interrupted: false }
    ));
    assert_eq!(
        driver.finishes.lock().unwrap().as_slice(),
        &[FinishCall {
            session,
            turn_id,
            model: Some("resolved-model".into()),
            interrupted: false,
        }]
    );
}

#[tokio::test]
async fn turn_finished_releases_the_session_before_the_event_is_observable() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "immediate-follow-up");
    let first_turn = TurnId::new();
    let second_turn = TurnId::new();
    let (first_sender, first_stream) = driver_stream();
    let (second_sender, second_stream) = driver_stream();
    let driver = Arc::new(FakeDriver::new(vec![first_stream, second_stream]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    backend
        .command(start_command(session.clone(), first_turn), events.clone())
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "first turn start").await;
    drop(first_sender);

    let first_events = collect_until_finished(&mut receiver).await;
    assert!(matches!(
        first_events.last().map(|event| &event.kind),
        Some(AgentEventKind::TurnFinished { interrupted: false })
    ));

    let stale_steer = backend
        .command(steer_command(session.clone(), first_turn), events.clone())
        .await
        .expect_err("a completed turn cannot accept stale guidance");
    assert_eq!(stale_steer.kind, EndpointErrorKind::NotFound);

    backend
        .command(start_command(session.clone(), second_turn), events.clone())
        .await
        .expect("TurnFinished makes the same session immediately startable");
    consume_permit(&driver.start_seen, "immediate follow-up start").await;
    assert_eq!(driver.starts.lock().unwrap().len(), 2);

    second_sender
        .send(Err(AgentError::Aborted("cleanup".into())))
        .unwrap();
    drop(second_sender);
    let second_events = collect_until_finished(&mut receiver).await;
    assert!(matches!(
        second_events.last().map(|event| &event.kind),
        Some(AgentEventKind::TurnFinished { interrupted: true })
    ));
}

#[tokio::test]
async fn one_session_is_busy_while_another_session_can_start() {
    let node_id = NodeId::new();
    let session_a = test_session(node_id, "a");
    let session_b = test_session(node_id, "b");
    let turn_a = TurnId::new();
    let turn_b = TurnId::new();
    let (sender_a, stream_a) = driver_stream();
    let (sender_b, stream_b) = driver_stream();
    let driver = Arc::new(FakeDriver::new(vec![stream_a, stream_b]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, _receiver) = event_sink();

    backend
        .command(start_command(session_a.clone(), turn_a), events.clone())
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "first session start").await;
    let error = backend
        .command(
            start_command(session_a.clone(), TurnId::new()),
            events.clone(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind, EndpointErrorKind::Busy);

    backend
        .command(start_command(session_b.clone(), turn_b), events)
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "second session start").await;
    assert_eq!(driver.starts.lock().unwrap().len(), 2);

    sender_a
        .send(Err(AgentError::Aborted("cleanup".into())))
        .unwrap();
    sender_b
        .send(Err(AgentError::Aborted("cleanup".into())))
        .unwrap();
}

#[tokio::test]
async fn only_matching_interrupt_aborts_and_late_text_is_dropped() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "interrupt");
    let turn_id = TurnId::new();
    let (release_late, stream) = gated_late_stream();
    let driver = Arc::new(FakeDriver::new(vec![stream]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    backend
        .command(start_command(session.clone(), turn_id), events.clone())
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "interruptible turn start").await;
    let abort = driver.abort_for(&session, turn_id);

    let wrong = backend
        .command(
            interrupt_command(session.clone(), TurnId::new()),
            events.clone(),
        )
        .await
        .unwrap_err();
    assert_eq!(wrong.kind, EndpointErrorKind::NotFound);
    assert!(!abort.is_aborted());

    backend
        .command(interrupt_command(session.clone(), turn_id), events)
        .await
        .unwrap();
    assert!(abort.is_aborted());
    release_late.send(()).unwrap();

    let emitted = collect_until_finished(&mut receiver).await;
    assert!(!emitted.iter().any(|event| matches!(
        event.kind,
        AgentEventKind::TextDelta { .. } | AgentEventKind::Error { .. }
    )));
    assert_eq!(
        emitted
            .iter()
            .filter(|event| matches!(event.kind, AgentEventKind::TurnFinished { .. }))
            .count(),
        1
    );
    assert_eq!(driver.finishes.lock().unwrap().len(), 1);
    assert!(driver.finishes.lock().unwrap()[0].interrupted);
}

#[tokio::test]
async fn create_session_preserves_the_caller_locator_and_query_delegates() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "caller-owned");
    let command = create_command(session.clone());
    let driver = Arc::new(FakeDriver::new(Vec::new()));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, _receiver) = event_sink();

    backend.command(command.clone(), events).await.unwrap();
    let snapshot = backend.query(AgentQuery::RuntimeOptions).await.unwrap();

    assert_eq!(driver.commands.lock().unwrap().as_slice(), &[command]);
    assert_eq!(driver.commands.lock().unwrap()[0].session, session);
    assert!(matches!(snapshot, AgentSnapshot::RuntimeOptions(_)));
}

#[tokio::test]
async fn session_runtime_options_query_rejects_a_remote_session_before_delegating() {
    let node_id = NodeId::new();
    let remote = test_session(NodeId::new(), "remote");
    let driver = Arc::new(FakeDriver::new(Vec::new()));
    let backend = EngineBackend::new(node_id, driver);

    let error = backend
        .query(AgentQuery::SessionRuntimeOptions { session: remote })
        .await
        .unwrap_err();

    assert_eq!(error.kind, EndpointErrorKind::CapabilityDenied);
}

#[tokio::test]
async fn aborted_stream_finishes_as_interrupted_without_an_error_event() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "aborted-stream");
    let turn_id = TurnId::new();
    let (sender, stream) = driver_stream();
    let driver = Arc::new(FakeDriver::new(vec![stream]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    backend
        .command(start_command(session, turn_id), events)
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "aborted turn start").await;
    sender
        .send(Err(AgentError::Aborted("user cancelled".into())))
        .unwrap();
    drop(sender);

    let emitted = collect_until_finished(&mut receiver).await;
    assert!(!emitted
        .iter()
        .any(|event| matches!(event.kind, AgentEventKind::Error { .. })));
    assert!(matches!(
        emitted.last().map(|event| &event.kind),
        Some(AgentEventKind::TurnFinished { interrupted: true })
    ));
    assert_eq!(driver.finishes.lock().unwrap().len(), 1);
    assert!(driver.finishes.lock().unwrap()[0].interrupted);
}

#[tokio::test]
async fn fatal_stream_error_is_terminal_and_still_finishes_exactly_once() {
    let node_id = NodeId::new();
    let session = test_session(node_id, "fatal-stream");
    let turn_id = TurnId::new();
    let (sender, stream) = driver_stream();
    let driver = Arc::new(FakeDriver::new(vec![stream]));
    let backend = EngineBackend::new(node_id, driver.clone());
    let (events, mut receiver) = event_sink();

    backend
        .command(start_command(session, turn_id), events)
        .await
        .unwrap();
    consume_permit(&driver.start_seen, "fatal turn start").await;
    sender
        .send(Err(AgentError::other("safe provider failure")))
        .unwrap();
    drop(sender);

    let emitted = collect_until_finished(&mut receiver).await;
    assert_eq!(emitted.len(), 3);
    assert!(matches!(
        &emitted[0].kind,
        AgentEventKind::Error { message, retryable: false }
            if message.contains("safe provider failure")
    ));
    assert!(matches!(emitted[1].kind, AgentEventKind::DiffInvalidated));
    assert!(matches!(
        emitted[2].kind,
        AgentEventKind::TurnFinished { interrupted: false }
    ));
    assert_eq!(driver.finishes.lock().unwrap().len(), 1);
    assert!(!driver.finishes.lock().unwrap()[0].interrupted);
}
