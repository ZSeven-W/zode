use zode_app_model::{AppCommand, ProviderModelsStatus};
use zode_node_protocol::{AgentCommandKind, ApprovalMode, RuntimeOptions, SandboxMode};

use super::{empty_fixture, wait_for_commands, wait_for_result, FakeEndpoint};
use crate::command_bridge::{prepare_dispatch, CommandBridge};

#[tokio::test]
async fn provider_reload_without_a_session_refreshes_the_composer_catalog() {
    let canonical = RuntimeOptions {
        models: vec!["claude-sonnet".into(), "gpt-codex".into()],
        active_model: Some("gpt-codex".into()),
        effort: Some("high".into()),
        approval_mode: ApprovalMode::Full,
        sandbox_mode: SandboxMode::WorkspaceWrite,
        sandbox_network: true,
    };
    let endpoint = FakeEndpoint::success_with_runtime(canonical.clone());
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    state.current_session = None;
    state.composer.model = None;
    state.composer.available_models.clear();

    let dispatch = prepare_dispatch(&mut state, AppCommand::ReloadProviderConfiguration)
        .unwrap()
        .unwrap();
    assert!(matches!(
        dispatch.commands[0].kind,
        AgentCommandKind::ReloadProviderConfiguration
    ));
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(state.composer.available_models, canonical.models);
    assert_eq!(state.composer.model, canonical.active_model);
    assert_eq!(state.composer.effort, canonical.effort);
    assert_eq!(state.composer.sandbox_label, "完全访问");
}

#[tokio::test]
async fn provider_reload_failure_stays_on_settings_state_without_a_fake_task() {
    let endpoint = FakeEndpoint::failing_at(0);
    let mut bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = empty_fixture();
    state.current_session = None;
    state.provider_models.status = ProviderModelsStatus::Saved {
        provider_id: "openai".into(),
    };
    let thread_count = state.threads.len();

    let dispatch = prepare_dispatch(&mut state, AppCommand::ReloadProviderConfiguration)
        .unwrap()
        .unwrap();
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;
    wait_for_result(&mut bridge, &mut state).await;

    assert_eq!(state.threads.len(), thread_count);
    assert!(state.current_session.is_none());
    assert!(matches!(
        state.provider_models.status,
        ProviderModelsStatus::Failed {
            provider_id: Some(ref provider_id),
            ref message,
        } if provider_id == "openai" && message.contains("offline")
    ));
}
