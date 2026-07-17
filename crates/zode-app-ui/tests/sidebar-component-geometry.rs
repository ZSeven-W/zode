use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, ProjectState, ShellRoute};
use zode_app_ui::{ProjectSidebar, SemanticIcon, SidebarRowTarget, ZodeTheme};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    Text(String, Point2D),
    Svg(String, Point2D, f32, f32),
    FillRound(Rect, f32, Color),
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
        self.operations.push(PaintOp::Text(text, origin));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, radius: f32, color: Color) {
        self.operations
            .push(PaintOp::FillRound(rect, radius, color));
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn fill_drop_shadow(&mut self, _rect: Rect, _radius: f32, _blur: f32, _color: Color) {}
    fn stroke_svg_path(
        &mut self,
        path: &str,
        top_left: Point2D,
        size: f32,
        _color: Color,
        width: f32,
    ) {
        self.operations
            .push(PaintOp::Svg(path.into(), top_left, size, width));
    }
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _offset: Point2D) {}
    fn resize(&mut self, _width: u32, _height: u32) {}
    fn dpi_scale(&self) -> f32 {
        1.0
    }
}

fn sidebar_state() -> zode_app_model::ZodeAppState {
    let mut state = demo_state();
    state.projects.clear();
    state.threads.clear();
    state.pinned_sessions.clear();
    state.current_session = None;
    state.presentation.route = ShellRoute::Conversation;
    state
}

#[test]
fn sidebar_uses_reference_vertical_rhythm_and_edge_insets() {
    let rect = Rect::xywh(0.0, 0.0, 292.0, 800.0);
    let layout = ProjectSidebar::layout(rect, &sidebar_state());

    assert_eq!(layout.brand, Rect::xywh(16.0, 38.0, 260.0, 47.0));
    assert_eq!(layout.navigation_rows[0].rect.origin.y, 86.0);
    assert_eq!(layout.navigation_rows[0].rect.size.y, 30.0);
    for pair in layout.navigation_rows.windows(2) {
        assert_eq!(pair[1].rect.origin.y - pair[0].rect.origin.y, 31.0);
    }
    assert_eq!(
        layout.sections[0].rect,
        Rect::xywh(16.0, 283.0, 260.0, 28.0)
    );
}

#[test]
fn project_rows_use_regular_icons_and_codex_aligned_labels() {
    let mut state = sidebar_state();
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let session = SessionLocator::new(state.host.node_id, "sidebar-geometry");
    state.projects.push(ProjectState {
        workspace_uri: workspace.clone(),
        expanded: true,
        available: true,
        last_opened_ms: 1,
    });
    state.threads.push(ThreadSummary {
        session: session.clone(),
        workspace_uri: workspace,
        title: "像素对齐任务".into(),
        updated_at_ms: 2,
        status: ThreadStatus::Idle,
    });
    state.current_session = Some(session);

    let rect = Rect::xywh(0.0, 0.0, 292.0, 800.0);
    let layout = ProjectSidebar::layout(rect, &state);
    let project = layout
        .rows
        .iter()
        .find(|row| matches!(row.target, SidebarRowTarget::Project(_)))
        .unwrap();
    let session = layout
        .rows
        .iter()
        .find(|row| matches!(row.target, SidebarRowTarget::Session(_)))
        .unwrap();
    assert_eq!(project.rect.origin.x, 8.0);
    assert_eq!(session.rect.origin.x, 32.0);
    assert_eq!(project.rect.size.y, 30.0);
    assert_eq!(session.rect.origin.y - project.rect.origin.y, 31.0);

    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();
    ProjectSidebar::paint(&mut painter, rect, &state, &theme);
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Svg(path, origin, 14.0, width)
            if path == SemanticIcon::Folder.path()
                && origin.x == 16.0
                && *width == SemanticIcon::Folder.stroke_width()
    )));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Text(text, origin) if text == "zode" && origin.x == 40.0
    )));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::Text(text, origin) if text == "像素对齐任务" && origin.x == 40.0
    )));
    assert!(painter.operations.iter().any(|operation| matches!(
        operation,
        PaintOp::FillRound(active, 8.0, color)
            if *active == session.rect && *color == theme.sidebar_row_selected
    )));
}
