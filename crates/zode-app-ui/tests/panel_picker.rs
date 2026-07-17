use std::collections::BTreeSet;

use zode_app_model::{
    demo_state, AppCommand, PreviewKind, PreviewState, PreviewTarget, ProjectState, SecondaryPane,
    TranscriptState,
};
use zode_app_ui::{
    Insets, PanelPicker, RectExt, ThreadHeader, WorkspaceSnapshot, EMPTY_SUGGESTION_IDS,
    PANEL_PICKER_ID, SECONDARY_HOME_BROWSER_ID, SECONDARY_HOME_FILES_ID, SECONDARY_HOME_REVIEW_ID,
    SECONDARY_HOME_SIDE_TASK_ID, SECONDARY_HOME_TERMINAL_ID, TERMINAL_ID,
    TERMINAL_SECONDARY_CLOSE_ID,
};
use zode_node_protocol::{
    NodeCapability, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri,
};

fn state_with_session() -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///tmp/zode-panel-picker").unwrap();
    let session = SessionLocator::new(state.host.node_id, "panel-session");
    state.host.capabilities.capabilities = BTreeSet::from([
        NodeCapability::Workspace,
        NodeCapability::FileSystem,
        NodeCapability::Terminal,
    ]);
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 0,
    });
    state.active_workspace = Some(workspace.clone());
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace.clone(),
        title: "panel task".into(),
        updated_at_ms: 0,
        status: ThreadStatus::Idle,
    });
    state
        .transcripts
        .insert(session.clone(), TranscriptState::default());
    state.current_session = Some(session.clone());
    state
        .presentation
        .sessions
        .entry(session)
        .or_default()
        .preview = PreviewState::Ready {
        target: PreviewTarget {
            workspace_uri: workspace,
            relative_path: "README.md".into(),
        },
        title: "README.md".into(),
        content: "real preview".into(),
        kind: PreviewKind::Markdown,
    };
    state
}

#[test]
fn header_exposes_one_unified_panel_picker() {
    let state = state_with_session();
    let rect = jian_widgets::Rect::xywh(240.0, 0.0, 1_560.0, 46.0);
    let picker = ThreadHeader::layout(rect, &state)
        .panel_picker
        .expect("panel picker");
    assert_eq!(picker.id, PANEL_PICKER_ID);
    assert_eq!(
        ThreadHeader::command_for_widget(&state, picker.id),
        Some(AppCommand::ToggleSidebar)
    );
}

#[test]
fn open_sidebar_without_a_selected_pane_exposes_the_picker_home() {
    let mut state = state_with_session();
    state.presentation.secondary_sidebar_open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let home = PanelPicker::home_layout(snapshot.layout.review_panel, &state).expect("picker home");
    assert_eq!(
        home.items.iter().map(|item| item.label).collect::<Vec<_>>(),
        vec!["审阅", "终端", "浏览器", "文件", "侧边任务"]
    );
    for (id, pane) in [
        (SECONDARY_HOME_REVIEW_ID, SecondaryPane::Review),
        (SECONDARY_HOME_TERMINAL_ID, SecondaryPane::Terminal),
    ] {
        let node = snapshot.node(id).expect("home item accessibility node");
        assert!(!node.disabled);
        assert!(node.actions.contains(&accesskit::Action::Click));
        assert_eq!(
            snapshot.hit_test(jian_widgets::Point2D::new(
                node.rect.origin.x + 4.0,
                node.rect.origin.y + 4.0,
            )),
            Some(id)
        );
        assert_eq!(
            PanelPicker::command_for_widget(&state, id),
            Some(AppCommand::OpenSecondary(pane))
        );
        assert!(!EMPTY_SUGGESTION_IDS.contains(&id));
    }
    for id in [
        SECONDARY_HOME_BROWSER_ID,
        SECONDARY_HOME_FILES_ID,
        SECONDARY_HOME_SIDE_TASK_ID,
    ] {
        let node = snapshot.node(id).expect("disabled home item node");
        assert!(node.disabled);
        assert!(node.actions.is_empty());
        assert!(node.value.is_some());
        assert_eq!(PanelPicker::command_for_widget(&state, id), None);
        assert_eq!(
            snapshot.hit_test(jian_widgets::Point2D::new(
                node.rect.origin.x + node.rect.size.x / 2.0,
                node.rect.origin.y + node.rect.size.y / 2.0,
            )),
            None
        );
        assert!(!EMPTY_SUGGESTION_IDS.contains(&id));
    }
}

#[test]
fn picker_home_stays_inside_narrow_split_fallback_geometry() {
    let mut state = state_with_session();
    state.presentation.secondary_sidebar_open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 760.0, 420.0, Insets::ZERO);
    assert_eq!(snapshot.layout.review_panel.size.x, 0.0);
    let home =
        PanelPicker::home_layout(snapshot.layout.primary_surface, &state).expect("picker home");
    assert!(snapshot.layout.primary_surface.contains(home.rect.origin));
    assert!(home.rect.max_x() <= snapshot.layout.primary_surface.max_x());
    assert!(home.rect.max_y() <= snapshot.layout.primary_surface.max_y());
}

#[test]
fn projectless_home_entries_are_disabled_for_accessibility_and_clicks() {
    let mut state = demo_state();
    state.current_session = None;
    state.active_workspace = None;
    state.projectless_workspace_root = None;
    state.projects.clear();
    state.presentation.secondary_sidebar_open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);

    for id in [
        SECONDARY_HOME_REVIEW_ID,
        SECONDARY_HOME_TERMINAL_ID,
        SECONDARY_HOME_BROWSER_ID,
        SECONDARY_HOME_FILES_ID,
        SECONDARY_HOME_SIDE_TASK_ID,
    ] {
        let node = snapshot.node(id).expect("disabled projectless home item");
        assert!(node.disabled);
        assert!(node.actions.is_empty());
        assert_eq!(PanelPicker::command_for_widget(&state, id), None);
        assert_eq!(
            snapshot.hit_test(jian_widgets::Point2D::new(
                node.rect.origin.x + node.rect.size.x / 2.0,
                node.rect.origin.y + node.rect.size.y / 2.0,
            )),
            None
        );
    }
}

#[test]
fn terminal_secondary_uses_real_terminal_input_and_close_nodes() {
    let mut state = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::Terminal);
    state.presentation.secondary_sidebar_open = true;
    state.terminal.open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);

    let terminal = snapshot.node(TERMINAL_ID).expect("terminal input node");
    let close = snapshot
        .node(TERMINAL_SECONDARY_CLOSE_ID)
        .expect("terminal close node");
    assert!(terminal.rect.size.x > 0.0 && terminal.rect.size.y > 0.0);
    assert!(close.actions.contains(&accesskit::Action::Click));
    assert_eq!(
        snapshot.hit_test(jian_widgets::Point2D::new(
            terminal.rect.origin.x + 8.0,
            terminal.rect.origin.y + 8.0,
        )),
        Some(TERMINAL_ID)
    );
}
