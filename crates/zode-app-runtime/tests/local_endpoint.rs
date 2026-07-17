use async_trait::async_trait;
use futures::StreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::timeout;
use zode_app_runtime::{EventSink, LocalAgentEndpoint, NodeBackend};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentEventKind, AgentQuery, AgentSnapshot,
    EndpointError, EndpointErrorKind, NodeId, RuntimeOptions, SandboxMode, SessionLocator, TurnId,
    UsageSnapshot, UserContent, PROTOCOL_VERSION,
};

const DEADLINE: Duration = Duration::from_secs(2);

fn session(name: &str) -> SessionLocator {
    SessionLocator::new(NodeId::new(), name)
}

fn start_turn(session: SessionLocator, turn_id: Option<TurnId>) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id,
        kind: AgentCommandKind::StartTurn {
            input: vec![UserContent::Text {
                text: "hello".into(),
            }],
        },
    }
}

fn runtime_options() -> RuntimeOptions {
    RuntimeOptions {
        models: vec!["fake-model".into()],
        active_model: Some("fake-model".into()),
        effort: None,
        approval_mode: Default::default(),
        sandbox_mode: SandboxMode::WorkspaceWrite,
        sandbox_network: false,
    }
}

#[derive(Default)]
struct RecordingBackend {
    commands: Mutex<Vec<AgentCommand>>,
    command_calls: AtomicUsize,
}

#[async_trait]
impl NodeBackend for RecordingBackend {
    async fn command(&self, command: AgentCommand, events: EventSink) -> Result<(), EndpointError> {
        self.command_calls.fetch_add(1, Ordering::SeqCst);
        self.commands.lock().unwrap().push(command.clone());

        let turn_id = command.turn_id.expect("fixture command has a turn id");
        events
            .send(
                command.session.clone(),
                turn_id,
                AgentEventKind::TextDelta {
                    delta: "reply".into(),
                },
            )
            .await?;
        events
            .send(
                command.session.clone(),
                turn_id,
                AgentEventKind::Usage {
                    usage: UsageSnapshot {
                        input_tokens: 3,
                        output_tokens: 5,
                        context_used: Some(0.25),
                        cost_usd: None,
                    },
                },
            )
            .await?;
        events
            .send(
                command.session,
                turn_id,
                AgentEventKind::TurnFinished { interrupted: false },
            )
            .await
    }

    async fn query(&self, query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        assert_eq!(query, AgentQuery::RuntimeOptions);
        Ok(AgentSnapshot::RuntimeOptions(runtime_options()))
    }
}

#[tokio::test]
async fn command_query_and_events_round_trip_through_the_actor() {
    let backend = Arc::new(RecordingBackend::default());
    let endpoint = LocalAgentEndpoint::spawn(backend.clone(), 8);
    let mut stream = endpoint.subscribe().await.unwrap();
    let session = session("round-trip");
    let turn_id = TurnId::new();
    let command = start_turn(session.clone(), Some(turn_id));

    endpoint.command(command.clone()).await.unwrap();
    let snapshot = endpoint.query(AgentQuery::RuntimeOptions).await.unwrap();

    assert_eq!(snapshot, AgentSnapshot::RuntimeOptions(runtime_options()));
    assert_eq!(backend.commands.lock().unwrap().as_slice(), &[command]);

    let mut events = Vec::new();
    for _ in 0..3 {
        events.push(
            timeout(DEADLINE, stream.next())
                .await
                .expect("event timed out")
                .expect("event stream closed")
                .expect("event failed"),
        );
    }

    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(matches!(
        &events[0].kind,
        AgentEventKind::TextDelta { delta } if delta == "reply"
    ));
    assert!(matches!(events[1].kind, AgentEventKind::Usage { .. }));
    assert!(matches!(
        events[2].kind,
        AgentEventKind::TurnFinished { interrupted: false }
    ));
    assert!(events
        .iter()
        .all(|event| event.session == session && event.turn_id == turn_id));
}

#[tokio::test]
async fn invalid_commands_are_rejected_before_the_backend() {
    let backend = Arc::new(RecordingBackend::default());
    let endpoint = LocalAgentEndpoint::spawn(backend.clone(), 4);
    let error = endpoint
        .command(start_turn(session("invalid"), None))
        .await
        .unwrap_err();

    assert_eq!(error.kind, EndpointErrorKind::InvalidRequest);
    assert_eq!(backend.command_calls.load(Ordering::SeqCst), 0);
}

struct FailingBackend;

#[async_trait]
impl NodeBackend for FailingBackend {
    async fn command(
        &self,
        _command: AgentCommand,
        _events: EventSink,
    ) -> Result<(), EndpointError> {
        Err(EndpointError {
            kind: EndpointErrorKind::CapabilityDenied,
            message: "command denied".into(),
        })
    }

    async fn query(&self, _query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        Err(EndpointError {
            kind: EndpointErrorKind::NotFound,
            message: "snapshot missing".into(),
        })
    }
}

#[tokio::test]
async fn backend_error_kinds_are_preserved() {
    let endpoint = LocalAgentEndpoint::spawn(Arc::new(FailingBackend), 4);
    let command_error = endpoint
        .command(start_turn(session("errors"), Some(TurnId::new())))
        .await
        .unwrap_err();
    let query_error = endpoint
        .query(AgentQuery::RuntimeOptions)
        .await
        .unwrap_err();

    assert_eq!(command_error.kind, EndpointErrorKind::CapabilityDenied);
    assert_eq!(query_error.kind, EndpointErrorKind::NotFound);
}

#[tokio::test]
async fn subscription_can_only_be_claimed_once() {
    let endpoint = LocalAgentEndpoint::spawn(Arc::new(RecordingBackend::default()), 4);
    let first = endpoint.subscribe().await.unwrap();
    drop(first);

    let error = match endpoint.subscribe().await {
        Ok(_) => panic!("second subscription unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(error.kind, EndpointErrorKind::Busy);
}
