use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{IntegrationsTab, ProjectState, SettingsCategory, ShellRoute, demo_state};
use zode_app_ui::{ProjectSidebar, RectExt, ZodeTheme, group_sessions};
use zode_node_protocol::{NodeId, SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    FillRound(Rect, Color),
    Text(String, Point2D, Color),
}

#[derive(Default)]
struct CapturePainter {
    operations: Vec<PaintOp>,
}

impl Painter for CapturePainter {
    fn begin_frame(&mut self) {}
    fn end_frame(&mut self) {}
    fn fill_rect(&mut self, _rect: Rect, _color: Color) {}
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
    fn stroke_svg_path(
        &mut self,
        _d: &str,
        _top_left: Point2D,
        _size: f32,
        _color: Color,
        _width: f32,
    ) {
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
fn local_settings_is_painted_in_the_bottom_footer() {
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint(
        &mut painter,
        Rect::xywh(0.0, 0.0, 240.0, 600.0),
        &demo_state(),
        &ZodeTheme::light(),
    );

    let settings_origin = painter.operations.iter().find_map(|operation| {
        let PaintOp::Text(text, origin, _) = operation else {
            return None;
        };
        (text == "本地设置").then_some(*origin)
    });
    assert!(settings_origin.is_some_and(|origin| origin.y > 560.0));
    assert!(
        !painter.operations.iter().any(
            |operation| matches!(operation, PaintOp::Text(text, _, _) if text.contains("账户"))
        )
    );
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
            PaintOp::FillRound(rect, color) if *color == theme.tokens.row_selected => Some(*rect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(selected_rects, vec![ProjectSidebar::footer_rect(rect)]);
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
            if *selected == plugins.rect && *color == theme.tokens.row_selected
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
            if *selected == current_row.rect && *color == theme.tokens.row_selected
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
            PaintOp::FillRound(rect, color) if *color == theme.tokens.row_selected => Some(*rect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(selected_rects, vec![project.rect]);
}

#[test]
fn dynamic_rows_stop_before_the_stable_footer() {
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
    let footer = ProjectSidebar::footer_rect(rect);

    let rows = ProjectSidebar::dynamic_row_layout(rect, &state);

    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.rect.max_y() <= footer.origin.y));
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

    assert!(
        !painter
            .operations
            .iter()
            .any(|operation| matches!(operation, PaintOp::Text(text, _, _) if text == "本地设置"))
    );
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
            PaintOp::FillRound(_, _) => None,
        })
        .collect::<Vec<_>>();
    for label in ["新", "已", "插", "站", "拉", "聊", "设"] {
        assert!(labels.contains(&label), "missing compact label {label}");
    }
    assert!(!labels.contains(&"拉取请求"));
    assert!(!labels.contains(&"本地设置"));
}

#[test]
fn coming_soon_navigation_is_muted_while_implemented_items_stay_foreground() {
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
    for label in ["新建任务", "插件"] {
        assert_eq!(text_color(label), Some(theme.sidebar_foreground));
    }
    for label in ["已安排", "站点", "拉取请求", "聊天"] {
        assert_eq!(text_color(label), Some(theme.tokens.muted_foreground));
    }
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
