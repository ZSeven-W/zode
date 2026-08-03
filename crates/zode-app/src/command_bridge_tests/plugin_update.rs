//! Desktop-side half of the plugin update flow: dispatching the apply, and
//! settling the overlay when the refreshed plugin list (or a failure) comes
//! back. The check half is a presentation query and is covered in
//! `presentation_bridge`'s own tests.

use zode_app_model::{PluginDetailMode, PluginDetailState, PluginUpdateState};
use zode_node_protocol::{InstalledPluginSummary, PluginTrustState, PluginUpdateAvailable};

use super::*;
use crate::command_bridge::plugin_market;

fn available() -> PluginUpdateAvailable {
    PluginUpdateAvailable {
        summary: "abc1234 → def5678（2 个提交）".into(),
        target_commit: "def5678abcdef".into(),
    }
}

fn summary(trust: PluginTrustState) -> InstalledPluginSummary {
    InstalledPluginSummary {
        id: "acme__tools".into(),
        repo: "acme/tools".into(),
        reference: "main".into(),
        installed_at_ms: 0,
        capabilities: Vec::new(),
        trust,
    }
}

fn state_with_pending_update() -> zode_app_model::ZodeAppState {
    let mut state = fixture();
    state.presentation.installed_plugins =
        LoadState::Ready(vec![summary(PluginTrustState::Trusted)]);
    let mut detail = PluginDetailState::overview("acme__tools");
    detail.update = PluginUpdateState::Available(available());
    state.presentation.plugin_detail = Some(detail);
    state
}

fn detail_update(state: &zode_app_model::ZodeAppState) -> PluginUpdateState {
    state
        .presentation
        .plugin_detail
        .as_ref()
        .unwrap()
        .update
        .clone()
}

#[tokio::test]
async fn apply_dispatches_the_update_command_and_parks_the_overlay() {
    let endpoint = FakeEndpoint::success(Vec::new());
    let bridge = CommandBridge::spawn_with_wake(endpoint.clone(), || {});
    let mut state = state_with_pending_update();

    let dispatch = prepare_dispatch(&mut state, AppCommand::ApplyPluginUpdate)
        .unwrap()
        .unwrap();
    assert_eq!(
        detail_update(&state),
        PluginUpdateState::Applying(available())
    );
    bridge.dispatch(dispatch).unwrap();
    wait_for_commands(&endpoint, 1).await;

    assert!(matches!(
        endpoint.commands.lock().unwrap()[0].kind,
        AgentCommandKind::ApplyPluginUpdate { ref plugin_id } if plugin_id == "acme__tools"
    ));
}

#[test]
fn apply_is_refused_without_a_checked_update_or_outside_the_overview() {
    let mut state = fixture();
    state.presentation.installed_plugins =
        LoadState::Ready(vec![summary(PluginTrustState::Trusted)]);
    state.presentation.plugin_detail = Some(PluginDetailState::overview("acme__tools"));
    assert!(plugin_market::prepare_apply_update(&mut state).is_err());

    let mut state = state_with_pending_update();
    state.presentation.plugin_detail.as_mut().unwrap().mode = PluginDetailMode::ConfirmUninstall;
    assert!(plugin_market::prepare_apply_update(&mut state).is_err());

    let mut state = fixture();
    assert!(plugin_market::prepare_apply_update(&mut state).is_err());
}

#[test]
fn a_finished_update_settles_the_overlay_and_reports_a_clean_resync() {
    let mut state = state_with_pending_update();
    plugin_market::prepare_apply_update(&mut state).unwrap();

    plugin_market::apply_success(&mut state, vec![summary(PluginTrustState::Trusted)]);

    let detail = state.presentation.plugin_detail.as_ref().unwrap();
    assert_eq!(detail.update, PluginUpdateState::UpToDate);
    assert_eq!(detail.notice.as_deref(), Some("插件已更新"));
}

#[test]
fn a_finished_update_that_drifted_tells_the_user_to_re_review() {
    let mut state = state_with_pending_update();
    plugin_market::prepare_apply_update(&mut state).unwrap();

    plugin_market::apply_success(
        &mut state,
        vec![summary(PluginTrustState::Drifted(vec![
            "hook:before_tool_use:*:hooks/check.sh".into(),
        ]))],
    );

    let detail = state.presentation.plugin_detail.as_ref().unwrap();
    assert_eq!(detail.update, PluginUpdateState::UpToDate);
    assert_eq!(
        detail.notice.as_deref(),
        Some("插件已更新，变更的能力需重新审查后才能启用")
    );
}

#[test]
fn a_failed_update_restores_the_pending_update_and_surfaces_the_reason() {
    let mut state = state_with_pending_update();
    plugin_market::prepare_apply_update(&mut state).unwrap();

    plugin_market::mark_failure(
        &mut state,
        &AgentCommandKind::ApplyPluginUpdate {
            plugin_id: "acme__tools".into(),
        },
        "Git 操作失败：boom",
    );

    let detail = state.presentation.plugin_detail.as_ref().unwrap();
    // The core update rolled the checkout back, so the same update is still
    // pending and the button is live again.
    assert_eq!(detail.update, PluginUpdateState::Available(available()));
    assert_eq!(
        detail.notice.as_deref(),
        Some("更新失败：Git 操作失败：boom")
    );
}

#[test]
fn an_unrelated_plugin_refresh_leaves_a_settled_update_alone() {
    let mut state = state_with_pending_update();
    plugin_market::apply_success(&mut state, vec![summary(PluginTrustState::Trusted)]);
    // No apply was in flight, so the pending update survives the refresh.
    assert_eq!(
        detail_update(&state),
        PluginUpdateState::Available(available())
    );
    assert_eq!(
        state
            .presentation
            .plugin_detail
            .as_ref()
            .unwrap()
            .notice
            .as_deref(),
        None
    );
}
