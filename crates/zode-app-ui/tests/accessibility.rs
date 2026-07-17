use std::collections::{BTreeMap, BTreeSet};

use accesskit::{Action, NodeId, Role};
use jian_core::CursorHint;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{
    AppCommand, ComingSoonFeature, EnvironmentEntry, EnvironmentSectionKind, EnvironmentSnapshot,
    IntegrationsTab, LoadState, SecondaryPane, SessionDiffState, SessionPresentationState,
    SettingsCategory, ShellRoute, TranscriptItem, TranscriptState,
};
use zode_app_ui::{
    accessibility_tree, ApprovalCard, Composer, EnvironmentPanel, FocusDirection, Insets,
    InteractionNode, ProjectSidebar, RectExt, SettingsPanel, ThreadTranscript, WidgetId,
    WorkspaceLayout, WorkspaceSnapshot,
};
use zode_node_protocol::{
    DiffFile, DiffFileStatus, DiffSnapshot, SessionLocator, ThreadStatus, ThreadSummary, ToolCall,
    ToolStatus, WorkspaceUri,
};

const NAVIGATION_ID: WidgetId = WidgetId(10);
const COMPOSER_ID: WidgetId = WidgetId(20);
const SEND_ID: WidgetId = WidgetId(30);

fn interaction_fixture() -> WorkspaceSnapshot {
    let layout = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);
    let composer_rect = Rect::xywh(
        layout.composer.min_x() + 7.0,
        layout.composer.min_y() + 11.0,
        layout.composer.width() - 14.0,
        layout.composer.height() - 22.0,
    );
    WorkspaceSnapshot {
        layout,
        nodes: vec![
            InteractionNode {
                id: NAVIGATION_ID,
                rect: layout.sidebar,
                role: Role::Navigation,
                name: "项目".into(),
                value: None,
                actions: vec![Action::Focus],
                focus_order: Some(0),
                cursor: CursorHint::Default,
                toggled: None,
            },
            InteractionNode {
                id: COMPOSER_ID,
                rect: composer_rect,
                role: Role::TextInput,
                name: "要求后续变更".into(),
                value: Some("draft".into()),
                actions: vec![Action::Focus, Action::SetValue],
                focus_order: Some(1),
                cursor: CursorHint::Text,
                toggled: None,
            },
            InteractionNode {
                id: SEND_ID,
                rect: Rect::xywh(
                    layout.composer.max_x() - 40.0,
                    layout.composer.max_y() - 40.0,
                    32.0,
                    32.0,
                ),
                role: Role::Button,
                name: "发送".into(),
                value: None,
                actions: vec![Action::Click, Action::Focus],
                focus_order: Some(2),
                cursor: CursorHint::Pointer,
                toggled: None,
            },
        ],
        focused: Some(COMPOSER_ID),
    }
}

#[test]
fn generated_widget_ids_are_stable_unique_and_cover_core_interactions() {
    let state = zode_app_model::demo_state();
    let first = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let second = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let first_ids = first.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let second_ids = second.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let unique = first_ids.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(unique.len(), first_ids.len(), "WidgetId values are unique");
    assert!(
        first_ids.iter().all(|id| id.0 != 0),
        "interactive WidgetId values never collide with the AccessKit root",
    );
    assert_eq!(first_ids, second_ids, "WidgetId values are stable");
    assert!(first_ids.len() >= 3, "core shell interactions are present");
}

#[test]
fn each_page_has_a_useful_default_focus_target() {
    let mut state = zode_app_model::demo_state();
    state.composer.focused = false;
    state.shell.page = zode_app_model::ShellPage::Settings;
    state.presentation.route = ShellRoute::Conversation;
    let conversation = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(conversation.focused, Some(zode_app_ui::COMPOSER_ID));

    state.presentation.route = ShellRoute::Terminal;
    state.terminal.focused = false;
    let terminal = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(terminal.focused, Some(zode_app_ui::TERMINAL_ID));

    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Skills);
    let integrations = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(integrations.focused, Some(WidgetId(71)));

    state.presentation.route = ShellRoute::ComingSoon(ComingSoonFeature::Sites);
    let coming_soon = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(coming_soon.focused, Some(WidgetId(5)));

    state.presentation.route = ShellRoute::Settings(SettingsCategory::Appearance);
    let settings = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(
        settings.focused,
        Some(SettingsPanel::category_widget_id(
            SettingsCategory::Appearance
        ))
    );
}

#[test]
fn typed_secondary_panes_expose_only_visible_shared_geometry() {
    let (mut state, session) = transcript_fixture();
    state.presentation.sessions.insert(
        session.clone(),
        SessionPresentationState {
            diff: SessionDiffState {
                dirty: false,
                load: LoadState::Ready(DiffSnapshot {
                    session: session.clone(),
                    files: vec![DiffFile {
                        path: "src/main.rs".into(),
                        status: DiffFileStatus::Modified,
                        additions: 2,
                        deletions: 1,
                    }],
                    unified: String::new(),
                }),
            },
            ..SessionPresentationState::default()
        },
    );
    state.presentation.secondary_pane = Some(SecondaryPane::Environment);

    let environment = WorkspaceSnapshot::build(&state, 1800.0, 1080.0, Insets::ZERO);
    let panel_layout = EnvironmentPanel::layout(environment.layout.context_panel, &state);
    assert_eq!(environment.layout.context_panel.width(), 300.0);
    assert_eq!(
        environment.node(WidgetId(100)).unwrap().rect,
        panel_layout.close_button
    );
    assert_eq!(
        environment.node(WidgetId(101)).unwrap().rect,
        panel_layout.review_button.unwrap()
    );
    assert!(environment.node(WidgetId(60)).is_some());
    assert!(environment.node(WidgetId(61)).is_some());
    assert!(environment
        .node(EnvironmentPanel::section_widget_id(
            &session,
            EnvironmentSectionKind::Changes,
        ))
        .is_some());
    assert!(environment
        .node(EnvironmentPanel::section_widget_id(
            &session,
            EnvironmentSectionKind::Host,
        ))
        .is_some());
    for absent in [
        EnvironmentSectionKind::Subagents,
        EnvironmentSectionKind::BackgroundProcesses,
        EnvironmentSectionKind::Sources,
    ] {
        assert!(environment
            .node(EnvironmentPanel::section_widget_id(&session, absent))
            .is_none());
    }

    state
        .presentation
        .sessions
        .get_mut(&session)
        .expect("current presentation")
        .context = LoadState::Ready(EnvironmentSnapshot {
        workspace_uri: WorkspaceUri::new("file:///repo/zode").unwrap(),
        branch: None,
        subagents: (0..20)
            .map(|index| EnvironmentEntry {
                id: format!("agent-{index}"),
                label: format!("agent {index}"),
                value: None,
            })
            .collect(),
        background_processes: Vec::new(),
        sources: Vec::new(),
    });
    let overflow = WorkspaceSnapshot::build(&state, 1800.0, 1080.0, Insets::ZERO);
    let overflow_panel = EnvironmentPanel::layout(overflow.layout.context_panel, &state);
    let review = overflow_panel
        .review_button
        .expect("the canonical diff stays reviewable");
    for kind in [
        EnvironmentSectionKind::Changes,
        EnvironmentSectionKind::Host,
        EnvironmentSectionKind::Comparisons,
        EnvironmentSectionKind::Subagents,
    ] {
        let section = overflow
            .node(EnvironmentPanel::section_widget_id(&session, kind))
            .expect("the visible part of a real section is accessible");
        assert!(section.rect.max_y() <= overflow_panel.content.max_y());
        assert!(section.rect.max_y() <= review.origin.y - 8.0);
    }

    let collapsed = WorkspaceSnapshot::build(&state, 1399.0, 900.0, Insets::ZERO);
    assert_eq!(collapsed.layout.context_panel.width(), 0.0);
    for id in [WidgetId(60), WidgetId(100), WidgetId(101)] {
        assert!(collapsed.node(id).is_none());
    }
    assert!(collapsed.node(WidgetId(61)).is_some());

    state.presentation.secondary_pane = Some(SecondaryPane::Review);
    let review = WorkspaceSnapshot::build(&state, 1800.0, 1080.0, Insets::ZERO);
    let close = review.node(WidgetId(102)).expect("visible review close");
    assert!(review.layout.review_panel.contains(rect_center(close.rect)));

    let collapsed = WorkspaceSnapshot::build(&state, 1399.0, 900.0, Insets::ZERO);
    assert_eq!(collapsed.layout.review_panel.width(), 0.0);
    assert_eq!(
        collapsed.node(WidgetId(102)).unwrap().rect,
        zode_app_ui::ReviewPanel::layout(collapsed.layout.primary_surface).close_button
    );
    for hidden in [WidgetId(20), WidgetId(21), WidgetId(60), WidgetId(61)] {
        assert!(
            collapsed.node(hidden).is_none(),
            "review fallback must not expose hidden node {hidden:?}"
        );
    }
    assert!(collapsed
        .nodes
        .iter()
        .all(|node| !node.name.contains("question")));
    assert_eq!(collapsed.focused, Some(WidgetId(102)));
}

#[test]
fn extreme_typed_snapshots_never_expose_empty_or_out_of_bounds_nodes() {
    let (mut state, _) = transcript_fixture();
    for route in [
        ShellRoute::Conversation,
        ShellRoute::Terminal,
        ShellRoute::Integrations(IntegrationsTab::Plugins),
        ShellRoute::Settings(SettingsCategory::General),
        ShellRoute::ComingSoon(ComingSoonFeature::Chats),
    ] {
        state.presentation.route = route;
        state.presentation.secondary_pane = Some(SecondaryPane::Review);
        for (width, height, insets) in [
            (0.0, 0.0, Insets::ZERO),
            (
                390.0,
                180.0,
                Insets {
                    top: 500.0,
                    right: 500.0,
                    bottom: 500.0,
                    left: 500.0,
                },
            ),
        ] {
            let snapshot = WorkspaceSnapshot::build(&state, width, height, insets);
            for node in &snapshot.nodes {
                assert!(node.rect.min_x().is_finite() && node.rect.min_y().is_finite());
                assert!(node.rect.width() > 0.0 && node.rect.height() > 0.0);
                assert!(node.rect.min_x() >= 0.0 && node.rect.min_y() >= 0.0);
                assert!(node.rect.max_x() <= snapshot.layout.viewport.max_x());
                assert!(node.rect.max_y() <= snapshot.layout.viewport.max_y());
            }
        }
    }
}

#[test]
fn generated_nodes_carry_role_name_value_actions_and_layout_rect() {
    let snapshot =
        WorkspaceSnapshot::build(&zode_app_model::demo_state(), 1221.0, 992.0, Insets::ZERO);
    let composer = snapshot
        .nodes
        .iter()
        .find(|node| node.role == Role::TextInput)
        .expect("composer interaction node");

    assert_eq!(composer.name, "要求后续变更");
    assert_eq!(composer.value.as_deref(), Some(""));
    assert!(composer.actions.contains(&Action::Focus));
    assert!(composer.actions.contains(&Action::SetValue));
    assert_eq!(
        composer.rect,
        Composer::layout(
            snapshot.layout.composer,
            &zode_app_model::demo_state().composer,
        )
        .input
    );
}

#[test]
fn hit_testing_reads_interaction_rects_from_the_snapshot() {
    let snapshot = interaction_fixture();
    let composer_rect = snapshot.node(COMPOSER_ID).unwrap().rect;
    let center = Point2D::new(
        composer_rect.min_x() + composer_rect.width() / 2.0,
        composer_rect.min_y() + composer_rect.height() / 2.0,
    );
    let layout_only_point = Point2D::new(
        snapshot.layout.composer.min_x() + 2.0,
        snapshot.layout.composer.min_y() + 2.0,
    );

    assert_eq!(snapshot.hit_test(layout_only_point), None);
    assert_eq!(snapshot.hit_test(center), Some(COMPOSER_ID));
}

#[test]
fn focus_order_and_tab_traversal_work_in_both_directions() {
    let snapshot = interaction_fixture();

    assert_eq!(
        snapshot.focusable_ids(),
        vec![NAVIGATION_ID, COMPOSER_ID, SEND_ID],
    );
    assert_eq!(
        snapshot.move_focus(Some(NAVIGATION_ID), FocusDirection::Forward),
        Some(COMPOSER_ID),
    );
    assert_eq!(
        snapshot.move_focus(Some(COMPOSER_ID), FocusDirection::Backward),
        Some(NAVIGATION_ID),
    );
    assert_eq!(
        snapshot.move_focus(Some(SEND_ID), FocusDirection::Forward),
        Some(NAVIGATION_ID),
    );
    assert_eq!(
        snapshot.move_focus(Some(NAVIGATION_ID), FocusDirection::Backward),
        Some(SEND_ID),
    );
}

#[test]
fn accesskit_tree_uses_physical_root_bounds() {
    let snapshot =
        WorkspaceSnapshot::build(&zode_app_model::demo_state(), 390.0, 844.0, Insets::ZERO);
    let update = accessibility_tree(&snapshot, 2.0);
    let (_, root) = update.nodes.first().expect("tree has a window root");
    let bounds = root.bounds().expect("root carries physical bounds");

    assert_eq!((bounds.x0, bounds.y0), (0.0, 0.0));
    assert_eq!((bounds.x1, bounds.y1), (780.0, 1688.0));
}

#[test]
fn accesskit_composer_node_preserves_id_physical_rect_and_semantics() {
    let snapshot = interaction_fixture();
    let update = accessibility_tree(&snapshot, 2.0);
    let (_, composer) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == NodeId(COMPOSER_ID.0))
        .expect("composer maps WidgetId directly to AccessKit NodeId");
    let bounds = composer.bounds().expect("composer has physical bounds");

    assert_eq!(composer.role(), Role::TextInput);
    assert_eq!(composer.label(), Some("要求后续变更"));
    assert_eq!(composer.value(), Some("draft"));
    assert!(composer.supports_action(Action::Focus));
    assert!(composer.supports_action(Action::SetValue));
    let composer_rect = snapshot.node(COMPOSER_ID).unwrap().rect;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (
            f64::from(composer_rect.min_x()) * 2.0,
            f64::from(composer_rect.min_y()) * 2.0,
            f64::from(composer_rect.max_x()) * 2.0,
            f64::from(composer_rect.max_y()) * 2.0,
        ),
    );
}

#[test]
fn markdown_height_and_follow_tail_share_one_virtual_layout() {
    let (mut state, session) = transcript_fixture();
    let transcript = state.transcripts.get_mut(&session).unwrap();
    transcript.items = vec![
        TranscriptItem::AssistantText(
            (0..20)
                .map(|index| format!("paragraph {index} with enough words to wrap"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ),
        TranscriptItem::Status {
            code: "after".into(),
            message: "after markdown".into(),
        },
    ];
    transcript.item_heights.clear();
    transcript.follow_tail = false;
    let viewport = Rect::xywh(0.0, 0.0, 360.0, 2_000.0);
    let rows = ThreadTranscript::visible_item_layout(viewport, transcript);
    assert!(rows[0].rect.size.y > 72.0);
    assert!(rows[1].rect.origin.y >= rows[0].rect.max_y() + 12.0);

    transcript.items = (0..40)
        .map(|index| TranscriptItem::Status {
            code: format!("status-{index}"),
            message: format!("status {index}"),
        })
        .collect();
    transcript.follow_tail = true;
    let viewport = Rect::xywh(0.0, 0.0, 360.0, 240.0);
    let tail = ThreadTranscript::visible_item_layout(viewport, transcript);
    assert!(tail.first().unwrap().index > 0);
    let command =
        ThreadTranscript::scroll_command(session, viewport, transcript, &BTreeMap::new(), -100.0);
    assert!(matches!(
        command,
        AppCommand::SetTranscriptViewport {
            follow_tail: false,
            ..
        }
    ));
}

#[test]
fn visible_transcript_semantics_share_painted_item_rects_without_entering_tab_order() {
    let (state, session) = transcript_fixture();
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let transcript = state.transcripts.get(&session).unwrap();
    let rows = ThreadTranscript::visible_item_layout(snapshot.layout.transcript, transcript);

    for (index, expected_label) in ["question", "answer", "working", "ready", "failed"]
        .into_iter()
        .enumerate()
    {
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.name.contains(expected_label))
            .expect("each visible readable item has a semantic node");
        assert_eq!(node.rect, rows[index].visible_rect);
        assert!(node.actions.is_empty());
        assert_eq!(node.focus_order, None);
        assert_eq!(snapshot.hit_test(rect_center(node.rect)), None);
    }
}

#[test]
fn tool_and_approval_controls_share_visible_geometry_and_emit_commands() {
    let (mut state, session) = transcript_fixture();
    let transcript = state.transcripts.get_mut(&session).unwrap();
    transcript.items = vec![
        TranscriptItem::Tool(tool("tool-stable")),
        TranscriptItem::Approval {
            id: "approval-stable".into(),
            tool: "write_file".into(),
        },
    ];
    transcript.item_heights = vec![72.0, 72.0];
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let rows = ThreadTranscript::visible_item_layout(
        snapshot.layout.transcript,
        state.transcripts.get(&session).unwrap(),
    );

    let tool_node = snapshot
        .nodes
        .iter()
        .find(|node| node.name.contains("read_file"))
        .expect("tool node");
    assert_eq!(tool_node.rect, rows[0].visible_rect);
    assert!(tool_node.actions.contains(&Action::Click));
    assert!(tool_node.actions.contains(&Action::Focus));
    assert_eq!(
        snapshot.hit_test(rect_center(tool_node.rect)),
        Some(tool_node.id)
    );
    assert_eq!(
        ThreadTranscript::command_for_widget(&state, tool_node.id),
        Some(AppCommand::SetToolExpanded {
            session: session.clone(),
            tool_id: "tool-stable".into(),
            expanded: true,
        }),
    );

    let buttons = ApprovalCard::button_layout(rows[1].rect);
    for button in buttons {
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.rect == button.rect)
            .expect("approval button semantic node");
        assert_eq!(node.name, button.label);
        assert!(node.actions.contains(&Action::Click));
        assert!(node.actions.contains(&Action::Focus));
        assert_eq!(snapshot.hit_test(rect_center(button.rect)), Some(node.id));
        assert_eq!(
            ThreadTranscript::command_for_widget(&state, node.id),
            Some(ApprovalCard::command("approval-stable", button.action)),
        );
    }
}

#[test]
fn partially_visible_transcript_controls_are_clipped_to_the_viewport() {
    let (mut state, session) = transcript_fixture();
    let transcript = state.transcripts.get_mut(&session).unwrap();
    transcript.items = std::iter::once(TranscriptItem::Tool(tool("edge-tool")))
        .chain((0..20).map(|index| TranscriptItem::Status {
            code: format!("status-{index}"),
            message: format!("status {index}"),
        }))
        .collect();
    transcript.item_heights = vec![72.0; transcript.items.len()];
    transcript.scroll_offset = 36.0;
    transcript.follow_tail = false;
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 600.0, Insets::ZERO);
    let tool_node = snapshot
        .nodes
        .iter()
        .find(|node| node.name.contains("read_file"))
        .unwrap();
    assert_eq!(tool_node.rect.origin.y, snapshot.layout.transcript.origin.y);
    assert!(
        tool_node.rect.origin.y + tool_node.rect.size.y
            <= snapshot.layout.transcript.origin.y + snapshot.layout.transcript.size.y
    );
    assert_eq!(
        snapshot.hit_test(Point2D::new(
            tool_node.rect.origin.x + 4.0,
            snapshot.layout.transcript.origin.y - 1.0,
        )),
        None,
    );

    let transcript = state.transcripts.get_mut(&session).unwrap();
    transcript.items[0] = TranscriptItem::Approval {
        id: "clipped-approval".into(),
        tool: "write_file".into(),
    };
    transcript.scroll_offset = 60.0;
    let snapshot = WorkspaceSnapshot::build(&state, 1221.0, 600.0, Insets::ZERO);
    assert!(snapshot
        .nodes
        .iter()
        .all(|node| { !matches!(node.name.as_str(), "允许一次" | "始终允许" | "拒绝") }));
}

#[test]
fn stable_dynamic_ids_follow_domain_keys_instead_of_list_order() {
    let (mut state, session) = transcript_fixture();
    let transcript = state.transcripts.get_mut(&session).unwrap();
    transcript.items = vec![
        TranscriptItem::Tool(tool("tool-stable")),
        TranscriptItem::Approval {
            id: "approval-stable".into(),
            tool: "write_file".into(),
        },
    ];
    let first = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let tool_id = first
        .nodes
        .iter()
        .find(|node| node.name.contains("read_file"))
        .unwrap()
        .id;
    let allow_id = first
        .nodes
        .iter()
        .find(|node| node.name == "允许一次")
        .unwrap()
        .id;

    state
        .transcripts
        .get_mut(&session)
        .unwrap()
        .items
        .insert(0, TranscriptItem::Thinking("prepended".into()));
    let second = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    assert_eq!(
        second
            .nodes
            .iter()
            .find(|node| node.name.contains("read_file"))
            .unwrap()
            .id,
        tool_id,
    );
    assert_eq!(
        second
            .nodes
            .iter()
            .find(|node| node.name == "允许一次")
            .unwrap()
            .id,
        allow_id,
    );
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
        ProjectSidebar::footer_rect(snapshot.layout.sidebar)
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
fn mixed_static_dynamic_and_transcript_ids_are_nonzero_unique_and_repeatable() {
    let (mut state, session) = transcript_fixture();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(zode_app_model::ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Semantic task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    state.transcripts.get_mut(&session).unwrap().items.extend([
        TranscriptItem::Tool(tool("unique-tool")),
        TranscriptItem::Approval {
            id: "unique-approval".into(),
            tool: "write_file".into(),
        },
    ]);
    let first = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let second = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let ids = first.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let unique = ids.iter().copied().collect::<BTreeSet<_>>();

    assert!(ids.iter().all(|id| id.0 != 0));
    assert_eq!(unique.len(), ids.len());
    assert_eq!(
        ids,
        second.nodes.iter().map(|node| node.id).collect::<Vec<_>>()
    );
}

fn transcript_fixture() -> (zode_app_model::ZodeAppState, SessionLocator) {
    let mut state = zode_app_model::demo_state();
    let session = SessionLocator::new(state.host.node_id, "semantic-transcript");
    state.current_session = Some(session.clone());
    state.transcripts.insert(
        session.clone(),
        TranscriptState {
            items: vec![
                TranscriptItem::UserText("question".into()),
                TranscriptItem::AssistantText("answer".into()),
                TranscriptItem::Thinking("working".into()),
                TranscriptItem::Status {
                    code: "ready".into(),
                    message: "ready".into(),
                },
                TranscriptItem::Error {
                    message: "failed".into(),
                    retryable: false,
                },
            ],
            item_heights: vec![72.0; 5],
            ..TranscriptState::default()
        },
    );
    (state, session)
}

fn tool(id: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_file".into(),
        status: ToolStatus::Completed,
        summary: "read complete".into(),
        detail: None,
    }
}

fn rect_center(rect: Rect) -> Point2D {
    Point2D::new(
        rect.origin.x + rect.size.x / 2.0,
        rect.origin.y + rect.size.y / 2.0,
    )
}
