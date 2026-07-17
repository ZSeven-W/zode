use zode_app_model::{integration_catalog, IntegrationMutationState, LoadState};
use zode_node_protocol::{
    IntegrationRegistryEntry, IntegrationRegistryKind, IntegrationRegistrySnapshot,
    IntegrationRegistryState,
};

use super::*;

#[tokio::test]
async fn integration_toggle_round_trips_through_the_endpoint_registry() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    install_git_catalog(&mut state, IntegrationRegistryState::Ready);
    let workspace_uri = state.threads[0].workspace_uri.clone();
    let dispatch = prepare_dispatch(
        &mut state,
        AppCommand::SetIntegrationEnabled {
            workspace_uri: workspace_uri.clone(),
            source_id: "tools:git".into(),
            enabled: false,
        },
    )
    .unwrap()
    .unwrap();
    assert!(matches!(
        state.presentation.integration_mutation,
        IntegrationMutationState::Updating { .. }
    ));
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(matches!(
        endpoint.commands.lock().unwrap()[0].kind,
        AgentCommandKind::SetIntegrationEnabled { enabled: false, .. }
    ));
    assert_eq!(
        state
            .presentation
            .integrations
            .ready()
            .unwrap()
            .all_entries()
            .next()
            .unwrap()
            .availability,
        zode_app_model::Availability::Disabled
    );
    assert_eq!(
        state.presentation.integration_mutation,
        IntegrationMutationState::Idle
    );
}

#[tokio::test]
async fn integration_toggle_failure_stays_visible_and_retryable() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = fixture();
    install_git_catalog(&mut state, IntegrationRegistryState::Ready);
    let workspace_uri = state.threads[0].workspace_uri.clone();
    let dispatch = prepare_dispatch(
        &mut state,
        AppCommand::SetIntegrationEnabled {
            workspace_uri,
            source_id: "tools:git".into(),
            enabled: false,
        },
    )
    .unwrap()
    .unwrap();
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert!(matches!(
        &state.presentation.integration_mutation,
        IntegrationMutationState::Failed { source_id, message }
            if source_id == "tools:git" && message.contains("offline")
    ));
}

fn install_git_catalog(
    state: &mut zode_app_model::ZodeAppState,
    registry_state: IntegrationRegistryState,
) {
    let workspace_uri = state.threads[0].workspace_uri.clone();
    state.presentation.integrations =
        LoadState::Ready(integration_catalog(IntegrationRegistrySnapshot {
            workspace_uri,
            entries: vec![IntegrationRegistryEntry {
                source_id: "tools:git".into(),
                name: "git".into(),
                description: "Git tools".into(),
                kind: IntegrationRegistryKind::ToolGroup,
                state: registry_state,
                installed: true,
            }],
            directory_error: Some("directory unavailable".into()),
        }));
}
