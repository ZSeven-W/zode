use accesskit::{Action, NodeId, Role};
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    reduce_settings_command, AppCommand, SettingsCategory, SettingsCommandOutcome, ShellRoute,
};
use zode_app_ui::{
    accessibility_tree, Insets, RectExt, SettingsPanel, WorkspaceSnapshot, SETTINGS_ROOT_ID,
};
use zode_node_protocol::{
    NodeId as AgentNodeId, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri,
};

#[test]
fn settings_nodes_expose_current_toggle_state_and_full_visual_hit_width() {
    let mut state = zode_app_model::demo_state();
    state.shell.page = zode_app_model::ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::Appearance);
    state.ui_preferences.theme = zode_app_model::ThemePreference::Dark;
    state.ui_preferences.reduced_motion = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let update = accessibility_tree(&snapshot, 1.0);

    let toggled = |id: zode_app_ui::WidgetId| {
        update
            .nodes
            .iter()
            .find(|(candidate, _)| *candidate == NodeId(id.0))
            .and_then(|(_, node)| node.toggled())
    };
    assert_eq!(
        toggled(zode_app_ui::THEME_DARK_ID),
        Some(accesskit::Toggled::True),
    );
    assert_eq!(
        toggled(zode_app_ui::THEME_SYSTEM_ID),
        Some(accesskit::Toggled::False),
    );
    assert_eq!(
        toggled(zode_app_ui::REDUCED_MOTION_ID),
        Some(accesskit::Toggled::True),
    );

    let dark = snapshot.node(zode_app_ui::THEME_DARK_ID).unwrap();
    let visual_toggle_edge = Point2D::new(
        snapshot.layout.transcript.max_x() - 19.0,
        dark.rect.origin.y + dark.rect.size.y / 2.0,
    );
    assert_eq!(
        snapshot.hit_test(visual_toggle_edge),
        Some(zode_app_ui::THEME_DARK_ID),
    );
}

#[test]
fn settings_scroll_view_exposes_accessibility_scroll_actions() {
    let mut state = zode_app_model::demo_state();
    let conversation = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(
        conversation
            .node(zode_app_ui::SETTINGS_NAV_ID)
            .unwrap()
            .role,
        Role::Button
    );

    state.shell.page = zode_app_model::ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let settings = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert!(settings.node(zode_app_ui::SETTINGS_NAV_ID).is_none());
    let scroll_view = settings.node(SETTINGS_ROOT_ID).unwrap();
    assert_eq!(scroll_view.role, Role::ScrollView);
    assert_eq!(scroll_view.rect, settings.layout.transcript);
    assert!(scroll_view.actions.contains(&Action::ScrollUp));
    assert!(scroll_view.actions.contains(&Action::ScrollDown));
    assert_ne!(zode_app_ui::SETTINGS_NAV_ID, SETTINGS_ROOT_ID);
}

#[test]
fn project_permission_revoke_uses_shared_visible_action_geometry() {
    let (mut state, session) = transcript_fixture();
    state.shell.page = zode_app_model::ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::Permissions);
    let workspace = state
        .threads
        .iter()
        .find(|thread| thread.session == session)
        .unwrap()
        .workspace_uri
        .clone();
    state
        .project_permissions
        .insert(workspace.clone(), vec!["write_file".into()]);
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let row = SettingsPanel::permission_row_layout(snapshot.layout.transcript, &state, &workspace)
        .pop()
        .unwrap();
    let node = snapshot.node(row.id).expect("revoke button semantic node");

    assert_eq!(node.rect, row.rect);
    assert_eq!(node.role, Role::Button);
    assert!(node.actions.contains(&Action::Click));
    assert_eq!(snapshot.hit_test(rect_center(row.rect)), Some(row.id));
    assert_eq!(
        SettingsPanel::command_for_widget(&state, row.id),
        Some(row.revoke_command)
    );
}

#[test]
fn zero_thread_startup_project_permissions_remain_visible_and_actionable() {
    let mut state = zode_app_model::demo_state();
    state.shell.page = zode_app_model::ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::Permissions);
    let workspace = WorkspaceUri::new("file:///repo/startup").unwrap();
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state
        .project_permissions
        .insert(workspace.clone(), vec!["write_file".into()]);
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let id = SettingsPanel::permission_widget_id(&workspace, "write_file");
    let node = snapshot
        .node(id)
        .expect("startup permission has a revoke control");

    assert_eq!(snapshot.hit_test(rect_center(node.rect)), Some(id));
    assert_eq!(
        SettingsPanel::command_for_widget(&state, id),
        Some(AppCommand::RevokeProjectPermission {
            workspace_uri: workspace,
            tool: "write_file".into(),
        })
    );
}

#[test]
fn many_permissions_never_expose_controls_beyond_a_480px_root() {
    let (mut state, session) = transcript_fixture();
    state.shell.page = zode_app_model::ShellPage::Settings;
    state.presentation.route = ShellRoute::Settings(SettingsCategory::Permissions);
    let workspace = state
        .threads
        .iter()
        .find(|thread| thread.session == session)
        .unwrap()
        .workspace_uri
        .clone();
    state.project_permissions.insert(
        workspace,
        (0..40).map(|index| format!("tool-{index}")).collect(),
    );
    let snapshot = WorkspaceSnapshot::build(&state, 900.0, 480.0, Insets::ZERO);
    assert!(snapshot
        .nodes
        .iter()
        .all(|node| node.rect.max_y() <= snapshot.layout.viewport.max_y()));

    let command = SettingsPanel::scroll_command(snapshot.layout.transcript, &state, 10_000.0);
    assert_eq!(
        reduce_settings_command(&mut state, command),
        SettingsCommandOutcome::Applied
    );
    let scrolled = WorkspaceSnapshot::build(&state, 900.0, 480.0, Insets::ZERO);
    let focused = scrolled
        .focused
        .expect("scrolled settings keeps a visible focus target");
    assert!(scrolled.node(focused).is_some());
    assert_ne!(focused, zode_app_ui::THEME_SYSTEM_ID);
    let last = scrolled
        .nodes
        .iter()
        .find(|node| node.name == "撤销 tool-39 权限")
        .expect("the final permission is reachable by scrolling");
    assert!(last.rect.max_y() <= scrolled.layout.transcript.max_y());
    assert_eq!(scrolled.hit_test(rect_center(last.rect)), Some(last.id));
    assert_eq!(
        SettingsPanel::command_for_widget(&state, last.id),
        Some(AppCommand::RevokeProjectPermission {
            workspace_uri: SettingsPanel::active_workspace_uri(&state).unwrap().clone(),
            tool: "tool-39".into(),
        })
    );
}

fn transcript_fixture() -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = zode_app_model::demo_state();
    let workspace_uri = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(AgentNodeId::new(), "settings-session");
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace_uri.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri,
        title: "Settings".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.current_session = Some(session.clone());
    (state, session)
}

fn rect_center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}
