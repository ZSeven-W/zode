use accesskit::Action;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{AppCommand, SecondaryPane};
use zode_app_ui::{
    CollapsedSidebarChrome, GlobalSearchViewState, Insets, PrimarySidebarLayoutOptions,
    ProjectPickerViewState, ProjectSidebar, RectExt, SecondarySidebarLayoutOptions,
    SidebarLayoutOptions, WidgetId, WorkspaceSnapshot, COLLAPSED_SIDEBAR_BACK_ID,
    COLLAPSED_SIDEBAR_FORWARD_ID, ENVIRONMENT_PANEL_ID, HELP_ID, NEW_SESSION_ID, REVIEW_CLOSE_ID,
    SETTINGS_NAV_ID, SIDEBAR_ID, SIDEBAR_PROJECTS_MORE_ID, SIDEBAR_PROJECTS_NEW_ID,
    SIDEBAR_PROJECT_MENU_FINDER_ID, SIDEBAR_PROJECT_MENU_PIN_ID, SIDEBAR_SEARCH_ID,
    SIDEBAR_TOGGLE_ID,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[test]
fn collapsed_sidebar_keeps_four_titlebar_controls_accessible() {
    let mut state = zode_app_model::demo_state();
    state.shell.sidebar_open = false;

    let snapshot = WorkspaceSnapshot::build(&state, 1_221.0, 992.0, Insets::ZERO);
    assert_eq!(snapshot.layout.sidebar.size.x, 0.0);
    let chrome = CollapsedSidebarChrome::layout(snapshot.layout.top_bar);

    let toggle = snapshot
        .node(SIDEBAR_TOGGLE_ID)
        .expect("collapsed sidebar toggle remains accessible");
    assert_eq!(toggle.name, "显示侧边栏");
    assert_eq!(toggle.rect, chrome.toggle);
    assert_eq!(toggle.actions, vec![Action::Click, Action::Focus]);
    assert!(!toggle.disabled);
    assert_eq!(snapshot.hit_test(rect_center(toggle.rect)), Some(toggle.id));

    for (id, rect, name) in [
        (COLLAPSED_SIDEBAR_BACK_ID, chrome.back, "返回"),
        (COLLAPSED_SIDEBAR_FORWARD_ID, chrome.forward, "前进"),
    ] {
        let control = snapshot
            .node(id)
            .expect("disabled history control remains accessible");
        assert_eq!(control.name, name);
        assert_eq!(control.rect, rect);
        assert!(control.disabled);
        assert!(control.actions.is_empty());
        assert_eq!(control.focus_order, None);
        assert_eq!(snapshot.hit_test(rect_center(control.rect)), None);
    }

    let new_task = snapshot
        .node(NEW_SESSION_ID)
        .expect("collapsed new-task control remains accessible");
    assert_eq!(new_task.name, "新建任务");
    assert_eq!(new_task.rect, chrome.new_task);
    assert_eq!(new_task.actions, vec![Action::Click, Action::Focus]);
    assert!(!new_task.disabled);
    assert_eq!(
        snapshot.hit_test(rect_center(new_task.rect)),
        Some(NEW_SESSION_ID)
    );
}

#[test]
fn transitioning_sidebar_clips_hit_targets_to_the_visible_slice() {
    let state = zode_app_model::demo_state();
    let snapshot = WorkspaceSnapshot::build_with_overlays_and_sidebar_options(
        &state,
        1_800.0,
        1_080.0,
        Insets::ZERO,
        &ProjectPickerViewState::default(),
        &GlobalSearchViewState::default(),
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions {
                open: true,
                width: 293.0,
                visibility: 0.25,
            },
            secondary: SecondarySidebarLayoutOptions::default(),
        },
    );
    assert_eq!(snapshot.layout.primary_sidebar_content.width(), 293.0);
    // 293.0 * 0.25 == 73.25, snapped to the nearest whole logical pixel.
    assert_eq!(snapshot.layout.sidebar.width(), 73.0);

    let sidebar_ids = [
        SIDEBAR_ID,
        SIDEBAR_TOGGLE_ID,
        SIDEBAR_SEARCH_ID,
        NEW_SESSION_ID,
        SIDEBAR_PROJECTS_MORE_ID,
        SIDEBAR_PROJECTS_NEW_ID,
        SETTINGS_NAV_ID,
        HELP_ID,
    ];
    for node in snapshot
        .nodes
        .iter()
        .filter(|node| sidebar_ids.contains(&node.id))
    {
        assert!(node.rect.min_x() >= snapshot.layout.sidebar.min_x());
        assert!(node.rect.max_x() <= snapshot.layout.sidebar.max_x());
    }

    let hidden_point = Point2D::new(
        snapshot.layout.sidebar.max_x() + 2.0,
        snapshot.layout.sidebar.min_y() + 100.0,
    );
    assert!(!snapshot
        .hit_test(hidden_point)
        .is_some_and(|id| sidebar_ids.contains(&id)));
}

#[test]
fn transitioning_right_surfaces_clip_hit_and_accessibility_rects() {
    let mut state = zode_app_model::demo_state();
    state.presentation.secondary_pane = Some(SecondaryPane::Review);
    state.presentation.secondary_sidebar_open = true;
    let snapshot = WorkspaceSnapshot::build_with_overlays_and_sidebar_options(
        &state,
        1_800.0,
        1_080.0,
        Insets::ZERO,
        &ProjectPickerViewState::default(),
        &GlobalSearchViewState::default(),
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions::default(),
            secondary: SecondarySidebarLayoutOptions {
                open: true,
                width: 700.0,
                pinned_summary_overlay: false,
                visibility: 0.25,
                pinned_summary_visibility: 0.0,
            },
        },
    );
    assert!(snapshot.node(REVIEW_CLOSE_ID).is_none());
    for node in snapshot
        .nodes
        .iter()
        .filter(|node| node.name.starts_with("预览文件"))
    {
        assert!(node.rect.min_x() >= snapshot.layout.review_panel.min_x());
        assert!(node.rect.max_x() <= snapshot.layout.review_panel.max_x());
    }

    state.presentation.secondary_pane = None;
    state.presentation.secondary_sidebar_open = false;
    state.presentation.pinned_summary_overlay_open = true;
    let summary = WorkspaceSnapshot::build_with_overlays_and_sidebar_options(
        &state,
        1_200.0,
        900.0,
        Insets::ZERO,
        &ProjectPickerViewState::default(),
        &GlobalSearchViewState::default(),
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions::default(),
            secondary: SecondarySidebarLayoutOptions {
                open: false,
                width: 700.0,
                pinned_summary_overlay: true,
                visibility: 0.0,
                pinned_summary_visibility: 0.5,
            },
        },
    );
    let panel = summary
        .node(ENVIRONMENT_PANEL_ID)
        .expect("partially visible summary remains accessible");
    assert!(panel.rect.min_x() >= summary.layout.context_panel.min_x());
    assert!(panel.rect.max_x() <= summary.layout.context_panel.max_x());
}

#[test]
fn zero_width_first_frame_of_right_transition_keeps_the_conversation_surface() {
    let mut state = zode_app_model::demo_state();
    state.presentation.secondary_pane = Some(SecondaryPane::Review);
    state.presentation.secondary_sidebar_open = true;
    let snapshot = WorkspaceSnapshot::build_with_overlays_and_sidebar_options(
        &state,
        1_800.0,
        1_080.0,
        Insets::ZERO,
        &ProjectPickerViewState::default(),
        &GlobalSearchViewState::default(),
        SidebarLayoutOptions {
            primary: PrimarySidebarLayoutOptions::default(),
            secondary: SecondarySidebarLayoutOptions {
                open: true,
                width: 700.0,
                pinned_summary_overlay: false,
                visibility: 0.0,
                pinned_summary_visibility: 0.0,
            },
        },
    );

    assert_eq!(snapshot.layout.review_panel.width(), 0.0);
    assert!(snapshot.layout.secondary_sidebar_content.width() > 0.0);
    assert!(snapshot.node(zode_app_ui::COMPOSER_ID).is_some());
}

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
    let session_row = rows
        .iter()
        .find(|row| row.session() == Some(&session))
        .unwrap();
    assert_eq!(
        first.node(session_row.pin_id.unwrap()).unwrap().name,
        "置顶任务"
    );
    assert_eq!(
        first.node(session_row.archive_id.unwrap()).unwrap().name,
        "归档任务"
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

#[test]
fn project_and_section_actions_and_open_menu_are_real_accessible_hit_targets() {
    let mut state = zode_app_model::demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace.clone(),
        expanded: false,
        available: true,
        last_opened_ms: 1,
    });
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let project = ProjectSidebar::dynamic_row_layout(snapshot.layout.sidebar, &state)
        .into_iter()
        .find(|row| matches!(row.target, zode_app_ui::SidebarRowTarget::Project(_)))
        .unwrap();
    for (id, name) in [
        (project.more_id.unwrap(), "项目菜单"),
        (project.new_id.unwrap(), "在项目中新建任务"),
        (SIDEBAR_PROJECTS_MORE_ID, "项目菜单"),
        (SIDEBAR_PROJECTS_NEW_ID, "新建项目"),
    ] {
        let node = snapshot.node(id).expect("sidebar action is accessible");
        assert_eq!(node.name, name);
        assert_eq!(snapshot.hit_test(rect_center(node.rect)), Some(id));
    }

    state.sidebar.project_menu = Some(workspace);
    let menu_snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    for id in [SIDEBAR_PROJECT_MENU_PIN_ID, SIDEBAR_PROJECT_MENU_FINDER_ID] {
        let node = menu_snapshot
            .node(id)
            .expect("open menu item is accessible");
        assert_eq!(menu_snapshot.hit_test(rect_center(node.rect)), Some(id));
    }
}

fn rect_center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}
