use accesskit::{Action, NodeId, Role};
use jian_widgets::Rect;
use zode_app_model::{
    demo_state, AppCommand, LoadState, SessionPresentationState, SettingsCategory, ShellPage,
    ShellRoute,
};
use zode_app_ui::{
    accessibility_tree, Insets, RectExt, SettingsPanel, WorkspaceLayout, WorkspaceSnapshot,
};
use zode_node_protocol::{
    NodeId as AgentNodeId, RuntimeOptions, SandboxMode, SessionLocator, TurnId,
};

#[test]
fn settings_general_exposes_the_reference_information_architecture() {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let shell = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        zode_app_ui::Insets::ZERO,
        state.presentation.route,
        None,
    );

    let layout = SettingsPanel::layout(shell.sidebar, shell.primary_surface, &state);

    assert!(layout.navigation.entries.len() >= 15);
    assert_eq!(layout.general.permission_presets.len(), 3);
    assert!(layout.general.general_rows.len() >= 8);
    assert!(layout.general.general_rows.iter().all(|row| {
        (center_y(row.label_rect) - center_y(row.value_rect)).abs() <= f32::EPSILON
    }));
}

#[test]
fn settings_general_matches_the_reference_vertical_rhythm() {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let shell = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        state.presentation.route,
        None,
    );

    let layout = SettingsPanel::layout(shell.sidebar, shell.primary_surface, &state);

    assert_eq!(layout.content, Rect::xywh(636.0, 70.0, 768.0, 1_010.0));
    assert_eq!(
        layout.general.permission_card,
        Rect::xywh(636.0, 174.0, 768.0, 216.0)
    );
    assert_eq!(
        layout.general.general_section_label,
        Rect::xywh(636.0, 436.0, 768.0, 24.0)
    );
    assert_eq!(
        layout.general.general_card,
        Rect::xywh(636.0, 474.0, 768.0, 600.0)
    );
    assert_eq!(layout.general.permission_presets.len(), 3);
    assert!(layout
        .general
        .permission_presets
        .iter()
        .all(|preset| preset.rect.size.y == 72.0));
    assert_eq!(layout.general.general_rows.len(), 10);
    assert!(layout
        .general
        .general_rows
        .iter()
        .all(|row| row.rect.size.y == 60.0));
    assert_eq!(
        layout.general.general_rows.last().unwrap().rect.max_y(),
        1_074.0
    );
    assert_eq!(
        SettingsPanel::max_scroll_offset(layout.content, &state),
        0.0
    );
}

#[test]
fn typed_navigation_is_enabled_while_unavailable_general_rows_stay_disabled() {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let layout = SettingsPanel::layout(
        Rect::xywh(0.0, 0.0, 240.0, 1_080.0),
        Rect::xywh(240.0, 0.0, 1_560.0, 1_080.0),
        &state,
    );

    assert!(layout.navigation.entries.iter().all(|entry| entry.enabled));
    assert!(layout
        .navigation
        .entries
        .iter()
        .all(|entry| entry.command.is_some()));
    assert!(layout
        .general
        .general_rows
        .iter()
        .filter(|row| !row.enabled)
        .all(|row| row.command.is_none()));
    let suggestions = layout
        .general
        .general_rows
        .iter()
        .find(|row| row.label == "建议提示")
        .unwrap();
    assert_eq!(suggestions.toggled, Some(true));
    assert_eq!(
        suggestions.command,
        Some(AppCommand::SetTaskSuggestions(false))
    );
    let sidebar_tasks = layout
        .general
        .general_rows
        .iter()
        .find(|row| row.label == "侧边栏任务列表")
        .unwrap();
    assert_eq!(sidebar_tasks.toggled, Some(true));
    assert_eq!(
        sidebar_tasks.command,
        Some(AppCommand::SetSidebarTasksExpanded(false))
    );
    assert!(layout
        .general
        .permission_presets
        .iter()
        .all(|preset| !preset.enabled));
}

#[test]
fn sandbox_radios_use_session_runtime_state_and_preserve_network() {
    let (mut state, session) = state_with_runtime(SandboxMode::WorkspaceWrite, true);
    let shell = WorkspaceLayout::compute_presentation(
        1_800.0,
        1_080.0,
        Insets::ZERO,
        state.presentation.route,
        None,
    );
    let layout = SettingsPanel::layout(shell.sidebar, shell.primary_surface, &state);
    let workspace_write = layout
        .general
        .permission_presets
        .iter()
        .find(|preset| preset.mode == SandboxMode::WorkspaceWrite)
        .unwrap();
    let read_only = layout
        .general
        .permission_presets
        .iter()
        .find(|preset| preset.mode == SandboxMode::ReadOnly)
        .unwrap();

    assert!(workspace_write.selected);
    assert!(workspace_write.enabled);
    assert_eq!(layout.general.general_rows[6].value, "标准");
    assert!(layout.general.general_rows[6].enabled);
    assert_eq!(
        SettingsPanel::command_for_widget(&state, layout.general.general_rows[6].id),
        Some(AppCommand::SetEffort("high".into()))
    );
    assert_eq!(
        SettingsPanel::command_for_widget(&state, read_only.id),
        Some(AppCommand::SetSandbox {
            mode: SandboxMode::ReadOnly,
            network: true,
        })
    );

    state.active_turns.insert(
        session,
        TurnId::parse("00000000-0000-0000-0000-000000000002").unwrap(),
    );
    let busy = SettingsPanel::layout(shell.sidebar, shell.primary_surface, &state);
    assert!(busy
        .general
        .permission_presets
        .iter()
        .all(|preset| !preset.enabled && preset.command.is_none()));
    assert_eq!(
        SettingsPanel::command_for_widget(&state, read_only.id),
        None
    );
}

#[test]
fn frozen_settings_layout_drives_hit_focus_and_accesskit_semantics() {
    let (state, _) = state_with_runtime(SandboxMode::ReadOnly, false);
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let layout = SettingsPanel::layout(
        snapshot.layout.sidebar,
        snapshot.layout.primary_surface,
        &state,
    );
    let profile = layout
        .navigation
        .entries
        .iter()
        .find(|entry| entry.label == "个人资料")
        .unwrap();
    let profile_node = snapshot.node(profile.id).unwrap();
    assert_eq!(profile_node.rect, profile.rect);
    assert!(!profile_node.disabled);
    assert!(profile_node.actions.contains(&Action::Click));
    assert!(snapshot.focusable_ids().contains(&profile.id));
    assert_eq!(snapshot.hit_test(center(profile.rect)), Some(profile.id));

    let preset = layout
        .general
        .permission_presets
        .iter()
        .find(|preset| preset.mode == SandboxMode::ReadOnly)
        .unwrap();
    let preset_node = snapshot.node(preset.id).unwrap();
    assert_eq!(preset_node.rect, preset.visible_rect.unwrap());
    assert_eq!(preset_node.role, Role::RadioButton);
    assert!(preset_node.actions.contains(&Action::Click));
    assert_eq!(preset_node.toggled, Some(accesskit::Toggled::True));
    assert_eq!(snapshot.hit_test(center(preset.rect)), Some(preset.id));

    let unavailable_row = &layout.general.general_rows[0];
    let unavailable_node = snapshot.node(unavailable_row.id).unwrap();
    assert!(unavailable_node.disabled);
    assert!(unavailable_node.actions.is_empty());
    assert!(!snapshot.focusable_ids().contains(&unavailable_row.id));
    assert_eq!(snapshot.hit_test(center(unavailable_row.rect)), None);

    let update = accessibility_tree(&snapshot, 1.0);
    let profile_accesskit = update
        .nodes
        .iter()
        .find(|(id, _)| *id == NodeId(profile.id.0))
        .unwrap();
    assert!(!profile_accesskit.1.is_disabled());
}

fn state_with_runtime(
    sandbox_mode: SandboxMode,
    sandbox_network: bool,
) -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = demo_state();
    state.shell.page = ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let session = SessionLocator::new(AgentNodeId::new(), "settings-runtime");
    state.current_session = Some(session.clone());
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            runtime_options: LoadState::Ready(RuntimeOptions {
                models: vec!["gpt-5".into()],
                active_model: Some("gpt-5".into()),
                effort: Some("standard".into()),
                sandbox_mode,
                sandbox_network,
            }),
            ..SessionPresentationState::default()
        },
    );
    (state, session)
}

fn center(rect: Rect) -> jian_widgets::Point2D {
    jian_widgets::Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

fn center_y(rect: Rect) -> f32 {
    rect.origin.y + rect.size.y / 2.0
}
