use zode_node_protocol::{
    AgentQuery, AgentSnapshot, IntegrationRegistryEntry, IntegrationRegistryKind,
    IntegrationRegistrySnapshot, IntegrationRegistryState, WorkspaceUri,
};

#[test]
fn integration_queries_are_workspace_scoped_and_fully_typed() {
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    let snapshot = IntegrationRegistrySnapshot {
        workspace_uri: workspace_uri.clone(),
        entries: vec![IntegrationRegistryEntry {
            source_id: "tools:git".into(),
            name: "git".into(),
            description: "Inspect and operate on the git repository".into(),
            kind: IntegrationRegistryKind::ToolGroup,
            state: IntegrationRegistryState::Ready,
            installed: true,
        }],
        directory_error: Some("online catalog is unavailable".into()),
    };

    assert_eq!(
        AgentQuery::Integrations {
            workspace_uri: workspace_uri.clone(),
        },
        AgentQuery::Integrations {
            workspace_uri: workspace_uri.clone(),
        }
    );
    assert_eq!(
        AgentSnapshot::Integrations(snapshot.clone()),
        AgentSnapshot::Integrations(snapshot)
    );
}
