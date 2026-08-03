use jian_widgets::Rect;
use zode_app_model::{
    demo_state, AppCommand, LoadState, PluginDetailState, PluginUpdateState, ZodeAppState,
};
use zode_app_ui::{
    IntegrationsPage, PluginDetailBody, PLUGIN_DETAIL_APPLY_UPDATE_ID,
    PLUGIN_DETAIL_CHECK_UPDATE_ID,
};
use zode_node_protocol::{InstalledPluginSummary, PluginTrustState, PluginUpdateAvailable};

fn available() -> PluginUpdateAvailable {
    PluginUpdateAvailable {
        summary: "abc1234 -> def5678".into(),
        target_commit: "def5678abcdef".into(),
    }
}

fn state_with(update: PluginUpdateState) -> ZodeAppState {
    let mut state = demo_state();
    state.presentation.installed_plugins = LoadState::Ready(vec![InstalledPluginSummary {
        id: "acme__tools".into(),
        repo: "acme/tools".into(),
        reference: "main".into(),
        installed_at_ms: 0,
        capabilities: Vec::new(),
        trust: PluginTrustState::Trusted,
    }]);
    let mut detail = PluginDetailState::overview("acme__tools");
    detail.update = update;
    state.presentation.plugin_detail = Some(detail);
    state
}

fn overview(state: &ZodeAppState) -> (Vec<String>, bool) {
    let layout = IntegrationsPage::plugin_detail_layout(Rect::xywh(0.0, 0.0, 900.0, 700.0), state)
        .expect("the detail overlay lays out");
    match layout.body {
        PluginDetailBody::Overview { update, .. } => (
            vec![update.check_label.clone(), update.apply_label.clone()],
            update.apply.is_some(),
        ),
        other => panic!("expected the overview body, got {other:?}"),
    }
}

#[test]
fn no_apply_button_exists_before_a_check_finds_an_update() {
    let state = state_with(PluginUpdateState::Idle);
    let (labels, has_apply) = overview(&state);
    assert_eq!(labels[0], "检查更新");
    assert!(!has_apply);
    assert_eq!(
        IntegrationsPage::command_for_widget(&state, PLUGIN_DETAIL_APPLY_UPDATE_ID),
        None,
    );
}

#[test]
fn a_pending_update_paints_an_apply_button_that_dispatches_the_command() {
    let state = state_with(PluginUpdateState::Available(available()));
    let (labels, has_apply) = overview(&state);
    assert!(has_apply);
    assert_eq!(labels[1], "更新");
    assert_eq!(
        IntegrationsPage::command_for_widget(&state, PLUGIN_DETAIL_APPLY_UPDATE_ID),
        Some(AppCommand::ApplyPluginUpdate),
    );
    assert_eq!(
        IntegrationsPage::command_for_widget(&state, PLUGIN_DETAIL_CHECK_UPDATE_ID),
        Some(AppCommand::CheckPluginUpdate),
    );
}

#[test]
fn both_buttons_go_inert_while_git_is_running() {
    for busy in [
        PluginUpdateState::Checking,
        PluginUpdateState::Applying(available()),
    ] {
        let state = state_with(busy.clone());
        assert_eq!(
            IntegrationsPage::command_for_widget(&state, PLUGIN_DETAIL_CHECK_UPDATE_ID),
            None,
            "check should be inert in {busy:?}",
        );
        assert_eq!(
            IntegrationsPage::command_for_widget(&state, PLUGIN_DETAIL_APPLY_UPDATE_ID),
            None,
            "apply should be inert in {busy:?}",
        );
    }
}

#[test]
fn the_footer_reports_every_settled_check_outcome() {
    let cases = [
        (PluginUpdateState::Checking, "正在检查更新…", "检查中…"),
        (PluginUpdateState::UpToDate, "已是最新版本", "检查更新"),
        (
            PluginUpdateState::Available(available()),
            "发现更新：abc1234 -> def5678",
            "检查更新",
        ),
        (
            PluginUpdateState::CheckFailed("未检测到 git".into()),
            "检查更新失败：未检测到 git",
            "检查更新",
        ),
    ];
    for (update, expected_status, expected_check_label) in cases {
        let state = state_with(update.clone());
        let layout =
            IntegrationsPage::plugin_detail_layout(Rect::xywh(0.0, 0.0, 900.0, 700.0), &state)
                .expect("the detail overlay lays out");
        let PluginDetailBody::Overview {
            update: controls, ..
        } = layout.body
        else {
            panic!("expected the overview body");
        };
        let status = controls.status.expect("a settled state reports a status");
        assert_eq!(status.text, expected_status, "for {update:?}");
        assert_eq!(
            status.error,
            matches!(update, PluginUpdateState::CheckFailed(_))
        );
        assert_eq!(controls.check_label, expected_check_label);
    }
}
