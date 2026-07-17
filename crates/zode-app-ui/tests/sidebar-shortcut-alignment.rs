use jian_widgets::{Color, Painter, Point2D, Rect, TextLayout};
use zode_app_model::{demo_state, ProjectState};
use zode_app_ui::{ProjectSidebar, ZodeTheme, NEW_SESSION_ID};
use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

#[derive(Debug, Clone, PartialEq)]
enum PaintOp {
    FillRound(Rect, Color),
    Text(String, Point2D),
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
        self.operations.push(PaintOp::Text(
            layout
                .runs()
                .iter()
                .map(|run| run.content.as_str())
                .collect(),
            origin,
        ));
    }
    fn clip_rect(&mut self, _rect: Rect) {}
    fn stroke_line(&mut self, _from: Point2D, _to: Point2D, _color: Color, _width: f32) {}
    fn fill_round_rect(&mut self, rect: Rect, _radius: f32, color: Color) {
        self.operations.push(PaintOp::FillRound(rect, color));
    }
    fn stroke_round_rect(&mut self, _rect: Rect, _radius: f32, _color: Color, _width: f32) {}
    fn fill_drop_shadow(&mut self, _rect: Rect, _radius: f32, _blur: f32, _color: Color) {}
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
fn new_task_shortcut_is_centered_inside_its_keycap() {
    let state = demo_state();
    let rect = Rect::xywh(0.0, 0.0, 240.0, 800.0);
    let theme = ZodeTheme::light();
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint_with_interaction(
        &mut painter,
        rect,
        &state,
        None,
        Some(NEW_SESSION_ID),
        false,
        &theme,
    );

    let keycap = painter
        .operations
        .iter()
        .find_map(|operation| match operation {
            PaintOp::FillRound(rect, color)
                if rect.size.x == 35.0
                    && rect.size.y == 20.0
                    && *color == theme.tokens.muted.with_alpha(0.78) =>
            {
                Some(*rect)
            }
            _ => None,
        })
        .expect("new-task shortcut keycap is painted");

    assert_text_ink_centered(&mut painter, "⌘N", keycap);
}

#[test]
fn numbered_shortcut_is_centered_inside_its_trailing_slot() {
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
        .expect("shortcut session row exists");
    let shortcut_slot = Rect::xywh(
        row.rect.origin.x + row.rect.size.x - 30.0,
        row.rect.origin.y,
        24.0,
        row.rect.size.y,
    );
    let mut painter = CapturePainter::default();

    ProjectSidebar::paint_with_interaction(
        &mut painter,
        rect,
        &state,
        None,
        None,
        true,
        &ZodeTheme::light(),
    );

    assert_text_ink_centered(&mut painter, "⌘1", shortcut_slot);
}

fn assert_text_ink_centered(painter: &mut CapturePainter, label: &str, container: Rect) {
    let origin = painter
        .operations
        .iter()
        .find_map(|operation| match operation {
            PaintOp::Text(text, origin) if text == label => Some(*origin),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{label} is painted"));
    let metrics = painter.measure_text_metrics_family_styled(label, 10.0, "system-ui", 400, false);
    let ink_center_x = origin.x + metrics.width * 0.5;
    let ink_center_y = origin.y + metrics.ink_center().expect("finite ink bounds");
    let container_center_x = container.origin.x + container.size.x * 0.5;
    let container_center_y = container.origin.y + container.size.y * 0.5;

    assert!(
        (ink_center_x - container_center_x).abs() <= 0.01,
        "{label} must be horizontally centered: ink={ink_center_x}, container={container_center_x}"
    );
    assert!(
        (ink_center_y - container_center_y).abs() <= 0.01,
        "{label} must be vertically centered: ink={ink_center_y}, container={container_center_y}"
    );
}
