use zode_app_model::{
    demo_state, reduce_presentation_command, AppCommand, LoadState, PluginDetailMode,
    PluginUpdateState, PresentationCommandOutcome, ZodeAppState,
};
use zode_node_protocol::{InstalledPluginSummary, PluginTrustState, PluginUpdateAvailable};

fn state_with_open_detail() -> ZodeAppState {
    let mut state = demo_state();
    state.presentation.installed_plugins = LoadState::Ready(vec![InstalledPluginSummary {
        id: "acme__tools".into(),
        repo: "acme/tools".into(),
        reference: "main".into(),
        installed_at_ms: 0,
        capabilities: Vec::new(),
        trust: PluginTrustState::Trusted,
    }]);
    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::OpenPluginDetail("acme__tools".into()),
        ),
        PresentationCommandOutcome::Applied,
    );
    state
}

fn available() -> PluginUpdateAvailable {
    PluginUpdateAvailable {
        summary: "abc1234 → def5678（2 个提交）".into(),
        target_commit: "def5678abc".into(),
    }
}

#[test]
fn a_freshly_opened_detail_overlay_starts_idle() {
    let state = state_with_open_detail();
    let detail = state.presentation.plugin_detail.as_ref().unwrap();
    assert_eq!(detail.update, PluginUpdateState::Idle);
    assert!(!detail.update.busy());
    assert_eq!(detail.update.pending(), None);
}

#[test]
fn check_update_enters_checking_and_clears_a_stale_notice() {
    let mut state = state_with_open_detail();
    state.presentation.plugin_detail.as_mut().unwrap().notice = Some("删除失败：boom".into());

    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate),
        PresentationCommandOutcome::Applied,
    );
    let detail = state.presentation.plugin_detail.as_ref().unwrap();
    assert_eq!(detail.update, PluginUpdateState::Checking);
    assert_eq!(detail.notice, None);
    assert!(detail.update.busy());
}

#[test]
fn a_second_check_while_one_is_in_flight_is_ignored() {
    let mut state = state_with_open_detail();
    reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate);
    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate),
        PresentationCommandOutcome::Ignored,
    );

    // Same guard while an update is being applied - the apply is dispatched
    // by the desktop command bridge, which parks the overlay in `Applying`.
    state.presentation.plugin_detail.as_mut().unwrap().update =
        PluginUpdateState::Applying(available());
    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate),
        PresentationCommandOutcome::Ignored,
    );
}

#[test]
fn checking_again_is_allowed_from_every_settled_state() {
    for settled in [
        PluginUpdateState::UpToDate,
        PluginUpdateState::Available(available()),
        PluginUpdateState::CheckFailed("git 操作失败".into()),
    ] {
        let mut state = state_with_open_detail();
        state.presentation.plugin_detail.as_mut().unwrap().update = settled.clone();
        assert_eq!(
            reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate),
            PresentationCommandOutcome::Applied,
            "expected {settled:?} to allow a re-check",
        );
        assert_eq!(
            state.presentation.plugin_detail.as_ref().unwrap().update,
            PluginUpdateState::Checking,
        );
    }
}

#[test]
fn check_update_is_ignored_while_another_overlay_mode_is_showing() {
    let mut state = state_with_open_detail();
    reduce_presentation_command(&mut state, AppCommand::RequestUninstallPlugin);
    assert_eq!(
        state.presentation.plugin_detail.as_ref().unwrap().mode,
        PluginDetailMode::ConfirmUninstall,
    );
    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate),
        PresentationCommandOutcome::Ignored,
    );
    assert_eq!(
        state.presentation.plugin_detail.as_ref().unwrap().update,
        PluginUpdateState::Idle,
    );
}

#[test]
fn check_update_without_an_open_overlay_is_ignored() {
    let mut state = demo_state();
    assert_eq!(
        reduce_presentation_command(&mut state, AppCommand::CheckPluginUpdate),
        PresentationCommandOutcome::Ignored,
    );
}

#[test]
fn reopening_the_overlay_restarts_the_update_machine() {
    let mut state = state_with_open_detail();
    state.presentation.plugin_detail.as_mut().unwrap().update =
        PluginUpdateState::Available(available());
    reduce_presentation_command(&mut state, AppCommand::ClosePluginDetail);
    reduce_presentation_command(
        &mut state,
        AppCommand::OpenPluginDetail("acme__tools".into()),
    );
    assert_eq!(
        state.presentation.plugin_detail.as_ref().unwrap().update,
        PluginUpdateState::Idle,
    );
}

#[test]
fn pending_reports_the_update_being_shown_or_applied() {
    assert_eq!(
        PluginUpdateState::Available(available()).pending(),
        Some(&available())
    );
    assert_eq!(
        PluginUpdateState::Applying(available()).pending(),
        Some(&available())
    );
    assert_eq!(PluginUpdateState::UpToDate.pending(), None);
    assert_eq!(
        PluginUpdateState::CheckFailed("boom".into()).pending(),
        None
    );
}
