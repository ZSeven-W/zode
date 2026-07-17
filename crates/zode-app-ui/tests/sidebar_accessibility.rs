use jian_widgets::{Point2D, Rect};
use zode_app_model::AppCommand;
use zode_app_ui::{Insets, ProjectSidebar, RectExt, WidgetId, WorkspaceSnapshot};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[test]
fn sidebar_dynamic_rows_share_snapshot_hit_rects_and_keep_stable_ids() {
    let mut state = zode_app_model::demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "session-stable");
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Stable task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    let first = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let rows = ProjectSidebar::dynamic_row_layout(first.layout.sidebar, &state);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let node = first.node(row.id).expect("row is represented in snapshot");
        assert_eq!(node.rect, row.rect);
        assert_eq!(first.hit_test(rect_center(row.rect)), Some(row.id));
    }
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, rows[1].id),
        Some(AppCommand::SelectSession(session.clone())),
    );

    state.threads.insert(
        0,
        ThreadSummary {
            session: SessionLocator::new(state.host.node_id, "newer-session"),
            workspace_uri: WorkspaceUri::new("file:///repo/other").unwrap(),
            title: "Newer".into(),
            updated_at_ms: 100,
            status: ThreadStatus::Idle,
        },
    );
    let second = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let stable_session_id = ProjectSidebar::session_widget_id(&session);
    assert!(first.node(stable_session_id).is_some());
    assert!(second.node(stable_session_id).is_some());
}

#[test]
fn sidebar_includes_projects_without_threads_and_static_rows_share_paint_layout() {
    let mut state = zode_app_model::demo_state();
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: WorkspaceUri::new("file:///repo/empty-project").unwrap(),
        expanded: false,
        available: false,
        last_opened_ms: 10,
    });
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let dynamic = ProjectSidebar::dynamic_row_layout(snapshot.layout.sidebar, &state);
    assert_eq!(dynamic.len(), 1);
    assert!(dynamic[0].label.contains("empty-project"));
    assert!(dynamic[0].label.contains("unavailable"));
    assert!(snapshot.node(dynamic[0].id).is_some());

    let navigation = ProjectSidebar::navigation_row_layout(snapshot.layout.sidebar);
    for (row, id) in navigation.into_iter().zip([
        zode_app_ui::NEW_SESSION_ID,
        zode_app_ui::WORKFLOWS_NAV_ID,
        zode_app_ui::PLUGINS_NAV_ID,
        zode_app_ui::OPENPENCIL_NAV_ID,
        zode_app_ui::BROWSER_NAV_ID,
        WidgetId(7),
    ]) {
        assert_eq!(snapshot.node(id).unwrap().rect, row.rect);
    }
    assert_eq!(zode_app_ui::NEW_SESSION_ID, WidgetId(2));
    assert_eq!(zode_app_ui::WORKFLOWS_NAV_ID, WidgetId(3));
    assert_eq!(zode_app_ui::PLUGINS_NAV_ID, WidgetId(4));
    assert_eq!(zode_app_ui::OPENPENCIL_NAV_ID, WidgetId(5));
    assert_eq!(zode_app_ui::BROWSER_NAV_ID, WidgetId(6));
    assert_eq!(zode_app_ui::SETTINGS_NAV_ID, WidgetId(9));
    assert_eq!(
        snapshot.node(zode_app_ui::SETTINGS_NAV_ID).unwrap().rect,
        ProjectSidebar::profile_rect(snapshot.layout.sidebar)
    );
}

#[test]
fn orphan_workspace_group_is_semantic_but_does_not_claim_an_unavailable_toggle() {
    let mut state = zode_app_model::demo_state();
    let workspace = WorkspaceUri::new("file:///repo/orphan").unwrap();
    state.threads.push(ThreadSummary {
        session: SessionLocator::new(state.host.node_id, "orphan-session"),
        workspace_uri: workspace,
        title: "Orphan task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let rows = ProjectSidebar::dynamic_row_layout(snapshot.layout.sidebar, &state);
    let project = rows
        .iter()
        .find(|row| matches!(row.target, zode_app_ui::SidebarRowTarget::Project(_)))
        .unwrap();
    let project_node = snapshot.node(project.id).unwrap();

    assert!(!project.actionable);
    assert!(project_node.actions.is_empty());
    assert_eq!(project_node.focus_order, None);
    assert_eq!(ProjectSidebar::command_for_widget(&state, project.id), None);
}

#[test]
fn dynamic_sidebar_rows_are_capped_by_available_height() {
    let mut state = zode_app_model::demo_state();
    for index in 0..20 {
        state.projects.push(zode_app_model::ProjectState {
            workspace_uri: WorkspaceUri::new(format!("file:///repo/project-{index}")).unwrap(),
            expanded: false,
            available: true,
            last_opened_ms: index,
        });
    }
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 480.0, Insets::ZERO);
    let rows = ProjectSidebar::dynamic_row_layout(snapshot.layout.sidebar, &state);
    assert!(!rows.is_empty());
    assert!(rows
        .iter()
        .all(|row| row.rect.max_y() <= snapshot.layout.sidebar.max_y()));
    for row in rows {
        assert!(snapshot.node(row.id).is_some());
    }
}

fn rect_center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}
