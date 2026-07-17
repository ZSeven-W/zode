use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
use agent::error::AgentError;
use agent::stream::Event;
use async_trait::async_trait;
use futures::{stream, StreamExt};
use zode_app_runtime::{DriverEventStream, EngineDriver, LocalAppRuntime};
use zode_core::bootstrap::{AppBootstrap, ResolvedBootstrap};
use zode_node_protocol::{
    AgentCommand, AgentCommandKind, AgentEndpoint, AgentEventKind, AgentQuery, AgentSnapshot,
    CapabilityManifest, EndpointError, EndpointErrorKind, NodeCapability, NodeId, SessionLocator,
    TurnId, UserContent, PROTOCOL_VERSION,
};

const ASYNC_DEADLINE: Duration = Duration::from_secs(5);
static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(1);

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "zode-local-runtime-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct FakeDriver;

#[async_trait]
impl EngineDriver for FakeDriver {
    async fn command(&self, _command: AgentCommand) -> Result<(), EndpointError> {
        Ok(())
    }

    async fn start_turn(
        &self,
        _command: AgentCommand,
        _abort: AbortController,
    ) -> DriverEventStream {
        Box::pin(stream::iter([
            Ok::<_, AgentError>(Event::TextDelta {
                delta: "hello".to_string(),
            }),
            Ok(Event::Usage {
                input_tokens: 4,
                output_tokens: 2,
                cache_read: 0,
                cache_create: 0,
            }),
        ]))
    }

    async fn finish_turn(
        &self,
        _session: &SessionLocator,
        _turn_id: TurnId,
        _model: Option<String>,
        _interrupted: bool,
    ) -> Result<(), EndpointError> {
        Ok(())
    }

    async fn query(&self, _query: AgentQuery) -> Result<AgentSnapshot, EndpointError> {
        Err(EndpointError {
            kind: EndpointErrorKind::Internal,
            message: "capabilities must be owned by runtime composition".to_string(),
        })
    }
}

async fn resolved_bootstrap(config_dir: &Path) -> ResolvedBootstrap {
    AppBootstrap::for_test(config_dir.to_path_buf())
        .resolve()
        .await
        .unwrap()
}

async fn local_runtime(config_dir: &Path, event_capacity: usize) -> LocalAppRuntime {
    let bootstrap = resolved_bootstrap(config_dir).await;
    let driver: Arc<dyn EngineDriver> = Arc::new(FakeDriver);
    LocalAppRuntime::with_driver(config_dir, bootstrap, driver, event_capacity).unwrap()
}

async fn capabilities(runtime: &LocalAppRuntime) -> CapabilityManifest {
    match runtime
        .endpoint()
        .query(AgentQuery::Capabilities)
        .await
        .unwrap()
    {
        AgentSnapshot::Capabilities(manifest) => manifest,
        snapshot => panic!("expected capabilities, got {snapshot:?}"),
    }
}

fn start_command(session: SessionLocator, turn_id: TurnId) -> AgentCommand {
    AgentCommand {
        version: PROTOCOL_VERSION,
        session,
        turn_id: Some(turn_id),
        kind: AgentCommandKind::StartTurn {
            input: vec![UserContent::Text {
                text: "run".to_string(),
            }],
        },
    }
}

#[tokio::test]
async fn node_identity_is_stable_across_composed_runtime_restarts() {
    let dir = TestDir::new("stable-node");
    let first = local_runtime(dir.path(), 8).await;
    let first_id = capabilities(&first).await.node_id;
    drop(first);

    let second = local_runtime(dir.path(), 8).await;
    let second_id = capabilities(&second).await.node_id;

    assert_eq!(first_id, second_id);
    assert!(dir.path().join("node.json").is_file());
}

#[tokio::test]
async fn capabilities_use_the_local_node_and_only_enabled_runtime_features() {
    let dir = TestDir::new("capabilities");
    let runtime = local_runtime(dir.path(), 8).await;
    let manifest = capabilities(&runtime).await;
    let persisted: serde_json::Value =
        serde_json::from_slice(&fs::read(dir.path().join("node.json")).unwrap()).unwrap();
    let persisted_node: NodeId = serde_json::from_value(persisted["nodeId"].clone()).unwrap();

    assert_eq!(manifest.node_id, persisted_node);
    assert_eq!(
        manifest.capabilities,
        BTreeSet::from([
            NodeCapability::Agent,
            NodeCapability::Workspace,
            NodeCapability::FileSystem,
            NodeCapability::Terminal,
            NodeCapability::Approval,
        ])
    );
}

#[tokio::test]
async fn composed_endpoint_streams_one_fake_turn_in_protocol_order() {
    let dir = TestDir::new("turn");
    let runtime = local_runtime(dir.path(), 1).await;
    let node_id = capabilities(&runtime).await.node_id;
    let session = SessionLocator::new(node_id, "composed-turn");
    let turn_id = TurnId::new();
    let endpoint = runtime.endpoint();
    let mut events = endpoint.subscribe().await.unwrap();

    endpoint
        .command(start_command(session.clone(), turn_id))
        .await
        .unwrap();

    let mut received = Vec::new();
    loop {
        let event = tokio::time::timeout(ASYNC_DEADLINE, events.next())
            .await
            .expect("timed out waiting for composed runtime event")
            .expect("composed event stream closed")
            .expect("composed event carried an endpoint error");
        let finished = matches!(event.kind, AgentEventKind::TurnFinished { .. });
        received.push(event);
        if finished {
            break;
        }
    }

    assert_eq!(received.len(), 4);
    assert!(matches!(
        &received[0].kind,
        AgentEventKind::TextDelta { delta } if delta == "hello"
    ));
    assert!(matches!(received[1].kind, AgentEventKind::Usage { .. }));
    assert!(matches!(received[2].kind, AgentEventKind::DiffInvalidated));
    assert!(matches!(
        received[3].kind,
        AgentEventKind::TurnFinished { interrupted: false }
    ));
    assert!(received
        .iter()
        .all(|event| event.session == session && event.turn_id == turn_id));
    assert_eq!(
        received
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
}

#[tokio::test]
async fn composed_endpoint_allows_only_one_event_subscription() {
    let dir = TestDir::new("single-subscription");
    let runtime = local_runtime(dir.path(), 8).await;
    let endpoint = runtime.endpoint();
    let _first = endpoint.subscribe().await.unwrap();

    let error = match endpoint.subscribe().await {
        Ok(_) => panic!("a second subscription must be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.kind, EndpointErrorKind::Busy);
}
