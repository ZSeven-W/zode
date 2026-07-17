use std::time::{SystemTime, UNIX_EPOCH};

use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{
    demo_state, AppCommand, EnvironmentSnapshot, IntegrationsTab, LoadState, ProjectState,
    SettingsCategory, ShellRoute,
};
use zode_app_ui::{
    group_sessions, ProjectSidebar, RectExt, SidebarRowTarget, SidebarSection, ZodeTheme,
};
use zode_node_protocol::{NodeId, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    Fill(Rect, Color),
    FillRound(Rect, Color),
    Shadow(Rect),
    Text(String, Point2D, Color),
    Svg(String, Point2D, f32, Color),
}

#[derive(Default)]
struct CapturePainter {
    operations: Vec<PaintOp>,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, rect: Rect, color: Color) {
        self.operations.push(PaintOp::Fill(rect, color));
    }
    fn stroke_rect(&mut self, _rect: Rect, _color: Color, _width: f32) {}
    fn draw_text(&mut self, layout: &TextLayout, origin: Point2D) {
        let text = layout
            .runs()
            .iter()
            .map(|run| run.content.as_str())
            .collect::<String>();
        let color = layout.runs().first().map_or(Color::TRANSPARENT, |run| {
            Color::rgba_u8(
                run.color.r(),
                run.color.g(),
                run.color.b(),
                f32::from(run.color.a()) / 255.0,
            )
        });
        self.operations.push(PaintOp::Text(text, origin, color));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, color: Color) {
        self.operations.push(PaintOp::FillRound(rect, color));
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn fill_drop_shadow(&mut self, rect: Rect, _radius: f32, _blur: f32, _color: Color) {
        self.operations.push(PaintOp::Shadow(rect));
    }
    fn stroke_svg_path(
        &mut self,
        d: &str,
        top_left: Point2D,
        size: f32,
        color: Color,
        _width: f32,
    ) {
        self.operations
            .push(PaintOp::Svg(d.to_owned(), top_left, size, color));
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

#[test]
fn sessions_group_by_workspace_newest_first() {
    let groups = group_sessions(fixture_sessions());

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].workspace_uri.as_str(), "file:///repo/zode");
    assert!(groups[0].sessions[0].updated_at_ms >= groups[0].sessions[1].updated_at_ms);
    assert_eq!(groups[1].workspace_uri.as_str(), "file:///repo/openpencil");
}

#[test]
fn empty_session_list_has_no_placeholder_group() {
    assert!(group_sessions(Vec::new()).is_empty());
}

#[test]
fn local_profile_is_painted_in_the_fixed_bottom_footer() {
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 240.0, 600.0),
        &demo_state(),
        &ZodeTheme::light(),
    );

    let profile_origin = painter.operations.iter().find_map(|operation| {
        let PaintOp::Text(text, origin, _) = operation else {
            return None;
        };
        (text == "本地").then_some(*origin)
    });
    assert!(profile_origin.is_some_and(|origin| origin.y > 560.0));
    assert!(!painter
        .operations
        .iter()
        .any(|operation| matches!(operation, PaintOp::Text(text, _, _) if text.contains("账户"))));
}

#[test]
fn wide_sidebar_reserves_titlebar_space_and_uses_navigation_icons() {
    let rect = Rect::xywh(0.0, 0.0, 240.0, 600.0);
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(&mut painter, rect, &demo_state(), &ZodeTheme::light());

    assert!(ProjectSidebar::navigation_row_layout(rect)[0].rect.origin.y >= 80.0);
    let brand_y = painter
        .operations
        .iter()
        .find_map(|operation| match operation {
            PaintOp::Text(text, origin, _) if text == "Zode" => Some(origin.y),
            _ => None,
        });
    assert!(brand_y.is_some_and(|y| y >= 48.0));
    assert_eq!(
        painter
            .operations
            .iter()
            .filter(|operation| matches!(operation, PaintOp::Svg(..)))
            .count(),
        8
    );
    let new_task_x = painter
        .operations
        .iter()
        .find_map(|operation| match operation {
            PaintOp::Text(text, origin, _) if text == "新建任务" => Some(origin.x),
            _ => None,
        });
    assert!(new_task_x.is_some_and(|x| x >= 40.0));
}

#[test]
fn settings_route_selects_the_footer_instead_of_new_session() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Settings(SettingsCategory::Appearance);
    let theme = ZodeTheme::light();
    let rect = Rect::xywh(0.0, 0.0, 240.0, 600.0);
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(&mut painter, rect, &state, &theme);

    let selected_rects = painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::FillRound(rect, color) if *color == theme.sidebar_row_selected => Some(*rect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(selected_rects, vec![ProjectSidebar::profile_rect(rect)]);
    assert!(!painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(selected, _) if *selected == ProjectSidebar::navigation_row_layout(rect)[0].rect
    )));
}

#[test]
fn every_integrations_tab_selects_the_plugins_navigation_item() {
    let mut state = demo_state();
    state.presentation.route = ShellRoute::Integrations(IntegrationsTab::Skills);
    let theme = ZodeTheme::light();
    let rect = Rect::xywh(0.0, 0.0, 240.0, 600.0);
    let plugins = ProjectSidebar::navigation_row_layout(rect)[2];
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(&mut painter, rect, &state, &theme);

    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(selected, color)
            if *selected == plugins.rect && *color == theme.sidebar_row_selected
    )));
}

#[test]
fn conversation_route_highlights_the_real_current_session() {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "current-session");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace.clone(),
        title: "zode 桌面端".into(),
        updated_at_ms: 2,
        status: ThreadStatus::Idle,
    });
    state.active_workspace = Some(workspace);
    state.current_session = Some(session.clone());
    state.presentation.route = ShellRoute::Conversation;
    let theme = ZodeTheme::light();
    let rect = Rect::xywh(0.0, 0.0, 240.0, 600.0);
    let current_row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| row.target == zode_app_ui::SidebarRowTarget::Session(session.clone()))
        .expect("current session row is visible");
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(&mut painter, rect, &state, &theme);

    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(selected, color)
            if *selected == current_row.rect && *color == theme.sidebar_row_selected
    )));
    assert!(!painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(selected, _) if *selected == ProjectSidebar::navigation_row_layout(rect)[0].rect
    )));
}

#[test]
fn active_project_replaces_the_new_task_selection_when_no_session_is_open() {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: false,
        available: true,
        last_opened_ms: 1,
    });
    state.active_workspace = Some(workspace);
    state.presentation.route = ShellRoute::Conversation;
    let theme = ZodeTheme::light();
    let rect = Rect::xywh(0.0, 0.0, 240.0, 600.0);
    let project = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .next()
        .expect("active project row is visible");
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(&mut painter, rect, &state, &theme);

    let selected_rects = painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::FillRound(rect, color) if *color == theme.sidebar_row_selected => Some(*rect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(selected_rects, vec![project.rect]);
}

#[test]
fn overflow_rows_scroll_instead_of_disappearing() {
    let mut state = demo_state();
    for index in 0..20 {
        state.projects.push(ProjectState {
            workspace_uri: WorkspaceUri::new(format!("file:///repo/project-{index}")).unwrap(),
            expanded: false,
            available: true,
            last_opened_ms: index,
        });
    }
    let rect = Rect::xywh(0.0, 0.0, 240.0, 480.0);
    let layout = ProjectSidebar::layout(rect, &state);

    assert!(!layout.rows.is_empty());
    assert!(layout.content_height > layout.scroll_viewport.size.y);
    assert!(layout.max_scroll > 0.0);
}

#[test]
fn footer_rect_stays_inside_one_thirty_nine_and_forty_pixel_rails() {
    for height in [1.0, 39.0, 40.0] {
        let rail = Rect::xywh(12.0, 20.0, 240.0, height);
        let footer = ProjectSidebar::footer_rect(rail);

        assert!(footer.origin.y >= rail.origin.y, "height {height}");
        assert!(footer.max_y() <= rail.max_y(), "height {height}");
        assert!(footer.size.y >= 0.0, "height {height}");
    }
}

#[test]
fn zero_height_footer_is_not_painted() {
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 240.0, 39.0),
        &demo_state(),
        &ZodeTheme::light(),
    );

    assert!(!painter
        .operations
        .iter()
        .any(|operation| matches!(operation, PaintOp::Text(text, _, _) if text == "本地设置")));
}

#[test]
fn compact_sidebar_keeps_navigation_and_settings_readable() {
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 64.0, 600.0),
        &demo_state(),
        &ZodeTheme::light(),
    );

    let labels = painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::Text(text, _, _) => Some(text.as_str()),
            PaintOp::Fill(_, _)
            | PaintOp::FillRound(_, _)
            | PaintOp::Shadow(_)
            | PaintOp::Svg(..) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        painter
            .operations
            .iter()
            .filter(|operation| matches!(operation, PaintOp::Svg(..)))
            .count(),
        7
    );
    assert!(!labels.contains(&"拉取请求"));
    assert!(!labels.contains(&"本地设置"));
}

#[test]
fn navigation_destinations_remain_enabled_before_their_pages_are_complete() {
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 240.0, 600.0),
        &demo_state(),
        &theme,
    );

    let text_color = |label: &str| {
        painter
            .operations
            .iter()
            .find_map(|operation| match operation {
                PaintOp::Text(text, _, color) if text == label => Some(*color),
                _ => None,
            })
    };
    for label in ["新建任务", "已安排", "插件", "站点", "拉取请求", "聊天"] {
        assert_eq!(text_color(label), Some(theme.sidebar_foreground));
    }
}

#[test]
fn sidebar_three_zones_keep_brand_scroll_content_and_profile_anchored() {
    let rect = Rect::xywh(0.0, 0.0, 240.0, 837.0);
    let state = demo_state();
    let first = ProjectSidebar::layout(rect, &state);
    let taller = ProjectSidebar::layout(Rect::xywh(0.0, 0.0, 240.0, 1_077.0), &state);

    assert_eq!(
        first.navigation_rows[0].rect,
        taller.navigation_rows[0].rect
    );
    assert_eq!(
        first.scroll_viewport.origin.y,
        taller.scroll_viewport.origin.y
    );
    assert_eq!(first.footer.max_y(), rect.max_y());
    assert_eq!(taller.footer.max_y(), 1_077.0);
    assert_eq!(first.footer.size.y, 46.0);
    assert!(first.scroll_viewport.max_y() <= first.footer.origin.y);
}

#[test]
fn native_material_sidebar_paints_translucent_rows_and_footer_hairline() {
    let mut state = demo_state();
    state.projects.clear();
    state.threads.clear();
    state.projects.push(ProjectState {
        workspace_uri: WorkspaceUri::new("file:///repo/material-hover").unwrap(),
        expanded: false,
        available: true,
        last_opened_ms: 1,
    });
    state.presentation.route = ShellRoute::Settings(SettingsCategory::General);
    let rect = Rect::xywh(0.0, 0.0, 240.0, 600.0);
    let theme = ZodeTheme::light().with_native_sidebar_material();
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(&mut painter, rect, &state, &theme);

    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(selected, color)
            if *selected == ProjectSidebar::profile_rect(rect)
                && *color == Color::BLACK.with_alpha(0.05)
    )));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Fill(divider, color)
            if *divider == Rect::xywh(0.0, 554.0, 240.0, 1.0)
                && *color == Color::BLACK.with_alpha(0.09)
    )));

    let hover_row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .next()
        .expect("material hover project is visible");
    let mut hovered = CapturePainter::default();
    ProjectSidebar::paint_with_interaction(
        &mut hovered,
        rect,
        &state,
        None,
        Some(hover_row.id),
        false,
        &theme,
    );
    assert!(hovered.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(row, color)
            if *row == hover_row.rect && *color == Color::BLACK.with_alpha(0.025)
    )));
}

#[test]
fn pinned_project_and_projectless_tasks_render_in_distinct_order() {
    let mut state = demo_state();
    let project = WorkspaceUri::new("file:///repo/zode").unwrap();
    let scratch_root = WorkspaceUri::new("file:///tmp/zode-tasks").unwrap();
    state.projectless_workspace_root = Some(scratch_root.clone());
    state.projects.push(ProjectState {
        workspace_uri: project.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 10,
    });
    let pinned = SessionLocator::new(state.host.node_id, "pinned");
    let project_session = SessionLocator::new(state.host.node_id, "project");
    let projectless = SessionLocator::new(state.host.node_id, "task");
    state.threads.extend([
        ThreadSummary {
            session: pinned.clone(),
            workspace_uri: project.clone(),
            title: "Pinned".into(),
            updated_at_ms: 30,
            status: ThreadStatus::Running,
        },
        ThreadSummary {
            session: project_session.clone(),
            workspace_uri: project,
            title: "Project".into(),
            updated_at_ms: 20,
            status: ThreadStatus::Idle,
        },
        ThreadSummary {
            session: projectless.clone(),
            workspace_uri: WorkspaceUri::new(format!("{}/task", scratch_root.as_str())).unwrap(),
            title: "Task".into(),
            updated_at_ms: 10,
            status: ThreadStatus::Failed,
        },
    ]);
    state.pinned_sessions.insert(pinned.clone());

    let layout = ProjectSidebar::layout(Rect::xywh(0.0, 0.0, 240.0, 1_077.0), &state);
    let sections = layout
        .sections
        .iter()
        .map(|section| section.section)
        .collect::<Vec<_>>();
    assert_eq!(
        sections,
        vec![
            SidebarSection::Pinned,
            SidebarSection::Projects,
            SidebarSection::Tasks
        ]
    );
    let sessions = layout
        .rows
        .iter()
        .filter_map(|row| row.session().cloned())
        .collect::<Vec<_>>();
    assert_eq!(sessions, vec![pinned.clone(), project_session, projectless]);
    assert_eq!(ProjectSidebar::shortcut_session(&state, 1), Some(pinned));
}

#[test]
fn command_shortcuts_appear_only_while_command_is_held() {
    let mut state = demo_state();
    state.projects.clear();
    state.threads.clear();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "shortcut-task");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Shortcut task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| row.session() == Some(&session))
        .unwrap();
    let theme = ZodeTheme::light();

    let mut normal = CapturePainter::default();
    ProjectSidebar::paint_with_interaction(&mut normal, rect, &state, None, None, false, &theme);
    assert!(!painted_labels(&normal)
        .iter()
        .any(|label| label.starts_with('⌘')));

    let mut hovered = CapturePainter::default();
    ProjectSidebar::paint_with_interaction(
        &mut hovered,
        rect,
        &state,
        None,
        Some(row.id),
        false,
        &theme,
    );
    assert!(!painted_labels(&hovered)
        .iter()
        .any(|label| label.starts_with('⌘')));

    let mut command_held = CapturePainter::default();
    ProjectSidebar::paint_with_interaction(
        &mut command_held,
        rect,
        &state,
        None,
        None,
        true,
        &theme,
    );
    assert!(painted_labels(&command_held).contains(&"⌘1"));
}

#[test]
fn session_actions_and_scroll_emit_typed_commands() {
    let mut state = demo_state();
    let scratch_root = WorkspaceUri::new("file:///tmp/zode-tasks").unwrap();
    state.projectless_workspace_root = Some(scratch_root.clone());
    let session = SessionLocator::new(state.host.node_id, "task-action");
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: WorkspaceUri::new(format!("{}/task-action", scratch_root.as_str())).unwrap(),
        title: "Task".into(),
        updated_at_ms: 1,
        status: ThreadStatus::Idle,
    });
    let row = ProjectSidebar::dynamic_row_layout(Rect::xywh(0.0, 0.0, 240.0, 600.0), &state)
        .into_iter()
        .find(|row| matches!(row.target, SidebarRowTarget::Task(_)))
        .unwrap();

    assert_eq!(
        ProjectSidebar::command_for_widget(&state, row.pin_id.unwrap()),
        Some(AppCommand::SetSessionPinned {
            session: session.clone(),
            pinned: true,
        })
    );
    assert_eq!(
        ProjectSidebar::command_for_widget(&state, row.archive_id.unwrap()),
        Some(AppCommand::SetSessionArchived {
            session,
            archived: true,
        })
    );

    for index in 0..20 {
        state.projects.push(ProjectState {
            workspace_uri: WorkspaceUri::new(format!("file:///repo/project-{index}")).unwrap(),
            expanded: false,
            available: true,
            last_opened_ms: index,
        });
    }
    let rect = Rect::xywh(0.0, 0.0, 240.0, 480.0);
    assert!(matches!(
        ProjectSidebar::scroll_command(rect, &state, 100.0),
        Some(AppCommand::SetSidebarScroll { offset }) if offset > 0.0
    ));
}

#[test]
fn action_hover_keeps_row_active_and_matches_the_reference_preview_card() {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/openpencil").unwrap();
    let session = SessionLocator::new(state.host.node_id, "hover-card");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace.clone(),
        title: "Codex Companion Task: implement every sidebar hover detail".into(),
        updated_at_ms: now_ms() - 2 * 24 * 60 * 60 * 1_000,
        status: ThreadStatus::Idle,
    });
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .context = LoadState::Ready(EnvironmentSnapshot {
        workspace_uri: workspace,
        branch: Some("v0.8.1".into()),
        subagents: Vec::new(),
        background_processes: Vec::new(),
        sources: Vec::new(),
    });
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| row.session() == Some(&session))
        .unwrap();
    let pin = row.pin_id.unwrap();
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint_with_interaction(
        &mut painter,
        rect,
        &state,
        None,
        Some(pin),
        false,
        &theme,
    );
    ProjectSidebar::paint_hover_overlay(&mut painter, rect, &state, None, Some(pin), &theme);

    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(active, color)
            if *active == row.rect && *color == theme.sidebar_row_hover
    )));
    let preview_card = painter
        .operations
        .iter()
        .find_map(|operation| match operation {
            PaintOp::FillRound(card, color)
                if card.size.x == 226.0
                    && card.size.y == 88.0
                    && *color == theme.tokens.popover =>
            {
                Some(*card)
            }
            _ => None,
        })
        .expect("preview card is painted");
    assert_eq!(preview_card.origin.y, row.rect.origin.y);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Shadow(card) if card.size.x == 226.0 && card.size.y == 88.0
    )));
    let labels = painted_labels(&painter);
    assert!(labels.contains(&"置顶任务"));
    assert!(labels.contains(&"2 天"));
    assert!(labels.contains(&"openpencil"));
    assert!(labels.contains(&"v0.8.1"));
    assert!(labels
        .iter()
        .any(|label| label.starts_with("Codex Companion") && label.ends_with('…')));
}

#[test]
fn action_tooltips_follow_pin_state_and_archive_action() {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/openpencil").unwrap();
    let session = SessionLocator::new(state.host.node_id, "hover-actions");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Hover actions".into(),
        updated_at_ms: now_ms(),
        status: ThreadStatus::Idle,
    });
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let theme = ZodeTheme::light();
    let row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| row.session() == Some(&session))
        .unwrap();
    let mut archive_painter = CapturePainter::default();
    ProjectSidebar::paint_hover_overlay(
        &mut archive_painter,
        rect,
        &state,
        None,
        row.archive_id,
        &theme,
    );
    assert!(painted_labels(&archive_painter).contains(&"归档任务"));

    state.pinned_sessions.insert(session.clone());
    let row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| row.session() == Some(&session))
        .unwrap();
    let mut pin_painter = CapturePainter::default();
    ProjectSidebar::paint_hover_overlay(&mut pin_painter, rect, &state, None, row.pin_id, &theme);
    assert!(painted_labels(&pin_painter).contains(&"取消置顶"));
}

#[test]
fn hover_card_never_leaks_a_branch_from_another_workspace() {
    let mut state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/openpencil").unwrap();
    let session = SessionLocator::new(state.host.node_id, "hover-branch-scope");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "Scoped branch".into(),
        updated_at_ms: now_ms(),
        status: ThreadStatus::Idle,
    });
    state
        .presentation
        .sessions
        .entry(session.clone())
        .or_default()
        .context = LoadState::Ready(EnvironmentSnapshot {
        workspace_uri: WorkspaceUri::new("file:///repo/other").unwrap(),
        branch: Some("secret-branch".into()),
        subagents: Vec::new(),
        background_processes: Vec::new(),
        sources: Vec::new(),
    });
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let row = ProjectSidebar::dynamic_row_layout(rect, &state)
        .into_iter()
        .find(|row| row.session() == Some(&session))
        .unwrap();
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint_hover_overlay(
        &mut painter,
        rect,
        &state,
        None,
        Some(row.id),
        &ZodeTheme::light(),
    );

    assert!(!painted_labels(&painter).contains(&"secret-branch"));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(card, _) if card.size.x == 226.0 && card.size.y == 68.0
    )));
}

fn painted_labels(painter: &CapturePainter) -> Vec<&str> {
    painter
        .operations
        .iter()
        .filter_map(|operation| match operation {
            PaintOp::Text(text, _, _) => Some(text.as_str()),
            PaintOp::Fill(_, _)
            | PaintOp::FillRound(_, _)
            | PaintOp::Shadow(_)
            | PaintOp::Svg(..) => None,
        })
        .collect()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn fixture_sessions() -> Vec<ThreadSummary> {
    let node_id = NodeId::parse("00000000-0000-0000-0000-000000000001").unwrap();
    [
        ("old-zode", "file:///repo/zode", 100),
        ("openpencil", "file:///repo/openpencil", 200),
        ("new-zode", "file:///repo/zode", 300),
    ]
    .into_iter()
    .map(|(id, workspace, updated_at_ms)| ThreadSummary {
        session: SessionLocator::new(node_id, id),
        workspace_uri: WorkspaceUri::new(workspace).unwrap(),
        title: id.into(),
        updated_at_ms,
        status: ThreadStatus::Idle,
    })
    .collect()
}
