use std::collections::BTreeSet;

use zode_app_model::{
    demo_state, AppCommand, PreviewKind, PreviewState, PreviewTarget, ProjectState, SecondaryPane,
    TranscriptState,
};
use zode_app_ui::{
    Insets, PanelPicker, RectExt, ThreadHeader, WorkspaceLayout, WorkspaceSnapshot,
    PANEL_PICKER_ID, TERMINAL_ID, TERMINAL_SECONDARY_CLOSE_ID,
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
        Some(AppCommand::ToggleSecondaryMenu)
    );
}

#[test]
fn menu_truthfully_enables_real_panes_and_disables_missing_contracts() {
    let mut state = state_with_session();
    state.presentation.secondary_menu_open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let anchor = ThreadHeader::layout(snapshot.layout.top_bar, &state)
        .panel_picker
        .unwrap();
    let menu = PanelPicker::menu_layout(anchor.rect, snapshot.layout.viewport, &state).unwrap();
    assert_eq!(menu.items.len(), 7);

    for pane in [
        SecondaryPane::Environment,
        SecondaryPane::Review,
        SecondaryPane::Terminal,
        SecondaryPane::DocumentPreview,
    ] {
        assert!(
            menu.items
                .iter()
                .find(|item| item.pane == pane)
                .unwrap()
                .enabled
        );
    }
    for pane in [
        SecondaryPane::Browser,
        SecondaryPane::Files,
        SecondaryPane::SideTask,
    ] {
        let item = menu.items.iter().find(|item| item.pane == pane).unwrap();
        assert!(!item.enabled);
        assert!(item.unavailable_reason.is_some());
        assert_eq!(PanelPicker::command_for_widget(&state, item.id), None);
        assert!(snapshot.node(item.id).is_some_and(|node| node.disabled));
        assert_eq!(
            snapshot.hit_test(jian_widgets::Point2D::new(
                item.rect.origin.x + item.rect.size.x / 2.0,
                item.rect.origin.y + item.rect.size.y / 2.0,
            )),
            None
        );
    }
}

#[test]
fn menu_items_switch_or_close_the_selected_real_pane() {
    let mut state = state_with_session();
    state.presentation.secondary_menu_open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 1_800.0, 1_080.0, Insets::ZERO);
    let anchor = ThreadHeader::layout(snapshot.layout.top_bar, &state)
        .panel_picker
        .unwrap();
    let menu = PanelPicker::menu_layout(anchor.rect, snapshot.layout.viewport, &state).unwrap();
    let terminal = menu
        .items
        .iter()
        .find(|item| item.pane == SecondaryPane::Terminal)
        .unwrap();
    assert_eq!(
        PanelPicker::command_for_widget(&state, terminal.id),
        Some(AppCommand::OpenSecondary(SecondaryPane::Terminal))
    );

    state.presentation.secondary_pane = Some(SecondaryPane::Terminal);
    assert_eq!(
        PanelPicker::command_for_widget(&state, terminal.id),
        Some(AppCommand::CloseSecondary)
    );
}

#[test]
fn picker_menu_and_terminal_pane_stay_inside_narrow_geometry() {
    let mut state = state_with_session();
    state.presentation.secondary_menu_open = true;
    let snapshot = WorkspaceSnapshot::build(&state, 760.0, 420.0, Insets::ZERO);
    let anchor = ThreadHeader::layout(snapshot.layout.top_bar, &state)
        .panel_picker
        .unwrap();
    let menu = PanelPicker::menu_layout(anchor.rect, snapshot.layout.viewport, &state).unwrap();
    assert!(snapshot.layout.viewport.contains(menu.rect.origin));
    assert!(menu.rect.max_x() <= snapshot.layout.viewport.max_x());
    assert!(menu.rect.max_y() <= snapshot.layout.viewport.max_y());

    let terminal = WorkspaceLayout::compute_presentation(
        760.0,
        420.0,
        Insets::ZERO,
        zode_app_model::ShellRoute::Conversation,
        Some(SecondaryPane::Terminal),
    );
    assert_eq!(terminal.review_panel.size.x, 0.0);
    assert!(terminal.primary_surface.size.x > 0.0);
}

#[test]
fn terminal_secondary_uses_real_terminal_input_and_close_nodes() {
    let mut state = state_with_session();
    state.presentation.secondary_pane = Some(SecondaryPane::Terminal);
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
