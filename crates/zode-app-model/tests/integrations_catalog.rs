use zode_app_model::{integration_catalog, Availability, IntegrationCategory, LoadState};
use zode_node_protocol::{
    IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistrySnapshot,
    IntegrationRegistryState, WorkspaceUri,
};

#[test]
fn production_projection_keeps_sources_and_never_marks_fixture_data() {
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    let snapshot = IntegrationRegistrySnapshot {
        workspace_uri: workspace_uri.clone(),
        entries: vec![
            IntegrationRegistryEntry {
                source_id: "tools:git".into(),
                name: "git".into(),
                description: "Git tools".into(),
                kind: IntegrationRegistryKind::ToolGroup,
                state: IntegrationRegistryState::Ready,
                installed: true,
            },
            IntegrationRegistryEntry {
                source_id: "mcp:github".into(),
                name: "github".into(),
                description: "MCP server".into(),
                kind: IntegrationRegistryKind::Mcp,
                state: IntegrationRegistryState::Configured,
                installed: true,
            },
        ],
        directory_error: Some("network catalog unavailable".into()),
    };

    let catalog = integration_catalog(snapshot);

    assert_eq!(catalog.workspace_uri, workspace_uri);
    assert_eq!(catalog.installed.len(), 2);
    assert_eq!(catalog.sections.len(), 2);
    assert!(catalog.all_entries().all(|entry| entry.source_id.is_some()));
    assert!(catalog.all_entries().all(|entry| !entry.fixture_only));
    assert!(catalog.sections.iter().any(|section| {
        section.category == IntegrationCategory::Mcp
            && section.rows[0].availability == Availability::Configured
    }));
    assert!(matches!(LoadState::Ready(catalog), LoadState::Ready(_)));
}
