use zode_node_protocol::{
    AgentQuery, AgentSnapshot, NodeId, RuntimeOptions, SandboxMode, SessionLocator,
};

#[test]
fn session_runtime_options_snapshots_carry_the_addressed_session() {
    let session = SessionLocator::new(NodeId::new(), "session-a");
    let options = RuntimeOptions {
        models: vec!["model-a".into()],
        active_model: Some("model-a".into()),
        effort: Some("high".into()),
        sandbox_mode: SandboxMode::ReadOnly,
        sandbox_network: true,
    };

    assert_eq!(
        AgentQuery::SessionRuntimeOptions {
            session: session.clone(),
        },
        AgentQuery::SessionRuntimeOptions {
            session: session.clone(),
        }
    );
    assert_eq!(
        AgentSnapshot::SessionRuntimeOptions {
            session: session.clone(),
            options: options.clone(),
        },
        AgentSnapshot::SessionRuntimeOptions { session, options }
    );
}
