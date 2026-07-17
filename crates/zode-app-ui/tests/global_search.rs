use accesskit::{Action, NodeId, Role};
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    demo_state, AppCommand, ProjectState, SettingsCategory, ShellRoute, ZodeAppState,
};
use zode_app_ui::{
    accessibility_tree, GlobalSearch, GlobalSearchTarget, GlobalSearchViewState, Insets,
    ProjectPickerViewState, RectExt, WorkspaceSnapshot, COMPOSER_ID, GLOBAL_SEARCH_INPUT_ID,
    GLOBAL_SEARCH_NEW_TASK_ID, GLOBAL_SEARCH_OPEN_FOLDER_ID, GLOBAL_SEARCH_SCRIM_ID,
    GLOBAL_SEARCH_SETTINGS_ID, GLOBAL_SEARCH_SURFACE_ID,
};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

fn workspace(value: &str) -> WorkspaceUri {
    WorkspaceUri::new(value).unwrap()
}

fn state_with_threads() -> (ZodeAppState, SessionLocator, SessionLocator) {
    let mut state = demo_state();
    state.projects.clear();
    state.threads.clear();
    state.archived_sessions.clear();
    state.current_session = None;
    state.presentation.route = ShellRoute::Conversation;

    let zode = workspace("file:///repo/zode");
    let openpencil = workspace("file:///repo/openpencil");
    state.projects.push(ProjectState {
        workspace_uri: zode.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 10,
    });
    state.active_workspace = Some(zode.clone());

    let older = SessionLocator::new(state.host.node_id, "older-task");
    let newer = SessionLocator::new(state.host.node_id, "newer-task");
    let archived = SessionLocator::new(state.host.node_id, "archived-task");
    state.threads.extend([
        ThreadSummary {
            session: older.clone(),
            workspace_uri: zode,
            title: "修复全局搜索".into(),
            updated_at_ms: 20,
            status: ThreadStatus::Idle,
        },
        ThreadSummary {
            session: newer.clone(),
            workspace_uri: openpencil.clone(),
            title: "Global Search polish".into(),
            updated_at_ms: 30,
            status: ThreadStatus::Idle,
        },
        ThreadSummary {
            session: archived.clone(),
            workspace_uri: openpencil,
            title: "Global Search archived".into(),
            updated_at_ms: 40,
            status: ThreadStatus::Idle,
        },
    ]);
    state.archived_sessions.insert(archived);
    (state, older, newer)
}

fn center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}

#[test]
fn choices_match_title_workspace_and_session_and_exclude_archived_tasks() {
    let (state, older, newer) = state_with_threads();

    let all = GlobalSearch::choices(&state, "");
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].target, GlobalSearchTarget::Thread(newer.clone()));
    assert_eq!(all[1].target, GlobalSearchTarget::Thread(older.clone()));
    assert_eq!(all[0].detail.as_deref(), Some("openpencil"));

    assert_eq!(
        GlobalSearch::choices(&state, "GLOBAL")[0].target,
        GlobalSearchTarget::Thread(newer.clone())
    );
    assert_eq!(
        GlobalSearch::choices(&state, "zode")[0].target,
        GlobalSearchTarget::Thread(older.clone())
    );
    assert_eq!(
        GlobalSearch::choices(&state, "older-task")[0].target,
        GlobalSearchTarget::Thread(older)
    );
    assert!(GlobalSearch::choices(&state, "archived").is_empty());
    assert_eq!(
        GlobalSearch::thread_widget_id(&newer),
        GlobalSearch::thread_widget_id(&newer)
    );
}

#[test]
fn layout_is_centered_over_the_true_viewport_and_clamps_on_narrow_windows() {
    let (state, _, _) = state_with_threads();
    let view = GlobalSearchViewState {
        open: true,
        query: String::new(),
        active_index: 0,
    };
    let wide_viewport = Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0);
    let wide = GlobalSearch::layout(wide_viewport, &state, &view).unwrap();
    assert_eq!(wide.scrim, wide_viewport);
    assert_eq!(wide.surface.size.x, 560.0);
    assert_eq!(wide.surface.origin.x, 620.0);
    assert_eq!(wide.surface.origin.y, (1_080.0 - wide.surface.size.y) / 2.0);

    let narrow_viewport = Rect::xywh(7.0, 11.0, 400.0, 720.0);
    let narrow = GlobalSearch::layout(narrow_viewport, &state, &view).unwrap();
    assert_eq!(narrow.scrim, narrow_viewport);
    assert_eq!(narrow.surface.size.x, 352.0);
    assert_eq!(narrow.surface.origin.x, 31.0);
    assert_eq!(
        narrow.surface.origin.y,
        11.0 + (720.0 - narrow.surface.size.y) / 2.0
    );
}

#[test]
fn active_and_pointer_targets_map_to_real_app_commands() {
    let (state, _, newer) = state_with_threads();
    let choices = GlobalSearch::choices(&state, "");
    let mut view = GlobalSearchViewState {
        open: true,
        query: String::new(),
        active_index: 0,
    };
    assert_eq!(
        GlobalSearch::command_for_active(
            &state,
            &view,
            &GlobalSearch::layout(Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0), &state, &view).unwrap(),
        ),
        Some(AppCommand::SelectSession(newer))
    );
    view.active_index = choices.len();
    let layout =
        GlobalSearch::layout(Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0), &state, &view).unwrap();
    assert_eq!(
        GlobalSearch::command_for_active(&state, &view, &layout),
        Some(AppCommand::BeginTask {
            workspace_uri: state.active_workspace.clone(),
        })
    );
    view.active_index += 1;
    let layout =
        GlobalSearch::layout(Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0), &state, &view).unwrap();
    assert_eq!(
        GlobalSearch::command_for_active(&state, &view, &layout),
        Some(AppCommand::CreateProject)
    );
    view.active_index += 1;
    let layout =
        GlobalSearch::layout(Rect::xywh(0.0, 0.0, 1_800.0, 1_080.0), &state, &view).unwrap();
    assert_eq!(
        GlobalSearch::command_for_active(&state, &view, &layout),
        Some(AppCommand::Navigate(ShellRoute::Settings(
            SettingsCategory::General,
        )))
    );
    assert_eq!(
        GlobalSearch::command_for_widget(&state, GLOBAL_SEARCH_SCRIM_ID),
        Some(AppCommand::CloseGlobalSearch)
    );
}

#[test]
fn accessibility_snapshot_keeps_search_focused_and_overlay_last_in_hit_order() {
    let (mut state, older, newer) = state_with_threads();
    state.global_search.open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let view = GlobalSearchViewState {
        open: true,
        query: String::new(),
        active_index: 0,
    };
    let layout = GlobalSearch::layout(snapshot.layout.viewport, &state, &view).unwrap();

    assert_eq!(snapshot.focused, Some(GLOBAL_SEARCH_INPUT_ID));
    assert_eq!(
        snapshot.node(GLOBAL_SEARCH_SURFACE_ID).unwrap().role,
        Role::Dialog
    );
    let input = snapshot.node(GLOBAL_SEARCH_INPUT_ID).unwrap();
    assert_eq!(input.role, Role::SearchInput);
    assert_eq!(input.name, "搜索任务或运行命令");
    assert!(input.actions.contains(&Action::SetValue));

    let thread_id = GlobalSearch::thread_widget_id(&newer);
    let thread = snapshot.node(thread_id).expect("matching task row");
    assert_eq!(thread.role, Role::MenuItem);
    assert!(thread.actions.contains(&Action::Focus));
    assert!(thread.focus_order.is_some());
    assert_eq!(snapshot.hit_test(center(thread.rect)), Some(thread_id));
    for id in [
        GLOBAL_SEARCH_NEW_TASK_ID,
        GLOBAL_SEARCH_OPEN_FOLDER_ID,
        GLOBAL_SEARCH_SETTINGS_ID,
    ] {
        let row = snapshot.node(id).expect("command row");
        assert!(row.actions.contains(&Action::Focus));
        assert!(row.focus_order.is_some());
        assert_eq!(snapshot.hit_test(center(row.rect)), Some(id));
    }
    assert_eq!(
        snapshot.focusable_ids(),
        [
            GLOBAL_SEARCH_INPUT_ID,
            thread_id,
            GlobalSearch::thread_widget_id(&older),
            GLOBAL_SEARCH_NEW_TASK_ID,
            GLOBAL_SEARCH_OPEN_FOLDER_ID,
            GLOBAL_SEARCH_SETTINGS_ID,
        ]
    );
    assert!(snapshot.node(COMPOSER_ID).is_some());
    assert_eq!(
        snapshot.hit_test(Point2D::new(
            layout.scrim.origin.x + 4.0,
            layout.scrim.origin.y + 4.0
        )),
        Some(GLOBAL_SEARCH_SCRIM_ID)
    );
    assert!(
        snapshot
            .nodes
            .iter()
            .position(|node| node.id == GLOBAL_SEARCH_SCRIM_ID)
            > snapshot
                .nodes
                .iter()
                .position(|node| node.id == zode_app_ui::COMPOSER_ID)
    );
}

#[test]
fn accessibility_tree_isolates_the_modal_search_from_background_content() {
    let (mut state, older, newer) = state_with_threads();
    state.global_search.open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let update = accessibility_tree(&snapshot, 1.0);
    let (_, root) = update.nodes.first().expect("window root");
    let (_, dialog) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == NodeId(GLOBAL_SEARCH_SURFACE_ID.0))
        .expect("modal global search dialog");

    assert_eq!(root.children(), &[NodeId(GLOBAL_SEARCH_SURFACE_ID.0)]);
    assert!(dialog.is_modal());
    assert_eq!(
        dialog.children(),
        &[
            NodeId(GLOBAL_SEARCH_INPUT_ID.0),
            NodeId(GlobalSearch::thread_widget_id(&newer).0),
            NodeId(GlobalSearch::thread_widget_id(&older).0),
            NodeId(GLOBAL_SEARCH_NEW_TASK_ID.0),
            NodeId(GLOBAL_SEARCH_OPEN_FOLDER_ID.0),
            NodeId(GLOBAL_SEARCH_SETTINGS_ID.0),
        ]
    );
    assert!(update
        .nodes
        .iter()
        .all(|(id, _)| *id != NodeId(COMPOSER_ID.0)));
    assert_eq!(update.focus, NodeId(GLOBAL_SEARCH_INPUT_ID.0));
}

#[test]
fn explicit_overlay_view_drives_filtering_without_mutating_model_state() {
    let (state, older, _) = state_with_threads();
    let snapshot = WorkspaceSnapshot::build_with_overlays(
        &state,
        1_800.0,
        1_080.0,
        Insets::ZERO,
        &ProjectPickerViewState::default(),
        &GlobalSearchViewState {
            open: true,
            query: "修复".into(),
            active_index: 0,
        },
    );
    assert!(snapshot
        .node(GlobalSearch::thread_widget_id(&older))
        .is_some());
    assert_eq!(
        snapshot
            .node(GLOBAL_SEARCH_INPUT_ID)
            .unwrap()
            .value
            .as_deref(),
        Some("修复")
    );
}

#[test]
fn minimum_window_uses_only_visible_tasks_and_keeps_every_action_reachable() {
    let (mut state, _, _) = state_with_threads();
    let task_workspace = workspace("file:///repo/zode");
    for index in 0..8 {
        state.threads.push(ThreadSummary {
            session: SessionLocator::new(state.host.node_id, format!("extra-{index}")),
            workspace_uri: task_workspace.clone(),
            title: format!("Extra task {index}"),
            updated_at_ms: 100 + index,
            status: ThreadStatus::Idle,
        });
    }
    assert_eq!(GlobalSearch::choices(&state, "").len(), 6);

    let viewport = Rect::xywh(0.0, 0.0, 300.0, 240.0);
    let mut view = GlobalSearchViewState {
        open: true,
        query: String::new(),
        active_index: usize::MAX,
    };
    let layout = GlobalSearch::layout(viewport, &state, &view).unwrap();
    assert_eq!(layout.thread_rows.len(), 1);
    assert_eq!(GlobalSearch::selectable_ids(&layout).len(), 4);
    assert!(layout.action_rows.iter().all(|row| {
        row.rect.origin.y >= layout.surface.origin.y
            && row.rect.max_y() <= layout.surface.max_y()
            && row.rect.max_y() <= viewport.max_y()
    }));
    assert!(layout.action_rows[2].active);
    assert_eq!(
        GlobalSearch::command_for_active(&state, &view, &layout),
        Some(AppCommand::Navigate(ShellRoute::Settings(
            SettingsCategory::General,
        )))
    );

    view.active_index = 1;
    let layout = GlobalSearch::layout(viewport, &state, &view).unwrap();
    assert!(layout.action_rows[0].active);
    assert_eq!(
        GlobalSearch::command_for_active(&state, &view, &layout),
        Some(AppCommand::BeginTask {
            workspace_uri: state.active_workspace.clone(),
        })
    );
}
